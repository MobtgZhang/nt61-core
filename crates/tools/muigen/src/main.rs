use clap::{Parser, Subcommand, ValueEnum};
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{absolute, Path, PathBuf};
use std::process::{self, Command, Output};

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Action,
}

#[derive(Subcommand)]
enum Action {
    /// Split a single-language Win32 module with the Windows SDK.
    Split {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        config: PathBuf,
        #[arg(long, value_enum, ignore_case = true)]
        lang: Language,
        #[arg(long)]
        out: PathBuf,
        /// Path to muirct.exe. Defaults to a lookup on PATH.
        #[arg(long)]
        muirct: Option<PathBuf>,
    },
    /// Split multiple language editions into one directory. The first edition supplies the LN file.
    Batch {
        /// Repeat once per language, with the fallback edition first.
        #[arg(long, required = true)]
        input: Vec<PathBuf>,
        /// Repeat in the same order as --input.
        #[arg(long, required = true)]
        config: Vec<PathBuf>,
        /// Repeat in the same order as --input.
        #[arg(long, required = true, value_enum, ignore_case = true)]
        lang: Vec<Language>,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        muirct: Option<PathBuf>,
    },
    /// Compile a resource script into the main module's .res and a linked .mui satellite.
    Build {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        config: PathBuf,
        #[arg(long, value_enum, ignore_case = true)]
        lang: Language,
        /// The final executable or DLL filename, including its extension.
        #[arg(long)]
        module: OsString,
        #[arg(long, value_enum, ignore_case = true)]
        machine: Machine,
        #[arg(long)]
        out: PathBuf,
        /// Path to rc.exe. Defaults to a lookup on PATH.
        #[arg(long)]
        rc: Option<PathBuf>,
        /// Path to the Microsoft link.exe. Defaults to a lookup on PATH.
        #[arg(long)]
        link: Option<PathBuf>,
    },
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Language {
    #[value(name = "zh-CN")]
    ZhCn,
    #[value(name = "en-US")]
    EnUs,
}

#[derive(Clone, Copy, ValueEnum)]
enum Machine {
    X86,
    X64,
    Arm64,
}

impl Machine {
    fn link_flag(self) -> &'static str {
        match self {
            Self::X86 => "/MACHINE:X86",
            Self::X64 => "/MACHINE:X64",
            Self::Arm64 => "/MACHINE:ARM64",
        }
    }
}

impl Language {
    fn tag(self) -> &'static str {
        match self {
            Self::ZhCn => "zh-CN",
            Self::EnUs => "en-US",
        }
    }

    fn langid(self) -> &'static str {
        match self {
            Self::ZhCn => "0x0804",
            Self::EnUs => "0x0409",
        }
    }
}

enum Failure {
    Message(String),
    Sdk { tool: PathBuf, output: Output },
}

impl From<String> for Failure {
    fn from(message: String) -> Self {
        Self::Message(message)
    }
}

fn path_error(action: &str, path: &Path, error: impl fmt::Display) -> String {
    format!("{action} '{}': {error}", path.display())
}

fn existing_file(path: &Path, kind: &str) -> Result<PathBuf, String> {
    let path = absolute(path).map_err(|error| path_error(kind, path, error))?;
    let metadata = fs::metadata(&path).map_err(|error| path_error(kind, &path, error))?;
    if !metadata.is_file() {
        return Err(path_error(kind, &path, "expected a regular file"));
    }
    Ok(path)
}

