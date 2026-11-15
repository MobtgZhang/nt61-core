use nt61_tools::fs::elf_to_pe::{append_base_relocations, elf_to_pe};

fn w16(b: &mut [u8], o: usize, v: u16) {
    b[o..o + 2].copy_from_slice(&v.to_le_bytes());
}
fn w32(b: &mut [u8], o: usize, v: u32) {
    b[o..o + 4].copy_from_slice(&v.to_le_bytes());
}
fn w64(b: &mut [u8], o: usize, v: u64) {
    b[o..o + 8].copy_from_slice(&v.to_le_bytes());
}
fn r32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}
fn r64(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(b[o..o + 8].try_into().unwrap())
}

fn fixture(machine: u16) -> Vec<u8> {
    let mut b = vec![0u8; 0x400];
    b[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
    w16(&mut b, 16, 2);
    w16(&mut b, 18, machine);
    w32(&mut b, 20, 1);
    w64(&mut b, 24, 0x201003);
    w64(&mut b, 32, 64);
    w16(&mut b, 52, 64);
    w16(&mut b, 54, 56);
    w16(&mut b, 56, 2);
    for (i, va, flags, off) in [(0, 0x203000, 6, 0x300), (1, 0x201003, 5, 0x200)] {
        let ph = 64 + i * 56;
        w32(&mut b, ph, 1);
        w32(&mut b, ph + 4, flags);
        w64(&mut b, ph + 8, off);
        w64(&mut b, ph + 16, va);
        w64(&mut b, ph + 32, 32);
        w64(&mut b, ph + 40, 64);
    }
    b[0x200..0x220].fill(0xcc);
    b[0x300..0x320].fill(0xdd);
    b
}

#[test]
fn all_architectures_preserve_linked_addresses_and_aligned_pe_sections() {
    for (arch, machine, pe_machine) in [
        ("x86_64", 62, 0x8664),
        ("aarch64", 183, 0xaa64),
        ("riscv64", 243, 0x5064),
        ("loongarch64", 258, 0x6264),
    ] {
        let pe = elf_to_pe(&fixture(machine), arch).unwrap();
        let base = r64(&pe, 0x98 + 24);
        assert_eq!(base & 0xffff, 0);
        assert_eq!(
            u16::from_le_bytes(pe[0x84..0x86].try_into().unwrap()),
            pe_machine
        );
        assert_eq!(base + u64::from(r32(&pe, 0x98 + 16)), 0x201003);
        let section = 0x188;
        assert_eq!(base + u64::from(r32(&pe, section + 12)), 0x201000);
        assert!(r32(&pe, section + 12) >= r32(&pe, 0x98 + 60));
        assert_eq!(r32(&pe, section + 16) % 0x1000, 0);
        let raw = r32(&pe, section + 20) as usize;
        assert_eq!(&pe[raw..raw + 3], &[0, 0, 0]);
        assert_eq!(&pe[raw + 3..raw + 35], &[0xcc; 32]);
    }
}

#[test]
fn malformed_elf_is_rejected_without_panicking() {
    let original = fixture(62);
    for (off, width, value) in [
        (5, 1, 2),
        (18, 2, 183),
        (32, 8, u64::MAX),
        (54, 2, 55),
        (64 + 32, 8, 65),
        (64 + 16, 8, u64::MAX - 15),
        (24, 8, 0x203000),
    ] {
        let mut b = original.clone();
        match width {
            1 => b[off] = value as u8,
            2 => w16(&mut b, off, value as u16),
            _ => w64(&mut b, off, value),
        }
        assert!(elf_to_pe(&b, "x86_64").is_none(), "field {off}");
    }
    let mut overlap = original;
    w64(&mut overlap, 64 + 16, 0x201010);
    assert!(elf_to_pe(&overlap, "x86_64").is_none());
    assert!(elf_to_pe(&fixture(62), "mystery").is_none());
}

#[test]
fn bss_segment_is_preserved_without_uninitialized_file_bytes() {
    let mut elf = fixture(62);
    w64(&mut elf, 64 + 32, 0);
    w64(&mut elf, 64 + 40, 0x4000);
    let pe = elf_to_pe(&elf, "x86_64").unwrap();
    let header = 0x188 + 40;
    assert_eq!(r32(&pe, header + 8), 0x4000);
    assert_eq!(r32(&pe, header + 16), 0);
    assert_eq!(r32(&pe, header + 20), 0);
}

#[test]
fn append_dir64_blocks_sort_deduplicate_pad_and_validate_targets() {
    let mut pe = elf_to_pe(&fixture(62), "x86_64").unwrap();
    let before = pe.clone();
    append_base_relocations(&mut pe, &[]).unwrap();
    assert_eq!(pe, before);
    assert!(append_base_relocations(&mut pe, &[0]).is_none());
    assert_eq!(pe, before);
    assert!(append_base_relocations(&mut pe, &[0x3ffc]).is_none());
    assert_eq!(pe, before);
    append_base_relocations(&mut pe, &[0x3008, 0x1008, 0x1008]).unwrap();
    let header = 0x188 + 80;
    let raw = r32(&pe, header + 20) as usize;
    assert_eq!(&pe[header..header + 6], b".reloc");
    assert_eq!(r32(&pe, 0x98 + 0x98), 0x4000);
    assert_eq!(r32(&pe, 0x98 + 0x9c), 24);
    assert_eq!(r32(&pe, raw), 0x1000);
    assert_eq!(r32(&pe, raw + 4), 12);
    assert_eq!(&pe[raw + 8..raw + 12], &[0x08, 0xa0, 0, 0]);
    assert_eq!(r32(&pe, raw + 12), 0x3000);
    assert_eq!(r32(&pe, 0x98 + 0x38), 0x5000);
    assert_eq!(pe[0x84 + 18] & 1, 0);
}

#[test]
fn adjacent_segments_sharing_a_page_keep_bytes_and_union_permissions() {
    let mut elf = fixture(62);
    w64(&mut elf, 64 + 16, 0x201080);
    let pe = elf_to_pe(&elf, "x86_64").unwrap();
    assert_eq!(u16::from_le_bytes(pe[0x86..0x88].try_into().unwrap()), 1);
    let raw = r32(&pe, 0x188 + 20) as usize;
    assert_eq!(&pe[raw + 3..raw + 35], &[0xcc; 32]);
    assert_eq!(&pe[raw + 0x80..raw + 0xa0], &[0xdd; 32]);
    assert_eq!(r32(&pe, 0x188 + 36) & 0xe000_0000, 0xe000_0000);
}

#[test]
fn relocation_append_rejects_duplicate_append_without_mutation() {
    let mut pe = elf_to_pe(&fixture(62), "x86_64").unwrap();
    append_base_relocations(&mut pe, &[0x1008]).unwrap();
    let before = pe.clone();
    assert!(append_base_relocations(&mut pe, &[0x1010]).is_none());
    assert_eq!(pe, before);
}

#[path = "../../winload/src/boot_image.rs"]
#[allow(dead_code)]
mod boot_image;

#[test]
fn converted_images_rebase_through_the_real_boot_loader_on_all_architectures() {
    for (arch, elf_machine, pe_machine) in [
        ("x86_64", 62, 0x8664),
        ("aarch64", 183, 0xaa64),
        ("riscv64", 243, 0x5064),
        ("loongarch64", 258, 0x6264),
    ] {
        let mut elf = fixture(elf_machine);
        w64(&mut elf, 0x300, 0x201003);
        let mut pe = elf_to_pe(&elf, arch).unwrap();
        append_base_relocations(&mut pe, &[0x3000]).unwrap();
        let header = boot_image::Header::parse(&pe, pe_machine).unwrap();
        for runtime_base in [0x100000u64, 0x80000000] {
            let mut mapped = vec![0; header.image_size];
            header.map(&pe, &mut mapped).unwrap();
            assert_eq!(header.relocate(&mut mapped, runtime_base).unwrap(), 1);
            assert_eq!(r64(&mapped, 0x3000), runtime_base + 0x1003);
            assert_eq!(&mapped[0x1003..0x1023], &[0xcc; 32]);
            assert_eq!(&mapped[..2], b"MZ");
            assert_eq!(&mapped[0x3020..0x3040], &[0; 32]);
        }
    }
}
