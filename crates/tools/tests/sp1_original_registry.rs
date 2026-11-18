//! Regression for REGF data-cell offsets observed in the original SP1 ISO.
extern crate alloc;
#[allow(dead_code)]
#[path = "../../boot/src/bcd_registry.rs"]
mod bcd_registry;
#[allow(dead_code)]
#[path = "../../registry/src/registry/hive.rs"]
mod hive;
#[allow(dead_code)]
#[path = "../../registry/src/registry/hive_writer.rs"]
mod hive_writer;

#[test]
#[ignore = "requires pinned SYSTEM hive extracted by audit-sp1-registry.py"]
fn original_sp1_system_data_and_inline_values() {
    let path = std::env::var_os("NT61_SP1_SYSTEM_HIVE").expect("NT61_SP1_SYSTEM_HIVE");
    let bytes = std::fs::read(path).unwrap();
    let hive = hive::Hive::parse(&bytes).unwrap();
    let boot_hive = bcd_registry::Hive::parse(&bytes).unwrap();
    let root = hive.root().unwrap();
    let index = 4096
        + u32::from_le_bytes(
            bytes[root.offset as usize + 0x20..root.offset as usize + 0x24]
                .try_into()
                .unwrap(),
        ) as usize;
    assert_eq!(&bytes[index + 4..index + 6], b"lh");
    // The NT lh hash is over every uppercase character, with wrapping *37.
    let count = u16::from_le_bytes(bytes[index + 6..index + 8].try_into().unwrap()) as usize;
    for i in 0..count {
        let offset = 4096
            + u32::from_le_bytes(
                bytes[index + 8 + i * 8..index + 12 + i * 8]
                    .try_into()
                    .unwrap(),
            ) as usize;
        let length =
            u16::from_le_bytes(bytes[offset + 0x4c..offset + 0x4e].try_into().unwrap()) as usize;
        let name = std::str::from_utf8(&bytes[offset + 0x50..offset + 0x50 + length]).unwrap();
        let hash = name.bytes().fold(0u32, |hash, c| {
            hash.wrapping_mul(37)
                .wrapping_add(c.to_ascii_uppercase() as u32)
        });
        assert_eq!(
            hash,
            u32::from_le_bytes(
                bytes[index + 12 + i * 8..index + 16 + i * 8]
                    .try_into()
                    .unwrap()
            )
        );
    }
    let select = hive.open_path(&["Select"]).unwrap().unwrap();
    assert_eq!(
        hive.find_value(&select, "Current")
            .unwrap()
            .unwrap()
            .as_u32(),
        Some(1)
    );
    for (path, name, kind, expected) in [
        (
            vec!["ControlSet001", "Control", "Session Manager"],
            "BootExecute",
            hive::ValueType::MultiString,
            "autocheck autochk *\0\0",
        ),
        (
            vec!["ControlSet001", "Services", "disk"],
            "ImagePath",
            hive::ValueType::ExpandString,
            "system32\\drivers\\disk.sys\0",
        ),
    ] {
        let node = hive.open_path(&path).unwrap().unwrap();
        let value = hive.find_value(&node, name).unwrap().unwrap();
        assert_eq!(value.value_type, kind);
        assert_eq!(
            value.data,
            expected
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>()
        );
        let boot_node = boot_hive.open(&path.join("\\")).unwrap();
        let boot_value = boot_hive.get_value(&boot_node, name).unwrap().unwrap();
        assert_eq!(boot_value.data, value.data);
    }
    // Independently located in the pinned ISO's SYSTEM. Segment payload
    // boundaries and final lengths are read directly, without the db reader.
    for (path, name, length, segments) in [
        (
            vec!["ControlSet002", "Control", "ProductOptions"],
            "ProductPolicy",
            17348,
            vec![0x5fc020, 0x5f8020],
        ),
        (
            vec!["ControlSet002", "services", "rdyboost", "Parameters"],
            "BootPlan",
            168068,
            vec![
                0x878020, 0x870020, 0x86c020, 0x868020, 0x864020, 0x860020, 0x85c020, 0x858020,
                0x854020, 0x850020, 0x84c020,
            ],
        ),
    ] {
        let node = hive.open_path(&path).unwrap().unwrap();
        let value = hive.find_value(&node, name).unwrap().unwrap();
        assert_eq!(value.value_type, hive::ValueType::Binary);
        let mut expected = Vec::new();
        for offset in segments {
            let size = (length - expected.len()).min(0x3fd8);
            expected.extend_from_slice(&bytes[offset + 4..offset + 4 + size]);
        }
        assert_eq!(value.data.len(), length);
        assert_eq!(value.data, expected);
    }
    // Every reachable original key/value must be enumerated. This is a
    // structural read check, not Windows ACL or registry API conformance.
    let mut pending = vec![root];
    let mut visited = std::collections::BTreeSet::new();
    let (mut keys, mut values) = (0, 0);
    while let Some(node) = pending.pop() {
        assert!(visited.insert(node.offset));
        let read_count = |field| {
            u32::from_le_bytes(
                bytes[node.offset as usize + field..node.offset as usize + field + 4]
                    .try_into()
                    .unwrap(),
            ) as usize
        };
        let children = hive.subkeys(&node).unwrap();
        assert_eq!(children.len(), read_count(0x18));
        let entries = hive.values(&node).unwrap();
        assert_eq!(entries.len(), read_count(0x28));
        keys += 1;
        values += entries.len();
        pending.extend(children);
    }
    assert_eq!((keys, values), (18471, 53921));
}