fn validate_config(path: &Path, language: Language, require_fallback: bool) -> Result<(), String> {
    let error = |detail: &str| path_error("invalid .rcconfig", path, detail);
    let bytes = fs::read(path).map_err(|detail| path_error("read .rcconfig", path, detail))?;
    let xml = if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
        let (pairs, remainder) = bytes[2..].as_chunks::<2>();
        if !remainder.is_empty() {
            return Err(error("incomplete UTF-16 code unit"));
        }
        let decode = if bytes[0] == 0xff {
            u16::from_le_bytes
        } else {
            u16::from_be_bytes
        };
        let units: Vec<_> = pairs.iter().copied().map(decode).collect();
        String::from_utf16(&units).map_err(|detail| error(&detail.to_string()))?
    } else {
        String::from_utf8(bytes).map_err(|detail| error(&detail.to_string()))?
    };
    let document = roxmltree::Document::parse(xml.trim_start_matches('\u{feff}'))
        .map_err(|detail| error(&detail.to_string()))?;
    let root = document.root_element();
    if !root.has_tag_name("localization") {
        return Err(error("expected a localization element"));
    }
    let mut configurations = root
        .children()
        .filter(|node| node.has_tag_name("resources"))
        .flat_map(|node| {
            node.children()
                .filter(|child| child.has_tag_name("win32Resources"))
        });
    let configuration = configurations
        .next()
        .ok_or_else(|| error("missing win32Resources element"))?;
    if configurations.next().is_some() {
        return Err(error("multiple win32Resources elements"));
    }
    if let Some(configured) = configuration.attribute("language") {
        if !configured.eq_ignore_ascii_case(language.tag()) {
            return Err(error(&format!(
                "language '{configured}' conflicts with --lang {}",
                language.tag()
            )));
        }
    }
    if require_fallback
        && (!configuration
            .attribute("ultimateFallbackLanguage")
            .is_some_and(|tag| tag.eq_ignore_ascii_case(language.tag()))
            || configuration.attribute("ultimateFallbackLocation") != Some("external"))
    {
        return Err(error(&format!(
            "the first batch configuration must set ultimateFallbackLanguage='{}' and ultimateFallbackLocation='external'",
            language.tag()
        )));
    }
    Ok(())
}

fn find_sdk(name: &str, flag: &str, explicit: Option<&Path>) -> Result<PathBuf, String> {
    let action = format!("locate {name}");
    if let Some(path) = explicit {
        return existing_file(path, &action);
    }
    std::env::var_os("PATH")
        .and_then(|paths| {
            std::env::split_paths(&paths)
                .find_map(|dir| existing_file(&dir.join(name), &action).ok())
        })
        .ok_or_else(|| {
            format!("{name} was not found on PATH. Install the Microsoft build tools or pass --{flag} PATH.")
        })
}

fn run_sdk(command: &mut Command) -> Result<(), Failure> {
    let tool = PathBuf::from(command.get_program());
    let output = command.output().map_err(|error| {
        path_error(
            &format!(
                "start {}",
                tool.file_name().unwrap_or_default().to_string_lossy()
            ),
            &tool,
            error,
        )
    })?;
    if !output.status.success() {
        return Err(Failure::Sdk { tool, output });
    }
    Ok(())
}

fn require_output(tool: &str, artifact: &Path) -> Result<(), String> {
    if !fs::symlink_metadata(artifact)
        .is_ok_and(|metadata| metadata.is_file() && metadata.len() != 0)
    {
        return Err(path_error(
            &format!("{tool} reported success"),
            artifact,
            "missing or empty output file",
        ));
    }
    Ok(())
}

fn module_name(path: &Path) -> Result<&OsStr, String> {
    if !path
        .extension()
        .and_then(OsStr::to_str)
        .is_some_and(|ext| ext.eq_ignore_ascii_case("exe") || ext.eq_ignore_ascii_case("dll"))
    {
        return Err(path_error(
            "invalid module name",
            path,
            "expected .exe or .dll",
        ));
    }
    path.file_name()
        .ok_or_else(|| path_error("invalid module name", path, "missing file name"))
}

fn suffixed(name: &OsStr, suffix: &str) -> OsString {
    let mut result = name.to_os_string();
    result.push(suffix);
    result
}

fn satellite_path(delivery: &Path, language: Language, name: &OsStr) -> Result<PathBuf, String> {
    let directory = delivery.join(language.tag());
    fs::create_dir_all(&directory)
        .map_err(|error| path_error("create staged language directory", &directory, error))?;
    Ok(directory.join(suffixed(name, ".mui")))
}

