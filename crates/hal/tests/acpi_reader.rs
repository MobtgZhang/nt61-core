//! The exact production parser operates on simulated physical memory. Tables
//! deliberately straddle page boundaries and exceed a single mapped page.
#[path = "../src/hal/common/acpi_reader.rs"]
mod reader;
#[path = "../src/hal/common/acpi_rtc.rs"]
mod rtc;
use std::collections::BTreeMap;
fn fix(bytes: &mut [u8], checksum: usize) {
    bytes[checksum] = 0;
    bytes[checksum] = 0u8.wrapping_sub(bytes.iter().fold(0u8, |sum, &b| sum.wrapping_add(b)));
}
fn table(sig: &[u8; 4], length: usize) -> Vec<u8> {
    let mut bytes = vec![0; length];
    bytes[..4].copy_from_slice(sig);
    bytes[4..8].copy_from_slice(&(length as u32).to_le_bytes());
    fix(&mut bytes, 9);
    bytes
}
fn rsdp(xsdt: u64, rsdt: u32) -> Vec<u8> {
    let mut bytes = vec![0; 36];
    bytes[..8].copy_from_slice(b"RSD PTR ");
    bytes[15] = 2;
    bytes[16..20].copy_from_slice(&rsdt.to_le_bytes());
    bytes[20..24].copy_from_slice(&36u32.to_le_bytes());
    bytes[24..32].copy_from_slice(&xsdt.to_le_bytes());
    fix(&mut bytes[..20], 8);
    fix(&mut bytes, 32);
    bytes
}
fn fixture() -> BTreeMap<u64, Vec<u8>> {
    let mut memory = BTreeMap::new();
    memory.insert(0x1ff8, rsdp(0x3ff0, 0x9001));
    let mut xsdt = table(b"XSDT", 36 + 8 * 600);
    xsdt[36 + 599 * 8..36 + 600 * 8].copy_from_slice(&0x8ff9u64.to_le_bytes());
    fix(&mut xsdt, 9);
    memory.insert(0x3ff0, xsdt);
    let mut rsdt = table(b"RSDT", 40);
    rsdt[36..40].copy_from_slice(&0x8ff9u32.to_le_bytes());
    fix(&mut rsdt, 9);
    memory.insert(0x9001, rsdt);
    let mut fadt = table(b"FACP", 5000);
    fadt[108] = 0x32;
    fix(&mut fadt, 9);
    memory.insert(0x8ff9, fadt);
    memory
}
fn read(memory: &BTreeMap<u64, Vec<u8>>, pa: u64, len: usize) -> Option<Vec<u8>> {
    memory.get(&pa)?.get(..len).map(|b| b.to_vec())
}
#[test]
fn aligned_mapping_includes_physical_offset_and_cross_page_tail() {
    for (physical, length, base, offset, pages) in [
        (0x1000, 4096, 0x1000, 0, 1),
        (0x1ff8, 20, 0x1000, 4088, 2),
        (0x3ff0, 4836, 0x3000, 4080, 3),
        (0x8ff9, 5000, 0x8000, 4089, 3),
    ] {
        assert_eq!(
            reader::mapping_range(physical, length),
            Some(reader::MappingRange {
                physical_base: base,
                offset,
                pages
            })
        );
    }
    for (physical, length) in [
        (0, 36),
        (1, 0),
        (u64::MAX - 10, 36),
        (0x1000, reader::MAX_TABLE_LENGTH + 1),
    ] {
        assert!(reader::mapping_range(physical, length).is_none());
    }
}
#[test]
fn multipage_unaligned_tables_and_century_use_the_same_checked_parser() {
    let memory = fixture();
    let mut source = |pa, len| read(&memory, pa, len);
    let roots = reader::roots(&mut source, 0x1ff8).unwrap();
    assert_eq!(roots, [(0x3ff0, 8), (0x9001, 4)]);
    let root = reader::table(&mut source, roots[0].0).unwrap();
    assert_eq!(root.len(), 4836);
    assert_eq!(reader::root_entries(&root, 8).unwrap(), [0x8ff9]);
    assert_eq!(reader::table(&mut source, 0x8ff9).unwrap().len(), 5000);
    assert_eq!(rtc::find_century_register(source, 0x1ff8), Some(0x32));
}
#[test]
fn both_rsdp_checksums_and_exact_signature_are_required() {
    for corrupt in [0, 8, 32] {
        let mut memory = fixture();
        let rsdp = memory.get_mut(&0x1ff8).unwrap();
        rsdp[corrupt] ^= 1;
        if corrupt == 8 {
            fix(rsdp, 32);
        } // extended checksum alone is insufficient
        assert!(reader::roots(&mut |pa, len| read(&memory, pa, len), 0x1ff8).is_none());
    }
    for length in [0u32, 20, 35, 4097, u32::MAX] {
        let mut memory = fixture();
        let bytes = memory.get_mut(&0x1ff8).unwrap();
        bytes[20..24].copy_from_slice(&length.to_le_bytes());
        fix(bytes, 32);
        assert!(reader::roots(&mut |pa, len| read(&memory, pa, len), 0x1ff8).is_none());
    }
}
#[test]
fn malformed_table_lengths_checksums_and_root_shapes_are_rejected() {
    for length in [0u32, 35, reader::MAX_TABLE_LENGTH as u32 + 1, u32::MAX] {
        let mut bytes = table(b"FACP", 36);
        bytes[4..8].copy_from_slice(&length.to_le_bytes());
        assert!(
            reader::table(&mut |_, len| bytes.get(..len).map(|b| b.to_vec()), 0x1000).is_none()
        );
    }
    let mut bytes = table(b"XSDT", 44);
    bytes[9] ^= 1;
    assert!(reader::root_entries(&bytes, 8).is_none());
    assert!(reader::table(&mut |_, len| bytes.get(..len).map(|b| b.to_vec()), 0x1000).is_none());
    for (sig, length, stride) in [(b"FACP", 44, 8), (b"XSDT", 43, 8), (b"RSDT", 41, 4)] {
        assert!(reader::root_entries(&table(sig, length), stride).is_none());
    }
}
#[test]
fn truncated_or_changed_reads_cannot_escape_validated_extent() {
    let valid = table(b"FACP", 128);
    let mut count = 0;
    assert!(reader::table(
        &mut |_, len| {
            count += 1;
            let mut bytes = valid[..len].to_vec();
            if count == 2 {
                bytes[4] = 36;
                fix(&mut bytes, 9);
            }
            Some(bytes)
        },
        0x1000
    )
    .is_none());
    assert!(reader::table(&mut |_, len| Some(valid[..len - 1].to_vec()), 0x1000).is_none());
}
#[test]
fn century_falls_back_to_rsdt_and_rejects_standard_rtc_registers() {
    let mut memory = fixture();
    memory.get_mut(&0x3ff0).unwrap()[9] ^= 1;
    assert_eq!(
        rtc::find_century_register(|pa, len| read(&memory, pa, len), 0x1ff8),
        Some(0x32)
    );
    for index in [0, 1, 0x0d, 0x80, 0xff] {
        let bytes = memory.get_mut(&0x8ff9).unwrap();
        bytes[108] = index;
        fix(bytes, 9);
        assert_eq!(
            rtc::find_century_register(|pa, len| read(&memory, pa, len), 0x1ff8),
            None
        );
    }
}
