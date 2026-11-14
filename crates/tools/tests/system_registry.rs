//! Read the system builder's output with the kernel's actual hive parser.
extern crate alloc;

#[allow(dead_code)]
#[path = "../../registry/src/registry/hive.rs"]
mod hive;

use nt61_tools::fs::system::SystemBuilder;

#[test]
fn system_partition_contains_mountable_registry_hives() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    SystemBuilder::new(root)
        .unwrap()
        .write_registry_hives()
        .unwrap();
    for name in ["SYSTEM", "SOFTWARE", "SAM", "SECURITY", "DEFAULT"] {
        let bytes = std::fs::read(root.join("Windows/System32/config").join(name)).unwrap();
        let hive = hive::Hive::parse(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(hive.cell_count() > 0, "{name}");
        assert!(hive.root().is_ok(), "{name}");
        if name == "SOFTWARE" {
            let node = hive
                .open_path(&["microsoft", "windows nt", "currentversion"])
                .unwrap()
                .expect("Windows version key");
            let value = hive.find_value(&node, "productname").unwrap().unwrap();
            assert_eq!(
                value.as_utf16_string().as_deref(),
                Some("Windows 7 Ultimate")
            );
        }
        if name == "SYSTEM" {
            let node = hive.open_path(&["select"]).unwrap().unwrap();
            assert_eq!(
                hive.find_value(&node, "current").unwrap().unwrap().as_u32(),
                Some(1)
            );
        }
    }
    assert!(root.join("Windows/System32/drivers/etc/hosts").is_file());
}
