//! Building real Win32 DLLs with `rust-lld`.
//!
//! # Why this exists
//!
//! The plan's stage B asks for *real* `user32.dll`, `gdi32.dll` and
//! friends in `C:\Windows\System32` — genuine PE32+ images whose
//! exported functions contain real code. Two obstacles stood in the
//! way:
//!
//!  * `x86_64-pc-windows-msvc` cannot **link** here (no `link.exe`,
//!    no import libraries), but it *can* **compile** — `rustc`
//!    happily emits COFF objects for it, because the COFF writer
//!    needs no system libraries.
//!  * `x86_64-unknown-none` produces ELF objects, and `lld`'s ELF
//!    driver refuses them in PE mode.
//!
//! Both obstacles have the same solution: **let `rust-lld` do the
//! linking, in `link.exe` mode**. `rust-lld` ships with the Rust
//! toolchain, so this needs no external toolchain and no network
//! access. It is not "an extra library" — it is part of the
//! toolchain that is already required to build the kernel.
//!
//! # The pipeline
//!
//! ```text
//!   Rust source
//!        │  rustc --target x86_64-pc-windows-msvc --emit=obj
//!        ▼
//!   COFF object (.obj)
//!        │  rust-lld -flavor link -dll -noentry
//!        ▼
//!   PE32+ DLL with .text / .rdata / .pdata and an export table
//! ```
//!
//! Imports are resolved through **import libraries** that `lld-link`
//! itself generates from a `.def` file:
//!
//! ```text
//!   kernel32.def  ──lld-link -def──▶  kernel32.lib  ──┐
//!                                                     ├──▶  user32.dll
//!   user32.obj   ────────────────────────────────────┘
//! ```
//!
//! The `.def` files are generated from the same specification that
//! `fs::win32_dlls` uses, so the import table of a built DLL and the
//! import table of a stub DLL can never drift apart.
//!
//! # The `lld-link` driver
//!
//! `lld-link` is driven through its **response file** syntax, which
//! avoids all shell-quoting problems and lets the argument list be
//! constructed programmatically.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

// =====================================================================
//  Toolchain discovery
// =====================================================================

/// The `rust-lld` driver, once discovered.
#[derive(Clone, Debug)]
pub struct LldLink {
    /// The path of the `rust-lld` binary.
    pub path: PathBuf,
}

/// Find the `rust-lld` binary of the active toolchain.
///
/// `rust-lld` lives next to the other toolchain binaries. The
/// `RUSTC` environment variable (set by `cargo` when it runs a build
/// script) points at the `rustc` binary, whose parent directory is
/// the toolchain's `bin` directory.
pub fn find_lld() -> Option<LldLink> {
    // 1. The directory of the running `rustc`.
    if let Ok(rustc) = std::env::var("RUSTC") {
        if let Some(bin) = Path::new(&rustc).parent() {
            let candidate = bin.join("rust-lld");
            if candidate.is_file() {
                return Some(LldLink { path: candidate });
            }
            // `rustc` may be a rustup shim in `~/.cargo/bin`; the
            // real binary is under the toolchain directory.
            if let Some(toolchain) = bin.parent().and_then(|p| p.parent()) {
                let candidate = toolchain
                    .join("lib")
                    .join("rustlib")
                    .join(std::env::consts::ARCH)
                    .join("unknown-linux-gnu")
                    .join("bin")
                    .join("rust-lld");
                if candidate.is_file() {
                    return Some(LldLink { path: candidate });
                }
            }
        }
    }

    // 2. Ask `rustc` where it lives.
    if let Ok(output) = Command::new("rustc").arg("--print").arg("sysroot").output() {
        if output.status.success() {
            let sysroot = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let candidate = Path::new(&sysroot)
                .join("lib")
                .join("rustlib")
                .join(std::env::consts::ARCH)
                .join("unknown-linux-gnu")
                .join("bin")
                .join("rust-lld");
            if candidate.is_file() {
                return Some(LldLink { path: candidate });
            }
        }
    }

    None
}

// =====================================================================
//  Response files
// =====================================================================

/// Write an `lld-link` response file.
///
/// The response file format is one argument per line; a path that
/// contains a space can be quoted. Writing the arguments ourselves
/// removes every quoting hazard.
pub fn write_response_file(path: &Path, arguments: &[String]) -> std::io::Result<()> {
    let mut text = String::new();
    for argument in arguments {
        if argument.contains(' ') || argument.contains('\t') {
            text.push('"');
            text.push_str(argument);
            text.push('"');
        } else {
            text.push_str(argument);
        }
        text.push('\n');
    }
    std::fs::write(path, text)
}