#[test]
fn tool_and_kernel_writers_store_offsets_to_the_allocated_cell_header() {
    let expected: Vec<u8> = "retained string\0"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    let mut root = hive_writer::HiveKey::new("Root");
    root.values.push(hive_writer::HiveValue {
        name: "Text".into(),
        value_type: hive::ValueType::String,
        data: expected.clone(),
    });
    let bytes = hive_writer::HiveWriter::new().serialise(&root, 0).unwrap();
    assert_cell_offset_and_data(&bytes, &expected);
    let root = nt61_tools::regf::Node::new("Root")
        .root()
        .value(nt61_tools::regf::Value::sz("Text", "retained string"));
    let bytes = nt61_tools::regf::HiveBuilder::new(root).finish();
    assert_cell_offset_and_data(&bytes, &expected);
}

fn assert_cell_offset_and_data(bytes: &[u8], expected: &[u8]) {
    let parsed = hive::Hive::parse(bytes).unwrap();
    assert_eq!(
        parsed
            .find_value(&parsed.root().unwrap(), "Text")
            .unwrap()
            .unwrap()
            .data,
        expected
    );
    let boot = bcd_registry::Hive::parse(bytes).unwrap();
    assert_eq!(
        boot.get_value(&boot.root().unwrap(), "Text")
            .unwrap()
            .unwrap()
            .data,
        expected
    );
    // Independent offset arithmetic, so a reader/writer sharing the same error
    // cannot make the regression pass through a roundtrip alone.
    let u32_at =
        |offset| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
    let root = 4096 + u32_at(0x24);
    assert_eq!(u32_at(0x28), bytes.len() - 4096);
    assert_eq!(u32_at(4100), 0);
    let list = 4096 + u32_at(root + 0x2c);
    let value = 4096 + u32_at(list + 4);
    let cell = 4096 + u32_at(value + 12);
    let length = i32::from_le_bytes(bytes[cell..cell + 4].try_into().unwrap());
    assert!(length < 0);
    assert!(length.unsigned_abs() as usize >= expected.len() + 4);
    assert_eq!(&bytes[cell + 4..cell + 4 + expected.len()], expected);
}

