use nt61_tools::fs::original_gate;
use std::fs;

#[test]
fn existing_desktop_media_are_accepted() {
    let temp = tempfile::tempdir().unwrap();
    let esp = temp.path().join("esp");
    let system = temp.path().join("system");
    fs::create_dir_all(&esp).unwrap();
    nt61_tools::fs::system::SystemBuilder::new(&system)
        .unwrap()
        .prepare_system_media()
        .unwrap();
    let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../desktop/assets/windows7");
    // Exercise the production installer and compare every retained media file,
    // including the optional sound schemes and regional wallpaper sets.
    for (source, destination, count) in [
        ("sounds", "Windows/Media", 306),
        ("cursors", "Windows/Cursors", 162),
        ("wallpapers/Desktop", "Windows/Web/Wallpaper", 32),
        ("User Account Pictures", "ProgramData/Microsoft/User Account Pictures", 37),
    ] {
        assert_eq!(assert_media_tree(&assets.join(source), &system.join(destination)), count);
    }
    let mut regional_count = 0;
    for country in fs::read_dir(assets.join("wallpapers/Desktop Regional")).unwrap() {
        let country = country.unwrap().path();
        let file = fs::read_dir(&country).unwrap().next().unwrap().unwrap();
        let name = file.file_name();
        let code = name.to_str().unwrap().split('-').next().unwrap();
        regional_count += assert_media_tree(
            &country,
            &system.join(format!("Windows/Globalization/MCT/MCT-{code}/Wallpaper")),
        );
    }
    assert_eq!(regional_count, 108);
    original_gate::validate_trees(&esp, &system).unwrap();
}

fn assert_media_tree(source: &std::path::Path, installed: &std::path::Path) -> usize {
    let mut files = 0;
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = installed.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            files += assert_media_tree(&entry.path(), &target);
        } else {
            assert_eq!(fs::read(entry.path()).unwrap(), fs::read(&target).unwrap(), "{}", target.display());
            files += 1;
        }
    }
    files
}

#[test]
fn rust_artifacts_pass_without_reading_an_external_reference() {
    let temp = tempfile::tempdir().unwrap();
    let esp = temp.path().join("esp");
    let system = temp.path().join("system");
    fs::create_dir_all(&esp).unwrap();
    fs::create_dir_all(&system).unwrap();
    fs::write(system.join("user32.dll"), b"local Rust output").unwrap();
    original_gate::validate_trees(&esp, &system).unwrap();
}

#[cfg(unix)]
#[test]
fn staging_alias_cannot_bypass_inspection() {
    let temp = tempfile::tempdir().unwrap();
    let esp = temp.path().join("esp");
    let system = temp.path().join("system");
    fs::create_dir_all(&esp).unwrap();
    fs::create_dir_all(&system).unwrap();
    std::os::unix::fs::symlink(&esp, system.join("alias")).unwrap();
    assert!(original_gate::validate_trees(&esp, &system)
        .unwrap_err()
        .to_string()
        .contains("symlink"));
}

#[test]
#[ignore = "requires external read-only SP1 reference; temporary staging stays outside repository"]
fn original_renamed_payload_is_rejected_by_content() {
    let reference =
        std::env::var_os("NT61_SP1_REFERENCE_DIR").expect("external reference directory");
    let source = std::path::Path::new(&reference).join("foundation/4/Windows/System32/user32.dll");
    let temp = tempfile::tempdir().unwrap();
    let esp = temp.path().join("esp");
    let system = temp.path().join("system");
    fs::create_dir_all(&esp).unwrap();
    fs::create_dir_all(&system).unwrap();
    let payload = fs::read(source).unwrap();
    fs::write(esp.join("renamed.bin"), &payload).unwrap();
    let err = original_gate::validate_trees(&esp, &system).unwrap_err();
    assert!(err.to_string().contains("original SP1 payload"), "{err}");
    assert_eq!(fs::read(esp.join("renamed.bin")).unwrap(), payload);
}