#[cfg(windows)]
fn publish(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::MoveFileW;

    let source: Vec<_> = source.as_os_str().encode_wide().chain([0]).collect();
    let destination: Vec<_> = destination.as_os_str().encode_wide().chain([0]).collect();
    if unsafe { MoveFileW(source.as_ptr(), destination.as_ptr()) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn publish(source: &Path, destination: &Path) -> io::Result<()> {
    use rustix::fs::{renameat_with, RenameFlags, CWD};

    renameat_with(CWD, source, CWD, destination, RenameFlags::NOREPLACE).map_err(Into::into)
}

#[cfg(not(any(windows, target_os = "linux")))]
fn publish(_source: &Path, _destination: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "muigen requires Windows or Linux",
    ))
}

fn staged_delivery(
    out: &Path,
    generate: impl FnOnce(&Path, &Path) -> Result<(), Failure>,
) -> Result<PathBuf, Failure> {
    let out = absolute(out).map_err(|error| path_error("resolve output path", out, error))?;
    let occupied = "output already exists. Choose a new directory";
    match fs::symlink_metadata(&out) {
        Ok(_) => return Err(path_error("cannot publish", &out, occupied).into()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(path_error("inspect output path", &out, error).into()),
    }
    let out_name = out
        .file_name()
        .ok_or_else(|| path_error("resolve output path", &out, "missing directory name"))?;
    let parent = out
        .parent()
        .ok_or_else(|| path_error("resolve output path", &out, "missing parent directory"))?;
    let stage = tempfile::Builder::new()
        .prefix(".muigen-")
        .tempdir_in(parent)
        .map_err(|error| path_error("stage in output parent", parent, error))?;
    let staged_output = stage.path().join("delivery");
    generate(stage.path(), &staged_output)?;
    let staged_output = fs::canonicalize(&staged_output)
        .map_err(|error| path_error("resolve staging directory", &staged_output, error))?;
    let destination = fs::canonicalize(parent)
        .map_err(|error| path_error("resolve output parent", parent, error))?
        .join(out_name);
    publish(&staged_output, &destination).map_err(|error| {
        if fs::symlink_metadata(&out).is_ok() {
            path_error("cannot publish", &out, occupied)
        } else {
            path_error("publish output directory", &out, error)
        }
    })?;
    Ok(out)
}

fn split_module(
    input: &Path,
    config: &Path,
    language: Language,
    tool: &Path,
    stage: &Path,
    main: &Path,
    satellite: &Path,
) -> Result<(), Failure> {
    let scratch = stage.join(language.tag());
    fs::create_dir(&scratch)
        .map_err(|error| path_error("create staging directory", &scratch, error))?;
    let staged_input = scratch.join(module_name(input)?);
    fs::copy(input, &staged_input)
        .map_err(|error| path_error("stage input module", input, error))?;
    run_sdk(
        Command::new(tool)
            .arg("-q")
            .arg(config)
            .args(["-x", language.langid()])
            .arg(staged_input)
            .arg(main)
            .arg(satellite),
    )?;
    require_output("muirct.exe", main)?;
    require_output("muirct.exe", satellite)?;
    Ok(())
}

fn split_editions(
    inputs: &[PathBuf],
    configs: &[PathBuf],
    languages: &[Language],
    out: &Path,
    sdk: Option<&Path>,
) -> Result<PathBuf, Failure> {
    if inputs.len() != configs.len() || inputs.len() != languages.len() {
        return Err("specify one --input, --config and --lang per edition"
            .to_owned()
            .into());
    }
    if languages
        .iter()
        .enumerate()
        .any(|(index, lang)| languages[..index].contains(lang))
    {
        return Err("each --lang must appear only once".to_owned().into());
    }
    let name = module_name(&inputs[0])?;
    staged_delivery(out, |stage, delivery| {
        let main = delivery.join(name);
        let tool = find_sdk("muirct.exe", "muirct", sdk)?;
        for (index, ((input, config), &language)) in
            inputs.iter().zip(configs).zip(languages).enumerate()
        {
            let input = existing_file(input, "open input module")?;
            if module_name(&input)? != name {
                return Err(path_error(
                    "invalid batch input",
                    &input,
                    "editions must have the same module filename",
                )
                .into());
            }
            let config = existing_file(config, "open .rcconfig")?;
            validate_config(&config, language, index == 0 && inputs.len() > 1)?;
            let satellite = satellite_path(delivery, language, name)?;
            let alternate_main = stage.join(suffixed(OsStr::new(language.tag()), ".ln"));
            let output_main = if index == 0 { &main } else { &alternate_main };
            split_module(
                &input,
                &config,
                language,
                &tool,
                stage,
                output_main,
                &satellite,
            )?;
            if index != 0 {
                run_sdk(
                    Command::new(&tool)
                        .arg("-c")
                        .arg(&main)
                        .arg("-e")
                        .arg(&satellite),
                )?;
                require_output("muirct.exe", &satellite)?;
            }
        }
        Ok(())
    })
}

fn execute(action: Action) -> Result<PathBuf, Failure> {
    match action {
        Action::Split {
            input,
            config,
            lang,
            out,
            muirct,
        } => split_editions(&[input], &[config], &[lang], &out, muirct.as_deref()),
        Action::Batch {
            input,
            config,
            lang,
            out,
            muirct,
        } => {
            if input.len() < 2 {
                return Err("batch requires at least two editions".to_owned().into());
            }
            split_editions(&input, &config, &lang, &out, muirct.as_deref())
        }
        Action::Build {
            input,
            config,
            lang,
            module,
            machine,
            out,
            rc,
            link,
        } => {
            let name = module_name(Path::new(&module))?;
            if name != module
                || !Path::new(&input)
                    .extension()
                    .and_then(OsStr::to_str)
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("rc"))
            {
                return Err(
                    "--module requires a single filename and --input requires a .rc file"
                        .to_owned()
                        .into(),
                );
            }
            staged_delivery(&out, |stage, delivery| {
                let input = existing_file(&input, "open resource script")?;
                let config = existing_file(&config, "open .rcconfig")?;
                validate_config(&config, lang, false)?;
                let rc = find_sdk("rc.exe", "rc", rc.as_deref())?;
                let link = find_sdk("link.exe", "link", link.as_deref())?;
                let satellite = satellite_path(delivery, lang, name)?;
                let main_res = delivery.join(suffixed(name, ".res"));
                let language_res = stage.join("satellite.res");
                run_sdk(
                    Command::new(rc)
                        .args(["/nologo", "/l", lang.langid(), "/q"])
                        .arg(config)
                        .arg("/fo")
                        .arg(&main_res)
                        .arg("/fm")
                        .arg(&language_res)
                        .arg(input),
                )?;
                require_output("rc.exe", &main_res)?;
                require_output("rc.exe", &language_res)?;
                let mut link_output = OsString::from("/OUT:");
                link_output.push(&satellite);
                run_sdk(
                    Command::new(link)
                        .args([
                            "/NOLOGO",
                            "/DLL",
                            "/NOENTRY",
                            "/MANIFEST:NO",
                            machine.link_flag(),
                        ])
                        .arg(link_output)
                        .arg(language_res),
                )?;
                require_output("link.exe", &satellite)?;
                Ok(())
            })
        }
    }
}

fn main() {
    match execute(Cli::parse().command) {
        Ok(directory) => println!("Generated MUI package in {}", directory.display()),
        Err(Failure::Message(message)) => {
            eprintln!("muigen: {message}");
            process::exit(1);
        }
        Err(Failure::Sdk { tool, output }) => {
            eprintln!("muigen: '{}' failed ({})", tool.display(), output.status);
            let _ = io::stdout().write_all(&output.stdout);
            let _ = io::stderr().write_all(&output.stderr);
            process::exit(output.status.code().unwrap_or(1));
        }
    }
}
