//! Keep active architecture dispatch constants and adapter arities tied to the
//! metadata extracted from the Windows 7 SP1 image.
use nt61_abi::syscall_metadata::NATIVE_SERVICE_METADATA;

fn key(name: &str) -> String {
    name.chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(char::to_uppercase)
        .collect()
}

#[test]
fn both_x64_native_constant_tables_match_the_shipped_services() {
    for source in [
        include_str!("../../arch/src/arch/x86_64/syscall.rs"),
        include_str!("../../arch/src/arch/x86_64/syscall_numbers.rs"),
    ] {
        let mut checked = 0;
        for line in source.lines() {
            let Some(declaration) = line.trim().strip_prefix("pub const NT_") else {
                continue;
            };
            let (suffix, value) = declaration.split_once(": u32 = ").unwrap();
            let name = format!("NT_{suffix}");
            let number =
                u16::from_str_radix(value.trim_end_matches(';').strip_prefix("0x").unwrap(), 16)
                    .unwrap();
            let metadata = NATIVE_SERVICE_METADATA
                .iter()
                .find(|entry| key(entry.name) == key(&name))
                .unwrap_or_else(|| panic!("{name} is not a Windows 7 SP1 service"));
            assert_eq!(number, metadata.number, "{name}");
            checked += 1;
        }
        assert!(
            checked == 295,
            "the active table must be checked, not an empty fixture"
        );
    }
}

#[test]
fn portable_adapters_use_the_shipped_argument_counts() {
    let mut checked = 0;
    for line in include_str!("../src/ke/ssdt_portable.rs").lines() {
        let Some(declaration) = line.trim().strip_prefix("(\"Nt") else {
            continue;
        };
        let name = format!("Nt{}", declaration.split_once('"').unwrap().0);
        let list = declaration
            .split_once('[')
            .unwrap()
            .1
            .split_once(']')
            .unwrap()
            .0;
        let count = list
            .split(',')
            .filter(|value| !value.trim().is_empty())
            .count();
        let metadata = NATIVE_SERVICE_METADATA
            .iter()
            .find(|entry| entry.name == name)
            .unwrap();
        assert_eq!(count, metadata.arguments as usize, "{name}");
        checked += 1;
    }
    assert_eq!(checked, 103);
}
