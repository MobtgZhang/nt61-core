use std::{
    path::Path,
    process::{Command, Output},
};

const INF: &str = r#"[Version]
Signature="$Windows NT$"
Class=Test
ClassGuid={4d36e967-e325-11ce-bfc1-08002be10318}
Provider=Test
DriverVer=11/19/2010,6.1.7601.17514
[Manufacturer]
v=Models,NTamd64
[Models.NTamd64]
x=Install,ROOT\TEST
[Install.NT]
AddReg=Values
[Values]
HKR,,value,,seed
"#;
const BINDINGS: &str = r#"{"bindings":[{"root":"Relative","scope":"DeviceSoftware","view":"Native","source_prefix":"","destination":"Device\\Software"}],"directories":[]}"#;

fn run(root: &Path, extra: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_driver-package-plan"))
        .current_dir(root)
        .args(["--inf", "test.inf", "--hardware-id", "ROOT\\TEST"])
        .args(extra)
        .output()
        .unwrap()
}
fn fixture() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("test.inf"), INF).unwrap();
    std::fs::write(directory.path().join("bindings.json"), BINDINGS).unwrap();
    directory
}
fn export(root: &Path) -> Output {
    run(
        root,
        &[
            "--resolve-dependencies",
            "--resolve-registry",
            "--registry-bindings",
            "bindings.json",
            "--export-registry-hive",
            "fragment.hiv",
        ],
    )
}

#[test]
fn optional_export_is_explicit_and_never_claims_installation_or_authentication() {
    let directory = fixture();
    let root = directory.path();
    let output = run(root, &[]);
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(report.get("registry_hive_export").is_none());
    assert_eq!(std::fs::read_dir(root).unwrap().count(), 2);
    let output = export(root);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schema"], 4);
    for key in ["package_authenticated", "installation_executed"] {
        assert_eq!(report[key], false);
    }
    let export = &report["registry_hive_export"];
    assert_eq!(export["complete_installation"], false);
    assert_eq!(export["keys"], 3);
    assert_eq!(export["values"], 1);
    let bytes = std::fs::read(root.join("fragment.hiv")).unwrap();
    assert_eq!(&bytes[..4], b"regf");
    // The second call must preserve the first export even though it is valid.
    let rejected = self::export(root);
    assert!(!rejected.status.success());
    assert!(rejected.stdout.is_empty());
    assert_eq!(std::fs::read(root.join("fragment.hiv")).unwrap(), bytes);
}

#[test]
fn malformed_config_missing_mapping_and_late_unsupported_write_publish_nothing() {
    let directory = fixture();
    let root = directory.path();
    for (configuration, source, reason) in [
        ("{}".to_owned(), INF.to_owned(), "parse bindings"),
        (
            BINDINGS.replace("\"directories\":[]", "\"directories\":[],\"ignored\":true"),
            INF.to_owned(),
            "parse bindings",
        ),
        (
            "{\"bindings\":[],\"directories\":[]}".into(),
            INF.into(),
            "MissingBinding",
        ),
        (
            BINDINGS.into(),
            format!("{INF}HKR,,omitted\n"),
            "UnsupportedOmittedData",
        ),
        (" ".repeat(1024 * 1024 + 1), INF.into(), "bindings exceed"),
    ] {
        std::fs::write(root.join("test.inf"), source).unwrap();
        std::fs::write(root.join("bindings.json"), configuration).unwrap();
        let output = export(root);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(reason),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!root.join("fragment.hiv").exists());
        assert_eq!(std::fs::read_dir(root).unwrap().count(), 2);
    }
}

#[test]
fn export_rejects_absent_model_selection_and_incomplete_flags() {
    let directory = fixture();
    let root = directory.path();
    let output = run(root, &["--export-registry-hive", "fragment.hiv"]);
    assert!(!output.status.success());
    assert!(!root.join("fragment.hiv").exists());
    let output = Command::new(env!("CARGO_BIN_EXE_driver-package-plan"))
        .current_dir(root)
        .args([
            "--inf",
            "test.inf",
            "--resolve-dependencies",
            "--resolve-registry",
            "--registry-bindings",
            "bindings.json",
            "--export-registry-hive",
            "fragment.hiv",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("exactly one selected model"));
    assert!(!root.join("fragment.hiv").exists());
}