#[test]
fn long_ascii_and_unicode_key_and_value_names_have_correct_encoding_flags() {
    for name in [
        "a".repeat(63),
        "a".repeat(64),
        "a".repeat(255),
        "注册表".into(),
        "A😀值".into(),
    ] {
        let root = nt61_tools::regf::Node::new("Root").root().subkey(
            nt61_tools::regf::Node::new(&name).value(nt61_tools::regf::Value::sz(&name, "data")),
        );
        let bytes = nt61_tools::regf::HiveBuilder::new(root).finish();
        let parsed = hive::Hive::parse(&bytes).unwrap();
        let child = parsed
            .find_subkey(&parsed.root().unwrap(), &name)
            .unwrap()
            .unwrap();
        assert_eq!(child.name, name);
        let value = parsed.find_value(&child, &name).unwrap().unwrap();
        assert_eq!(value.name, name);
        assert_eq!(value.as_utf16_string().as_deref(), Some("data"));
        let boot = bcd_registry::Hive::parse(&bytes).unwrap();
        let child = boot.open(&name).unwrap();
        assert_eq!(child.name, name);
        let value = boot.get_value(&child, &name).unwrap().unwrap();
        assert_eq!(value.name, name);
        assert_eq!(value.as_string().as_deref(), Some("data"));
    }
}

#[test]
fn boot_manager_reads_complete_paths_and_boot_options_from_the_production_bcd() {
    let bytes = nt61_tools::hive_gen::build_bcd();
    let boot = bcd_registry::Hive::parse(&bytes).unwrap();
    let kernel = hive::Hive::parse(&bytes).unwrap();
    let mut pending = vec![(String::new(), kernel.root().unwrap())];
    let mut count = 0;
    while let Some((path, key)) = pending.pop() {
        let boot_key = if path.is_empty() {
            boot.root().unwrap()
        } else {
            boot.open(&path).unwrap()
        };
        for value in kernel.values(&key).unwrap() {
            let actual = boot.get_value(&boot_key, &value.name).unwrap().unwrap();
            assert_eq!(actual.value_type, value.value_type.to_u32(), "{path}");
            assert_eq!(actual.data, value.data, "{path}");
            count += 1;
        }
        for child in kernel.subkeys(&key).unwrap() {
            let path = if path.is_empty() {
                child.name.clone()
            } else {
                format!("{path}\\{}", child.name)
            };
            pending.push((path, child));
        }
    }
    assert!(count > 10);
    let mut loader_paths = 0;
    for object in boot.subkeys(&boot.open("Objects").unwrap()).unwrap() {
        if let Ok(key) = boot.open(&format!("Objects\\{}\\Elements\\12000002", object.name)) {
            let value = boot.get_value(&key, "Element").unwrap().unwrap();
            if value.as_string().as_deref() == Some("\\Windows\\System32\\winload.efi") {
                loader_paths += 1;
            }
        }
    }
    assert_eq!(loader_paths, 4);
}

#[test]
fn writer_lh_hashes_match_the_pinned_sp1_system_root_entries() {
    let mut root = hive_writer::HiveKey::new("Root");
    root.subkeys = vec![
        hive_writer::HiveKey::new("ControlSet001"),
        hive_writer::HiveKey::new("Select"),
    ];
    let kernel = hive_writer::HiveWriter::new().serialise(&root, 0).unwrap();
    let root = nt61_tools::regf::Node::new("Root")
        .root()
        .subkey(nt61_tools::regf::Node::new("ControlSet001"))
        .subkey(nt61_tools::regf::Node::new("Select"));
    let tool = nt61_tools::regf::HiveBuilder::new(root).finish();
    for bytes in [kernel, tool] {
        let u32_at =
            |offset| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        let root = 4096 + u32_at(0x24);
        let index = 4096 + u32_at(root + 0x20);
        assert_eq!(&bytes[index + 4..index + 6], b"lh");
        assert_eq!(u32_at(index + 12), 0x8f3ba9a2);
        assert_eq!(u32_at(index + 20), 0x5f0024a0);
    }
}