/// The result of running `lld-link`.
#[derive(Clone, Debug)]
pub struct LinkOutcome {
    /// The exit status.
    pub success: bool,
    /// The captured standard output.
    pub stdout: String,
    /// The captured standard error.
    pub stderr: String,
}

/// Run `lld-link` with the given arguments.
pub fn run_lld(lld: &LldLink, arguments: &[String]) -> std::io::Result<LinkOutcome> {
    let output = Command::new(&lld.path)
        .arg("-flavor")
        .arg("link")
        .args(arguments)
        .output()?;
    Ok(LinkOutcome {
        success: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    })
}

// =====================================================================
//  Import libraries
// =====================================================================

/// An import library: a module name and the symbols it provides.
#[derive(Clone, Debug)]
pub struct ImportLibrary {
    /// The DLL name as it appears in the import table.
    pub dll_name: &'static str,
    /// The symbols the DLL provides.
    pub symbols: &'static [&'static str],
}

/// Write a `.def` file describing an import library.
///
/// The `.def` file is the *authoritative* description of what the
/// DLL exports. `lld-link` turns it into an import library that the
/// linker can resolve against.
pub fn write_def_file(path: &Path, dll_name: &str, symbols: &[&str]) -> std::io::Result<()> {
    let mut text = String::new();
    text.push_str(&format!("LIBRARY {dll_name}\r\n"));
    text.push_str("EXPORTS\r\n");
    for symbol in symbols {
        text.push_str(symbol);
        text.push_str("\r\n");
    }
    std::fs::write(path, text)
}

/// Build an import library from a `.def` file.
pub fn build_import_library(
    lld: &LldLink,
    def_file: &Path,
    out_file: &Path,
) -> std::io::Result<LinkOutcome> {
    run_lld(
        lld,
        &[
            format!("-def:{}", def_file.display()),
            format!("-out:{}", out_file.display()),
            "-machine:x64".to_string(),
            "-implib".to_string(),
            out_file.display().to_string(),
        ],
    )
}

// =====================================================================
//  DLL linking
// =====================================================================

/// How a Ring 3 DLL is linked.
#[derive(Clone, Debug)]
pub struct LinkSpec {
    /// The DLL name, e.g. `"user32.dll"`.
    pub dll_name: &'static str,
    /// The preferred load address.
    pub image_base: u64,
    /// The COFF objects to link.
    pub objects: Vec<PathBuf>,
    /// The import libraries to link against.
    pub import_libraries: Vec<PathBuf>,
    /// The symbols to export, and the archive that defines them.
    pub exports: Vec<(String, PathBuf)>,
    /// The output path.
    pub output: PathBuf,
}

/// Link a Win32 DLL.
///
/// The `exports` list names the symbols to export and, for each, the
/// archive that defines it. `lld-link` can export a symbol from a
/// specific library with `/export:symbol=library`, which is what
/// makes it possible to export the Rust functions of a `staticlib`
/// under the exact Win32 names (`CreateWindowExW` and friends).
pub fn link_dll(lld: &LldLink, spec: &LinkSpec) -> std::io::Result<LinkOutcome> {
    let mut arguments: Vec<String> = Vec::new();
    arguments.push("-dll".to_string());
    arguments.push("-noentry".to_string());
    arguments.push("-machine:x64".to_string());
    arguments.push(format!("-base:0x{:x}", spec.image_base));
    arguments.push(format!("-out:{}", spec.output.display()));
    arguments.push("-subsystem:windows".to_string());
    // The DLLs must be relocatable so the loader can place them.
    arguments.push("-dynamicbase".to_string());
    arguments.push("-nxcompat".to_string());

    for object in &spec.objects {
        arguments.push(object.display().to_string());
    }
    for library in &spec.import_libraries {
        arguments.push(library.display().to_string());
    }
    for (symbol, archive) in &spec.exports {
        arguments.push(format!("-export:{symbol}={}", archive.display()));
    }

    run_lld(lld, &arguments)
}

// =====================================================================
//  The full pipeline
// =====================================================================

/// Where the build artifacts live.
#[derive(Clone, Debug)]
pub struct Workspace {
    /// The directory that holds the intermediate artifacts.
    pub dir: PathBuf,
}

