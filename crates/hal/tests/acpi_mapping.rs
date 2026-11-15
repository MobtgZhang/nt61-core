//! Exercise actual HAL lookup + mapping ownership, with only the privileged
//! physical-map boundary replaced. Unmapped buffers are freed immediately.
#![allow(dead_code)]
mod ke {
    pub mod sync {
        pub struct Spinlock<T>(std::sync::Mutex<T>);
        impl<T> Spinlock<T> {
            pub const fn new(value: T) -> Self {
                Self(std::sync::Mutex::new(value))
            }
            pub fn lock(&self) -> std::sync::MutexGuard<'_, T> {
                self.0.lock().unwrap()
            }
        }
    }
}
mod hal {
    pub mod x86_64 {
        pub mod io_port {
            pub fn read_port_uchar(_: u16) -> u8 {
                panic!("privileged I/O")
            }
            pub fn write_port_uchar(_: u16, _: u8) {
                panic!("privileged I/O")
            }
        }
    }
}
mod mm {
    pub mod syspte {
        use std::collections::BTreeMap;
        use std::sync::Mutex;
        #[derive(Default)]
        pub struct State {
            pub physical: Vec<u8>,
            pub maps: BTreeMap<u64, (u64, Vec<u8>)>,
            pub mapped: usize,
            pub unmapped: usize,
        }
        pub static STATE: Mutex<State> = Mutex::new(State {
            physical: Vec::new(),
            maps: BTreeMap::new(),
            mapped: 0,
            unmapped: 0,
        });
        pub fn map_io_space(physical: u64, pages: u64) -> Option<u64> {
            assert_eq!(physical & 4095, 0);
            let mut state = STATE.lock().unwrap();
            let bytes = state
                .physical
                .get(physical as usize..physical.checked_add(pages * 4096)? as usize)?
                .to_vec();
            let base = bytes.as_ptr() as u64;
            state.maps.insert(base, (pages, bytes));
            state.mapped += 1;
            Some(base)
        }
        pub fn unmap_io_space(base: u64, pages: u64) {
            let mut state = STATE.lock().unwrap();
            let (allocated, bytes) = state
                .maps
                .remove(&base)
                .expect("must unmap original mapping base once");
            assert_eq!(allocated, pages);
            drop(bytes);
            state.unmapped += 1;
        }
    }
}
#[path = "../src/hal/common/acpi.rs"]
mod acpi;
fn fix(bytes: &mut [u8], offset: usize) {
    bytes[offset] = 0;
    bytes[offset] = 0u8.wrapping_sub(bytes.iter().fold(0u8, |s, &b| s.wrapping_add(b)));
}
fn table(signature: &[u8; 4], length: usize) -> Vec<u8> {
    let mut b = vec![0; length];
    b[..4].copy_from_slice(signature);
    b[4..8].copy_from_slice(&(length as u32).to_le_bytes());
    fix(&mut b, 9);
    b
}
#[test]
fn actual_lookup_releases_all_mappings_and_retains_validated_snapshots() {
    let mut ram = vec![0u8; 0x30000];
    let rsdp = 0x1ff8;
    let xsdt = 0x3ff0;
    let fadt = 0xaff9;
    let bad = 0x12001;
    let mut root = table(b"XSDT", 36 + 8 * 600);
    root[36..44].copy_from_slice(&(bad as u64).to_le_bytes());
    root[36 + 599 * 8..].copy_from_slice(&(fadt as u64).to_le_bytes());
    fix(&mut root, 9);
    ram[xsdt..xsdt + root.len()].copy_from_slice(&root);
    let mut rp = vec![0; 36];
    rp[..8].copy_from_slice(b"RSD PTR ");
    rp[15] = 2;
    rp[20..24].copy_from_slice(&36u32.to_le_bytes());
    rp[24..32].copy_from_slice(&(xsdt as u64).to_le_bytes());
    fix(&mut rp[..20], 8);
    fix(&mut rp, 32);
    ram[rsdp..rsdp + 36].copy_from_slice(&rp);
    let mut ft = table(b"FACP", 5000);
    ft[108] = 0x32;
    ft[8] = 1;
    fix(&mut ft, 9);
    ram[fadt..fadt + ft.len()].copy_from_slice(&ft);
    let mut invalid = table(b"BAD!", 5000);
    invalid[100] ^= 1;
    ram[bad..bad + invalid.len()].copy_from_slice(&invalid);
    mm::syspte::STATE.lock().unwrap().physical = ram;
    acpi::set_rsdp(rsdp as u64);
    assert!(acpi::validate_rsdp(rsdp as u64));
    let pointer = acpi::find_table(b"FACP").unwrap();
    assert_eq!(acpi::find_table(b"FACP"), Some(pointer));
    assert!(acpi::find_table(b"BAD!").is_none());
    assert_eq!(acpi::rtc_century_register(), Some(0x32));
    let tables = acpi::enumerate_tables();
    assert_eq!(tables, [(pointer, *b"FACP")]);
    let state = mm::syspte::STATE.lock().unwrap();
    assert!(state.maps.is_empty());
    assert_eq!(state.mapped, state.unmapped);
    assert!(state.mapped > 10);
    // The temporary map was freed; the complete immutable copy remains valid.
    assert_eq!(
        unsafe { std::slice::from_raw_parts(pointer.cast::<u8>(), 5000) },
        ft
    );
    drop(state);

    // A legacy FADT is valid without later revision fields. These must read
    // as absent, rather than reading beyond its checked snapshot allocation.
    let mut root = table(b"XSDT", 60);
    root[36..44].copy_from_slice(&0x16ff9u64.to_le_bytes());
    root[44..52].copy_from_slice(&0x19001u64.to_le_bytes());
    root[52..60].copy_from_slice(&0x1bfffu64.to_le_bytes());
    fix(&mut root, 9);
    let mut short_fadt = table(b"FACP", 116);
    short_fadt[8] = 1;
    fix(&mut short_fadt, 9);
    let short_mcfg = table(b"MCFG", 44); // no complete allocation record
    let mut short_madt = table(b"APIC", 46);
    short_madt[44] = 0;
    short_madt[45] = 2; // type 0 requires 8 bytes, not merely the common 2
    fix(&mut short_madt, 9);
    let mut state = mm::syspte::STATE.lock().unwrap();
    for (pa, bytes) in [
        (0x15001, root),
        (0x16ff9, short_fadt),
        (0x19001, short_mcfg),
        (0x1bfff, short_madt),
    ] {
        state.physical[pa..pa + bytes.len()].copy_from_slice(&bytes);
    }
    let mut rp = state.physical[rsdp..rsdp + 36].to_vec();
    rp[24..32].copy_from_slice(&0x15001u64.to_le_bytes());
    fix(&mut rp, 32);
    state.physical[rsdp..rsdp + 36].copy_from_slice(&rp);
    drop(state);
    let legacy = acpi::parse_fadt().unwrap();
    assert_eq!(
        (
            legacy.revision,
            legacy.minor_revision,
            legacy.reset_register
        ),
        (1, 0, 0)
    );
    assert_eq!(acpi::get_mcfg_info(), None);
    assert!(acpi::parse_madt());
    assert_eq!(acpi::madt_info().lapic_count, 0);
    let state = mm::syspte::STATE.lock().unwrap();
    assert!(state.maps.is_empty());
    assert_eq!(state.mapped, state.unmapped);
}
