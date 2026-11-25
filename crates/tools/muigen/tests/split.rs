#![cfg(target_os = "linux")]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

struct Fixture {
    root: TempDir,
    input: PathBuf,
    config: PathBuf,
    sdk: PathBuf,
    out: PathBuf,
}

impl Fixture {
    fn new(config: &str, sdk_script: &str) -> Self {
        let root = tempfile::tempdir().unwrap();
        let sdk_directory = root.path().join("Windows SDK");
        fs::create_dir(&sdk_directory).unwrap();
        let fixture = Self {
            input: root.path().join("My App.DLL"),
            config: root.path().join("my resources.rcconfig"),
            sdk: sdk_directory.join("muirct.exe"),
            out: root.path().join("distribution folder"),
            root,
        };
        fs::write(&fixture.input, b"source").unwrap();
        fs::write(&fixture.config, config).unwrap();
        fs::write(&fixture.sdk, format!("#!/bin/sh\nset -eu\n{sdk_script}\n")).unwrap();
        fs::set_permissions(&fixture.sdk, fs::Permissions::from_mode(0o700)).unwrap();
        fixture
    }

    fn command(&self, language: &str) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_muigen"));
        command
            .arg("split")
            .arg("--input")
            .arg(&self.input)
            .arg("--config")
            .arg(&self.config)
            .args(["--lang", language])
            .arg("--out")
            .arg(&self.out);
        command
    }

    fn run(&self, language: &str) -> Output {
        self.command(language)
            .arg("--muirct")
            .arg(&self.sdk)
            .output()
            .unwrap()
    }

    fn assert_clean(&self) {
        assert_eq!(fs::read(&self.input).unwrap(), b"source");
        assert!(!fs::read_dir(self.root.path()).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".muigen-")
        }));
    }
}

const CONFIG: &str = "<localization><resources><win32Resources/></resources></localization>";
const WORKING_SDK: &str = r#"
test "$#" -eq 7
test "$1" = -q
test -f "$2"
test "$3" = -x
test "$(cat "$5")" = source
printf '%s' "$4" > "$6"
printf satellite > "$7"
"#;

