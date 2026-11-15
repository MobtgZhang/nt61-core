use nt61_boot_types::native_exports::{ExportTarget, NativeExport};
use nt61_tools::fs::{
    elf_to_pe::{append_base_relocations, elf_to_pe},
    pe_exports::append_exports_from_elf,
    pe_read,
};

fn w16(b: &mut [u8], o: usize, n: u16) {
    b[o..o + 2].copy_from_slice(&n.to_le_bytes());
}
fn w32(b: &mut [u8], o: usize, n: u32) {
    b[o..o + 4].copy_from_slice(&n.to_le_bytes());
}
fn w64(b: &mut [u8], o: usize, n: u64) {
    b[o..o + 8].copy_from_slice(&n.to_le_bytes());
}
fn fixture(machine: u16) -> Vec<u8> {
    let mut b = vec![0; 0x800];
    b[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
    w16(&mut b, 16, 2);
    w16(&mut b, 18, machine);
    w32(&mut b, 20, 1);
    w64(&mut b, 24, 0x201000);
    w64(&mut b, 32, 64);
    w16(&mut b, 52, 64);
    w16(&mut b, 54, 56);
    w16(&mut b, 56, 2);
    for (i, va, flags, off) in [(0, 0x201000, 5, 0x200), (1, 0x203000, 6, 0x300)] {
        let p = 64 + i * 56;
        w32(&mut b, p, 1);
        w32(&mut b, p + 4, flags);
        w64(&mut b, p + 8, off);
        w64(&mut b, p + 16, va);
        w64(&mut b, p + 32, 32);
        w64(&mut b, p + 40, 64);
    }
    b[0x200..0x220].fill(0xcc);
    b[0x300..0x320].fill(0xdd);
    w64(&mut b, 40, 0x400);
    w16(&mut b, 58, 64);
    w16(&mut b, 60, 5);
    for (i, kind, flags, va, off, len) in [
        (1, 1, 6, 0x201000, 0x200, 32),
        (2, 8, 3, 0x203000, 0x300, 64),
        (3, 2, 0, 0, 0x600, 72),
        (4, 3, 0, 0, 0x680, 12),
    ] {
        let s = 0x400 + i * 64;
        w32(&mut b, s + 4, kind);
        w64(&mut b, s + 8, flags);
        w64(&mut b, s + 16, va);
        w64(&mut b, s + 24, off);
        w64(&mut b, s + 32, len);
    }
    w32(&mut b, 0x400 + 3 * 64 + 40, 4);
    w64(&mut b, 0x400 + 3 * 64 + 56, 24);
    b[0x680..0x68c].copy_from_slice(b"\0func\0value\0");
    for (i, name, kind, section, va, len) in
        [(1, 1, 0x12, 1, 0x201000, 16), (2, 6, 0x11, 2, 0x203030, 8)]
    {
        let s = 0x600 + i * 24;
        w32(&mut b, s, name);
        b[s + 4] = kind;
        w16(&mut b, s + 6, section);
        w64(&mut b, s + 8, va);
        w64(&mut b, s + 16, len);
    }
    b
}
const EXPORTS: &[NativeExport] = &[
    NativeExport {
        name: "Value",
        ordinal: 7,
        target: ExportTarget::Data("value"),
    },
    NativeExport {
        name: "Call",
        ordinal: 5,
        target: ExportTarget::Function("func"),
    },
    NativeExport {
        name: "Forward",
        ordinal: 8,
        target: ExportTarget::Forwarder("other.#17"),
    },
];

#[test]
fn real_function_data_and_forwarders_on_four_architectures() {
    for (arch, machine) in [
        ("x86_64", 62),
        ("aarch64", 183),
        ("riscv64", 243),
        ("loongarch64", 258),
    ] {
        let elf = fixture(machine);
        let mut pe = elf_to_pe(&elf, arch).unwrap();
        append_base_relocations(&mut pe, &[0x3030]).unwrap();
        let entry = pe_read::parse(&pe).unwrap().entry_point;
        append_exports_from_elf(&mut pe, &elf, "native.dll", EXPORTS).unwrap();
        let parsed = pe_read::parse(&pe).unwrap();
        assert_eq!(parsed.entry_point, entry);
        assert_eq!(parsed.exports_by_name["Call"], 0x1000);
        assert_eq!(parsed.exports_by_name["Value"], 0x3030);
        assert_eq!(parsed.exports_by_ordinal[&7], 0x3030);
        assert_eq!(parsed.forwarders["Forward"], "other.#17");
        assert_eq!(parsed.relocations, vec![0x3030]);
        assert_eq!(parsed.exports_by_ordinal.get(&6).copied().unwrap_or(0), 0);
        assert_eq!(parsed.sections.last().unwrap().characteristics, 0x40000040);
        let before = pe.clone();
        assert!(append_exports_from_elf(&mut pe, &elf, "native.dll", EXPORTS).is_err());
        assert_eq!(pe, before);
    }
}

#[test]
fn invalid_symbol_kind_missing_and_duplicate_exports_fail_atomically() {
    let elf = fixture(62);
    let original = elf_to_pe(&elf, "x86_64").unwrap();
    for exports in [
        vec![NativeExport {
            name: "Bad",
            ordinal: 1,
            target: ExportTarget::Function("value"),
        }],
        vec![NativeExport {
            name: "Bad",
            ordinal: 1,
            target: ExportTarget::Data("func"),
        }],
        vec![NativeExport {
            name: "Bad",
            ordinal: 1,
            target: ExportTarget::Function("missing"),
        }],
        vec![EXPORTS[0], EXPORTS[0]],
        vec![NativeExport {
            name: "Bad",
            ordinal: 1,
            target: ExportTarget::Forwarder("other.#0"),
        }],
        vec![NativeExport {
            name: "Bad",
            ordinal: 1,
            target: ExportTarget::Forwarder("other.#+1"),
        }],
        vec![NativeExport {
            name: "Bad",
            ordinal: 1,
            target: ExportTarget::Forwarder("no_separator"),
        }],
    ] {
        let mut pe = original.clone();
        assert!(append_exports_from_elf(&mut pe, &elf, "native.dll", &exports).is_err());
        assert_eq!(pe, original);
    }
}

#[test]
fn malformed_tables_bounds_architecture_and_section_permissions_fail_atomically() {
    let elf = fixture(62);
    let original = elf_to_pe(&elf, "x86_64").unwrap();
    for (offset, width, value) in [
        (40, 8, u64::MAX),
        (58, 2, 63),
        (18, 2, 183),
        (0x600 + 24 + 6, 2, 0),
        (0x600 + 24 + 8, 8, 0x201020),
        (0x600 + 24 + 16, 8, 0x1000),
        (0x400 + 3 * 64 + 40, 4, 9),
        (0x600 + 24 + 4, 1, 0x11),
    ] {
        let mut bad = elf.clone();
        match width {
            8 => w64(&mut bad, offset, value),
            4 => w32(&mut bad, offset, value as u32),
            2 => w16(&mut bad, offset, value as u16),
            _ => bad[offset] = value as u8,
        };
        let mut pe = original.clone();
        assert!(
            append_exports_from_elf(&mut pe, &bad, "native.dll", EXPORTS).is_err(),
            "offset {offset}"
        );
        assert_eq!(pe, original);
    }
    for (offset, value) in [
        (0x188 + 36, 0x40000040),
        (0x98 + 60, 0x188 + 80),
        (0x98 + 144, 0x1000),
    ] {
        let mut pe = original.clone();
        w32(&mut pe, offset, value);
        let before = pe.clone();
        assert!(append_exports_from_elf(&mut pe, &elf, "native.dll", EXPORTS).is_err());
        assert_eq!(pe, before);
    }
}

#[test]
fn function_exports_reject_pe_code_from_a_different_elf_build() {
    let elf = fixture(62);
    let mut pe = elf_to_pe(&elf, "x86_64").unwrap();
    let code = u32::from_le_bytes(pe[0x188 + 20..0x188 + 24].try_into().unwrap()) as usize;
    pe[code] ^= 1;
    let before = pe.clone();
    assert!(append_exports_from_elf(&mut pe, &elf, "native.dll", EXPORTS).is_err());
    assert_eq!(pe, before);
}

#[path = "../../ntoskrnl/src/native_exports.rs"]
#[allow(dead_code)]
mod api;

#[test]
fn native_memory_apis_use_typed_efiabi_and_preserve_memory_semantics() {
    // On this x86_64 host the function pointer explicitly uses Win64, exercising
    // register ordering/shadow space rather than accidentally calling SysV.
    let copy: unsafe extern "efiapi" fn(*mut u8, *const u8, usize) = api::nt61_export_RtlCopyMemory;
    let mut a = [0u8; 8];
    let b = *b"abcdefgh";
    unsafe {
        copy(a.as_mut_ptr(), b.as_ptr(), 8);
        assert_eq!(a, b);
        api::nt61_export_RtlMoveMemory(a.as_mut_ptr().add(2), a.as_ptr(), 6);
        assert_eq!(&a, b"ababcdef");
        api::nt61_export_RtlFillMemory(a.as_mut_ptr(), 3, 0x66);
        assert_eq!(&a, b"fffbcdef");
        assert_eq!(
            api::nt61_export_RtlCompareMemory(a.as_ptr(), b.as_ptr(), 8),
            0
        );
        assert_eq!(
            api::nt61_export_RtlCompareMemory(b.as_ptr(), b.as_ptr(), 8),
            8
        );
        api::nt61_export_RtlZeroMemory(a.as_mut_ptr(), 8);
        assert_eq!(a, [0; 8]);
        copy(core::ptr::null_mut(), core::ptr::null(), 0);
    }
}

#[test]
fn unicode_descriptor_layout_boundaries_and_failure_behavior() {
    assert_eq!(core::mem::size_of::<api::UnicodeString>(), 16);
    let source = [0x41u16, 0x4e2d, 0];
    let mut out = api::UnicodeString {
        length: 99,
        maximum_length: 99,
        buffer: core::ptr::null(),
    };
    unsafe {
        api::nt61_export_RtlInitUnicodeString(&mut out, source.as_ptr());
        assert_eq!(out.length, 4);
        assert_eq!(out.maximum_length, 6);
        assert_eq!(out.buffer, source.as_ptr());
        assert_eq!(
            api::nt61_export_RtlInitUnicodeStringEx(&mut out, core::ptr::null()),
            0
        );
        assert_eq!(out.length, 0);
        assert_eq!(out.maximum_length, 0);
        assert!(out.buffer.is_null());
        let mut large = vec![1u16; 0x8000];
        large[0x7fff] = 0;
        out.length = 123;
        assert_eq!(
            api::nt61_export_RtlInitUnicodeStringEx(&mut out, large.as_ptr()),
            0xc0000106u32 as i32
        );
        assert_eq!(out.length, 0);
        assert_eq!(out.maximum_length, 0);
        assert_eq!(out.buffer, large.as_ptr());
        api::nt61_export_RtlInitUnicodeString(&mut out, large.as_ptr());
        assert_eq!(out.length, 0xfffc);
        assert_eq!(out.maximum_length, 0xfffe);
        large[0x7ffe] = 0;
        assert_eq!(
            api::nt61_export_RtlInitUnicodeStringEx(&mut out, large.as_ptr()),
            0
        );
        assert_eq!(out.maximum_length, 0xfffe);
    }
}


// Compile and run the dedicated production string ABI/edge-case harness with
// the regular PE export tests so native services cannot bypass routine checks.
#[path = "../../ntoskrnl/tests/native_strings_cases.rs"]
#[allow(dead_code)]
mod native_strings;