impl Workspace {
    /// Create a workspace under `dir`.
    pub fn new(dir: impl Into<PathBuf>) -> std::io::Result<Self> {
        let dir = dir.into();
        std::fs::create_dir_all(&dir)?;
        Ok(Self { dir })
    }

    /// The path of an intermediate file.
    pub fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }
}

/// The outcome of a complete DLL build.
#[derive(Clone, Debug)]
pub struct DllBuildOutcome {
    /// The linked PE image.
    pub bytes: Vec<u8>,
    /// The `lld-link` output.
    pub link: LinkOutcome,
}

/// Compile a Rust `staticlib` into COFF objects for the Windows
/// target.
///
/// The `x86_64-pc-windows-msvc` target is used because `rustc`
/// emits **COFF** objects for it, which `lld-link` accepts. The
/// target cannot *link* here (there is no `link.exe` and no import
/// libraries), but the compiler never needs them.
pub fn compile_to_coff(
    cargo: &str,
    manifest_dir: &Path,
    package: &str,
    target: &str,
    release: bool,
) -> std::io::Result<std::io::Result<Vec<PathBuf>>> {
    let mut command = Command::new(cargo);
    command.current_dir(manifest_dir);
    command.arg("rustc");
    if release {
        command.arg("--release");
    }
    command.arg("--target").arg(target);
    command.arg("-p").arg(package);
    command.arg("--crate-type").arg("staticlib");
    command.arg("--").arg("--emit=obj");

    let output = command.output()?;
    if !output.status.success() {
        return Ok(Err(std::io::Error::other(
            String::from_utf8_lossy(&output.stderr).to_string(),
        )));
    }

    // The objects are written to
    // `target/<target>/<profile>/build/<package>-<hash>/out/*.o`.
    let profile = if release { "release" } else { "debug" };
    let target_dir = manifest_dir.join("target").join(target).join(profile);
    let mut objects = Vec::new();
    collect_objects(&target_dir, &mut objects)?;
    objects.sort();
    Ok(Ok(objects))
}

/// Find the COFF objects under `target_dir`.
fn collect_objects(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_objects(&path, out)?;
        } else if path
            .extension()
            .is_some_and(|ext| ext == "obj" || ext == "o")
        {
            out.push(path);
        }
    }
    Ok(())
}

/// The import libraries the Win32 DLLs need, in load order.
///
/// `ntdll.dll` is first because everything else imports from it.
pub fn default_import_libraries() -> Vec<ImportLibrary> {
    use super::win32_dlls;
    let mut libraries = Vec::new();
    for spec in win32_dlls::ALL_DLLS {
        let symbols: Vec<&str> = spec.exports.iter().map(|(name, _)| *name).collect();
        // The symbols have to outlive the `ImportLibrary`, so they are
        // leaked once at startup. The count is bounded by the
        // specification, so this is a handful of kilobytes at most.
        let leaked: &'static [&'static str] = Box::leak(
            symbols
                .into_iter()
                .map(|name| &*Box::leak(name.to_string().into_boxed_str()))
                .collect::<Vec<&'static str>>()
                .into_boxed_slice(),
        );
        libraries.push(ImportLibrary {
            dll_name: spec.file_name,
            symbols: leaked,
        });
    }
    libraries
}

/// The exports of a DLL, grouped by the archive that defines them.
pub fn exports_of(dll_name: &str) -> Vec<(String, PathBuf)> {
    use super::win32_dlls;
    let Some(spec) = win32_dlls::ALL_DLLS
        .iter()
        .find(|spec| spec.file_name == dll_name)
    else {
        return Vec::new();
    };
    // Every symbol of a DLL is defined by that DLL's own archive.
    let archive = PathBuf::from(format!("{}.lib", dll_name.trim_end_matches(".dll")));
    spec.exports
        .iter()
        .map(|(name, _)| ((*name).to_string(), archive.clone()))
        .collect()
}

/// A map from a DLL name to the symbols it exports.
pub fn export_map() -> BTreeMap<String, Vec<String>> {
    use super::win32_dlls;
    win32_dlls::ALL_DLLS
        .iter()
        .map(|spec| {
            (
                spec.file_name.to_string(),
                spec.exports
                    .iter()
                    .map(|(name, _)| (*name).to_string())
                    .collect(),
            )
        })
        .collect()
}