fn diagnostic(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn assert_pair(out: &Path, language: &str, langid: &[u8]) {
    assert_eq!(fs::read(out.join("My App.DLL")).unwrap(), langid);
    assert_eq!(
        fs::read(out.join(language).join("My App.DLL.mui")).unwrap(),
        b"satellite"
    );
    assert_eq!(fs::read_dir(out).unwrap().count(), 2);
}

#[test]
fn supports_spaced_paths_and_canonical_language_tag() {
    let fixture = Fixture::new(CONFIG, WORKING_SDK);
    let output = fixture.run("ZH-cn");
    assert!(output.status.success(), "{}", diagnostic(&output));
    assert_pair(&fixture.out, "zh-CN", b"0x0804");
    fixture.assert_clean();
}

#[test]
fn uses_path_and_accepts_matching_utf16_configuration() {
    let fixture = Fixture::new(CONFIG, WORKING_SDK);
    let xml = CONFIG.replace("win32Resources/", "win32Resources language='EN-us'/");
    let bytes: Vec<_> = [0xff, 0xfe]
        .into_iter()
        .chain(xml.encode_utf16().flat_map(u16::to_le_bytes))
        .collect();
    fs::write(&fixture.config, bytes).unwrap();
    let path = std::env::join_paths([
        fixture.sdk.parent().unwrap(),
        Path::new("/usr/bin"),
        Path::new("/bin"),
    ])
    .unwrap();
    let output = fixture.command("en-US").env("PATH", path).output().unwrap();
    assert!(output.status.success(), "{}", diagnostic(&output));
    assert_pair(&fixture.out, "en-US", b"0x0409");
    fixture.assert_clean();
}

#[test]
fn diagnoses_invalid_language_configuration_and_sdk_path() {
    let fixture = Fixture::new(CONFIG, WORKING_SDK);
    assert!(diagnostic(&fixture.run("fr-FR")).contains("invalid value"));
    let output = fixture
        .command("zh-CN")
        .env("PATH", fixture.root.path())
        .output()
        .unwrap();
    assert!(diagnostic(&output).contains("pass --muirct PATH"));
    fs::set_permissions(&fixture.sdk, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(diagnostic(&fixture.run("zh-CN")).contains("start muirct.exe"));
    fs::write(
        &fixture.config,
        CONFIG.replace("win32Resources/", "win32Resources language='en-US'/"),
    )
    .unwrap();
    assert!(diagnostic(&fixture.run("zh-CN")).contains("conflicts with --lang zh-CN"));
    fs::write(&fixture.config, "<localization>").unwrap();
    assert!(diagnostic(&fixture.run("zh-CN")).contains("invalid .rcconfig"));
    assert!(!fixture.out.exists());
    fixture.assert_clean();
}

#[test]
fn forwards_sdk_failure_and_cleans_partial_files() {
    let fixture = Fixture::new(
        CONFIG,
        r#"
printf modified > "$5"
printf partial > "$6"
printf 'sdk stdout\n'
printf 'sdk stderr\n' >&2
exit 42
"#,
    );
    let output = fixture.run("zh-CN");
    assert_eq!(output.status.code(), Some(42));
    assert!(diagnostic(&output).contains("sdk stderr"));
    assert_eq!(output.stdout, b"sdk stdout\n");
    assert!(!fixture.out.exists());
    fixture.assert_clean();
}

#[test]
fn rejects_success_with_missing_or_empty_artifacts() {
    for script in [r#"printf main > "$6""#, r#"printf main > "$6"; : > "$7""#] {
        let fixture = Fixture::new(CONFIG, script);
        let output = fixture.run("zh-CN");
        assert!(!output.status.success());
        assert!(diagnostic(&output).contains("missing or empty output file"));
        assert!(!fixture.out.exists());
        fixture.assert_clean();
    }
}

#[test]
fn preserves_a_preexisting_directory() {
    let fixture = Fixture::new(CONFIG, WORKING_SDK);
    fs::create_dir(&fixture.out).unwrap();
    let marker = fixture.out.join("keep.txt");
    fs::write(&marker, b"keep").unwrap();
    assert!(diagnostic(&fixture.run("zh-CN")).contains("output already exists"));
    assert_eq!(fs::read(marker).unwrap(), b"keep");
    fixture.assert_clean();
}

#[test]
fn concurrent_splits_publish_only_one_pair() {
    let script = format!(
        r#"
{WORKING_SDK}
participant_dir=$(dirname "$2")
touch "$participant_dir/$4.ready"
for attempt in 1 2 3 4 5 6 7 8 9 10; do
  if test -f "$participant_dir/0x0804.ready" && test -f "$participant_dir/0x0409.ready"; then
    exit 0
  fi
  sleep 0.1
done
exit 99
"#
    );
    let fixture = Fixture::new(CONFIG, &script);
    std::thread::scope(|scope| {
        let chinese = scope.spawn(|| fixture.run("zh-CN"));
        let english = scope.spawn(|| fixture.run("en-US"));
        let chinese = chinese.join().unwrap();
        let english = english.join().unwrap();
        assert_ne!(chinese.status.success(), english.status.success());
        if chinese.status.success() {
            assert!(diagnostic(&english).contains("output already exists"));
            assert_pair(&fixture.out, "zh-CN", b"0x0804");
        } else {
            assert!(diagnostic(&chinese).contains("output already exists"));
            assert_pair(&fixture.out, "en-US", b"0x0409");
        }
    });
    fixture.assert_clean();
}

#[test]
fn batch_retains_first_main_and_synchronizes_alternate_satellite() {
    for checksum_status in [0, 42] {
        let script = format!(
            r#"
if test "$1" = -c; then
    test "$#" -eq 4
    test "$(cat "$2")" = 0x0804
    test "$3" = -e
    test "$(cat "$4")" = satellite
    printf synchronized > "$4"
    exit {checksum_status}
fi
{WORKING_SDK}
"#
        );
        let fallback = CONFIG.replace(
            "win32Resources/",
            "win32Resources ultimateFallbackLanguage='zh-CN' ultimateFallbackLocation='external'/",
        );
        let fixture = Fixture::new(&fallback, &script);
        let alternate_dir = fixture.root.path().join("english edition");
        fs::create_dir(&alternate_dir).unwrap();
        let alternate = alternate_dir.join(fixture.input.file_name().unwrap());
        fs::copy(&fixture.input, &alternate).unwrap();
        let alternate_config = alternate_dir.join("english.rcconfig");
        fs::write(&alternate_config, CONFIG).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_muigen"))
            .arg("batch")
            .arg("--input")
            .arg(&fixture.input)
            .arg("--config")
            .arg(&fixture.config)
            .args(["--lang", "zh-CN"])
            .arg("--input")
            .arg(&alternate)
            .arg("--config")
            .arg(&alternate_config)
            .args(["--lang", "en-US"])
            .arg("--out")
            .arg(&fixture.out)
            .arg("--muirct")
            .arg(&fixture.sdk)
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(checksum_status),
            "{}",
            diagnostic(&output)
        );
        if checksum_status == 0 {
            assert_eq!(fs::read(fixture.out.join("My App.DLL")).unwrap(), b"0x0804");
            assert_eq!(
                fs::read(fixture.out.join("en-US/My App.DLL.mui")).unwrap(),
                b"synchronized"
            );
            assert_eq!(
                fs::read(fixture.out.join("zh-CN/My App.DLL.mui")).unwrap(),
                b"satellite"
            );
            assert_eq!(fs::read_dir(&fixture.out).unwrap().count(), 3);
        } else {
            assert!(!fixture.out.exists());
        }
        fixture.assert_clean();
    }
}

#[test]
fn build_publishes_neutral_resource_and_satellite_only_when_link_succeeds() {
    for link_status in [0, 42] {
        let fixture = Fixture::new(CONFIG, WORKING_SDK);
        let script = fixture.root.path().join("my resources.rc");
        fs::write(&script, b"resource script").unwrap();
        let rc = fixture.sdk.with_file_name("rc.exe");
        let link = fixture.sdk.with_file_name("link.exe");
        fs::write(
            &rc,
            r#"#!/bin/sh
set -eu
test "$#" -eq 10
test "$1 $2 $3 $4" = '/nologo /l 0x0804 /q'
test -f "$5"
test "$6" = /fo
test "$8" = /fm
test "$(cat "${10}")" = 'resource script'
printf neutral > "$7"
printf localized > "$9"
"#,
        )
        .unwrap();
        fs::write(
            &link,
            format!(
                r#"#!/bin/sh
set -eu
test "$#" -eq 7
test "$1 $2 $3 $4 $5" = '/NOLOGO /DLL /NOENTRY /MANIFEST:NO /MACHINE:X64'
test "$(cat "$7")" = localized
printf satellite > "${{6#/OUT:}}"
exit {link_status}
"#,
            ),
        )
        .unwrap();
        for tool in [&rc, &link] {
            fs::set_permissions(tool, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let output = Command::new(env!("CARGO_BIN_EXE_muigen"))
            .args(["build", "--input"])
            .arg(&script)
            .arg("--config")
            .arg(&fixture.config)
            .args([
                "--lang",
                "zh-CN",
                "--module",
                "My App.dll",
                "--machine",
                "x64",
                "--out",
            ])
            .arg(&fixture.out)
            .arg("--rc")
            .arg(&rc)
            .arg("--link")
            .arg(&link)
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(link_status),
            "{}",
            diagnostic(&output)
        );
        if link_status == 0 {
            assert_eq!(
                fs::read(fixture.out.join("My App.dll.res")).unwrap(),
                b"neutral"
            );
            assert_eq!(
                fs::read(fixture.out.join("zh-CN/My App.dll.mui")).unwrap(),
                b"satellite"
            );
            assert_eq!(fs::read_dir(&fixture.out).unwrap().count(), 2);
        } else {
            assert!(!fixture.out.exists());
        }
        assert_eq!(fs::read(script).unwrap(), b"resource script");
        fixture.assert_clean();
    }
}
