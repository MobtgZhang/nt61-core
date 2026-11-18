//! Exercise persistence through the actual REGF writer and kernel reader.
use nt61_driver_package::{
    registry_apply::{RegistryBinding, RegistryImage},
    Inf, InfSet, RegistryAction, RegistryRoot, RegistryScope, RegistryValue, RegistryView,
    RegistryWriteMode, Target,
};
use nt61_tools::registry_hive::{encode_registry_hive, verify_registry_hive, write_registry_hive};

fn fixture() -> (nt61_driver_package::RegistryPlan, Vec<RegistryBinding>) {
    let source = br#"[Version]
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
HKR,Empty,,0x10
"#;
    let inf = Inf::parse(source).unwrap();
    let documents = [("test.inf", &inf)];
    let set = InfSet::new(&documents).unwrap();
    let plan = set
        .resolve_registry(
            "test.inf",
            &inf.models(Target::SP1_AMD64).unwrap()[0],
            Target::SP1_AMD64,
            &["ROOT\\TEST"],
        )
        .unwrap();
    let bindings = vec![RegistryBinding {
        root: RegistryRoot::Relative,
        scope: Some(RegistryScope::DeviceSoftware),
        view: RegistryView::Native,
        source_prefix: String::new(),
        destination: "Device\\Software".into(),
    }];
    (plan, bindings)
}

fn image_with_value(kind: u32, name: &str, data: Vec<u8>) -> RegistryImage {
    let (mut plan, bindings) = fixture();
    plan.operations[0].action = RegistryAction::SetValue {
        name: name.into(),
        value: RegistryValue {
            kind,
            data,
            requires_directory_resolution: false,
        },
        mode: RegistryWriteMode::Replace,
    };
    let mut image = RegistryImage::default();
    image.apply(&plan, &bindings, &[]).unwrap();
    image
}

#[test]
fn persisted_hive_preserves_every_type_byte_default_value_and_empty_key() {
    for kind in 0..=11 {
        let data = match kind {
            1 | 2 => "中文\0".encode_utf16().flat_map(u16::to_le_bytes).collect(),
            7 => "One\0Two\0\0"
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect(),
            4 => 0xfedcba98u32.to_le_bytes().to_vec(),
            _ => (0..=255).collect(),
        };
        let image = image_with_value(kind, "", data);
        assert!(image.contains_key("Device\\Software\\Empty").unwrap());
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("fragment.hiv");
        let length = write_registry_hive(&image, &path).unwrap();
        let reopened = std::fs::read(&path).unwrap();
        assert_eq!(length, reopened.len());
        assert_eq!(length % 4096, 0);
        assert_eq!(&reopened[..4], b"regf");
        verify_registry_hive(&image, &reopened).unwrap();
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }
}

#[test]
fn verification_detects_value_corruption_missing_and_extra_keys() {
    let image = image_with_value(3, "value", b"unique payload bytes".to_vec());
    let mut bytes = encode_registry_hive(&image).unwrap();
    let index = bytes
        .windows(20)
        .position(|b| b == b"unique payload bytes")
        .unwrap();
    bytes[index] ^= 1;
    assert!(verify_registry_hive(&image, &bytes).is_err());
    assert!(verify_registry_hive(&RegistryImage::default(), &bytes).is_err());
    let empty = encode_registry_hive(&RegistryImage::default()).unwrap();
    assert!(verify_registry_hive(&image, &empty).is_err());
    assert!(verify_registry_hive(&image, &bytes[..4096]).is_err());
}

#[test]
fn existing_file_or_symlink_is_never_replaced_and_temporary_is_removed() {
    let image = image_with_value(3, "value", vec![1, 2, 3]);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("existing.hiv");
    std::fs::write(&path, b"preserve original bytes").unwrap();
    assert_eq!(
        write_registry_hive(&image, &path).unwrap_err().kind(),
        std::io::ErrorKind::AlreadyExists
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"preserve original bytes");
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    #[cfg(unix)]
    {
        let link = directory.path().join("link.hiv");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(write_registry_hive(&image, &link).is_err());
        assert!(link.symlink_metadata().unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read(&path).unwrap(), b"preserve original bytes");
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 2);
    }
}

#[test]
fn writer_limits_fail_before_creating_an_artifact() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("never.hiv");
    for image in [
        image_with_value(3, "large", vec![1; 4061]),
        image_with_value(99, "unknown_type", vec![]),
        image_with_value(3, &"a".repeat(2001), vec![]),
    ] {
        assert!(write_registry_hive(&image, &path).is_err());
        assert!(!path.exists());
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
    }
    // Largest supported data cell must still survive an HBIN boundary.
    let image = image_with_value(3, &"a".repeat(2000), vec![7; 4060]);
    write_registry_hive(&image, &path).unwrap();
    verify_registry_hive(&image, &std::fs::read(path).unwrap()).unwrap();
}

#[test]
fn unsupported_recursive_indexes_are_rejected_before_reader_recursion() {
    let image = image_with_value(3, "value", vec![]);
    let mut bytes = encode_registry_hive(&image).unwrap();
    let root = 4096 + u32::from_le_bytes(bytes[0x24..0x28].try_into().unwrap()) as usize;
    let index =
        4096 + u32::from_le_bytes(bytes[root + 0x20..root + 0x24].try_into().unwrap()) as usize;
    bytes[index + 4..index + 6].copy_from_slice(b"ri");
    bytes[index + 8..index + 12].copy_from_slice(&((index - 4096) as u32).to_le_bytes());
    assert!(verify_registry_hive(&image, &bytes).is_err());
}
