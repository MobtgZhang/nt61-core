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
    let mut registered = std::collections::BTreeSet::new();
    for invocation in include_str!("../src/ke/ssdt_common.rs")
        .split("register_syscall(")
        .skip(1)
    {
        let fields: Vec<_> = invocation
            .split_once(')')
            .unwrap()
            .0
            .split(',')
            .map(str::trim)
            .collect();
        if !fields[0].starts_with("0x") {
            continue;
        }
        let number = u16::from_str_radix(&fields[0][2..], 16).unwrap();
        let metadata = &NATIVE_SERVICE_METADATA[usize::from(number)];
        assert_eq!(fields[3].trim_matches('"'), metadata.name);
        assert_eq!(fields[2].parse::<u8>().unwrap(), metadata.arguments);
        registered.insert(metadata.name.to_owned());
    }
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
        registered.insert(name);
        checked += 1;
    }
    assert!(checked >= 147);
    assert!(
        registered.len() >= 201,
        "at least half of the 401 native services must remain registered"
    );
}

#[test]
fn shadow_registrations_match_win7_identities_and_exact_private_stub_arities() {
    use nt61_abi::syscall_metadata::{WIN32K_ARGUMENTS, WIN32K_STACK_ARGUMENTS};
    let mut seen = std::collections::BTreeSet::new();
    let mut exact_checked = 0;
    let source = include_str!("../src/ke/shadow_ssdt.rs");
    for registration in source.split("register!(").skip(1) {
        let arguments = registration.split_once(')').unwrap().0;
        let fields: Vec<_> = arguments
            .split(',')
            .map(str::trim)
            .filter(|field| !field.is_empty())
            .collect();
        assert_eq!(fields.len(), 3);
        let name = fields[0].trim_matches('"');
        let number = nt61_abi::syscalls::win32k_number(name)
            .unwrap_or_else(|| panic!("{name} is not a Win7 SP1 service"));
        assert!(seen.insert(number), "duplicate registration: {name}");
        let arity: u8 = fields[2].parse().unwrap();
        let index = usize::from(number - 0x1000);
        assert_eq!(
            arity.saturating_sub(4),
            WIN32K_STACK_ARGUMENTS[index],
            "{name}"
        );
        if WIN32K_ARGUMENTS[index] == 255 {
            // These six calls have no WOW64 transition stub in the supplied
            // image. Preserve the distinction between header signatures and
            // exact binary evidence; the kernel table proves only <=4 here.
            let expected = match name {
                "NtUserNotifyProcessCreate"
                | "NtGdiMoveTo"
                | "NtUserSetClassLongPtr"
                | "NtUserSetWindowLongPtr" => 4,
                "NtGdiSelectBrush" => 2,
                "NtGdiSetTextJustification" => 3,
                _ => panic!("new registration lacks exact stub evidence: {name}"),
            };
            assert_eq!(arity, expected, "{name}");
        } else {
            assert_eq!(arity, WIN32K_ARGUMENTS[index], "{name}");
            exact_checked += 1;
        }
    }
    assert!(
        exact_checked >= 408,
        "exact binary arity coverage must not regress"
    );
    assert!(
        seen.len() >= 414,
        "at least half of the 827 services must remain registered"
    );
}
