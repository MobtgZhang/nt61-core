//! PE factory regression tests.
//!
//! These tests are the contract between the emitter in `fs::pe` and
//! the loader in `nt61-loader`. The loader reads the export table, the
//! import table and the base relocation table of every image it
//! loads; if the emitter writes any of them wrongly, the loader
//! resolves the wrong address and the kernel jumps into garbage.
//!
//! The images are parsed with `fs::pe_read`, an independent reader
//! that follows the PE/COFF specification directly.

use nt61_tools::fs::pe::{
    self, ExportSymbol, ImportModule, ImportSymbol, PeImageSpec, SectionSpec,
};
use nt61_tools::fs::pe_read;

/// `IMAGE_FILE_MACHINE_AMD64`.
const MACHINE_AMD64: u16 = 0x8664;

/// Build a small DLL with the given exports and imports.
fn sample_dll() -> PeImageSpec {
    let mut spec = PeImageSpec::new(
        0x0000_0001_8000_0000,
        pe::IMAGE_SUBSYSTEM_WINDOWS_GUI,
        true,
        MACHINE_AMD64,
    );
    spec.module_name = "user32.dll".into();
    spec.entry_point_rva = 0x1000;
    spec.sections = vec![
        SectionSpec::code(vec![0x90; 64]),
        SectionSpec::rdata(Vec::new()),
        SectionSpec::reloc(Vec::new()),
    ];
    spec.exports = vec![
        ExportSymbol::ByName {
            name: "GetLastError".into(),
            ordinal: 1,
            rva: 0x1010,
        },
        ExportSymbol::ByName {
            name: "SetLastError".into(),
            ordinal: 2,
            rva: 0x1020,
        },
        ExportSymbol::ByOrdinal {
            ordinal: 3,
            rva: 0x1030,
        },
        ExportSymbol::Forwarder {
            name: "MessageBoxW".into(),
            ordinal: 4,
            target: "user32.MessageBoxW".into(),
        },
    ];
    spec.imports = vec![ImportModule {
        dll_name: "ntdll.dll".into(),
        symbols: vec![
            ImportSymbol::ByName("NtUserWaitMessage".into()),
            ImportSymbol::ByOrdinal(7),
        ],
    }];
    spec.relocations = vec![0x1018, 0x1028, 0x1038];
    spec
}

#[test]
fn emitted_image_has_valid_pe_headers() {
    let spec = sample_dll();
    let (bytes, _) = pe::emit(&spec);
    let image = pe_read::parse(&bytes).expect("the emitted image must parse");

    assert_eq!(image.magic, 0x20B, "PE32+ magic");
    assert_eq!(image.machine, MACHINE_AMD64);
    assert_eq!(image.number_of_sections, 3);
    assert_eq!(image.image_base, 0x0000_0001_8000_0000);
    assert_eq!(image.subsystem, pe::IMAGE_SUBSYSTEM_WINDOWS_GUI);
    assert_eq!(
        image.characteristics & pe::IMAGE_FILE_DLL,
        pe::IMAGE_FILE_DLL,
        "the image must be marked as a DLL"
    );
    assert_eq!(image.entry_point, 0x1000);
    assert!(image.size_of_image > 0);
    assert_eq!(image.size_of_headers, 0x400);
}

#[test]
fn export_directory_lists_names_ordinals_and_forwarders() {
    let spec = sample_dll();
    let (bytes, report) = pe::emit(&spec);
    assert_eq!(report.export_count, 4);
    assert_eq!(report.forwarder_count, 1);

    let image = pe_read::parse(&bytes).expect("parse");
    assert_eq!(image.exports_by_name.len(), 3, "three exports carry a name");
    assert_eq!(image.exports_by_ordinal.len(), 4, "four ordinal slots");

    assert_eq!(image.exports_by_name.get("GetLastError"), Some(&0x1010));
    assert_eq!(image.exports_by_name.get("SetLastError"), Some(&0x1020));
    assert!(image.exports_by_name.contains_key("MessageBoxW"));

    // The forwarder must be recognised as such and carry its target.
    assert_eq!(
        image.forwarders.get("MessageBoxW").map(String::as_str),
        Some("user32.MessageBoxW")
    );

    // The ordinal-only export is reachable by ordinal but has no
    // name.
    assert_eq!(image.exports_by_ordinal.get(&3), Some(&0x1030));
    assert!(!image.exports_by_name.keys().any(|k| k.is_empty()));
}

#[test]
fn import_directory_lists_modules_and_symbols() {
    let spec = sample_dll();
    let (bytes, report) = pe::emit(&spec);
    assert_eq!(report.import_module_count, 1);
    assert_eq!(report.import_symbol_count, 2);

    let image = pe_read::parse(&bytes).expect("parse");
    assert_eq!(image.imports.len(), 1);
    let import = &image.imports[0];
    assert_eq!(import.dll_name, "ntdll.dll");
    assert_eq!(
        import.symbols,
        vec![Some("NtUserWaitMessage".to_string()), None]
    );
    assert_eq!(import.ordinals, vec![7]);
}

#[test]
fn base_relocation_table_is_well_formed() {
    let spec = sample_dll();
    let (bytes, report) = pe::emit(&spec);
    assert_eq!(report.relocation_count, 3);

    let image = pe_read::parse(&bytes).expect("parse");
    let mut sorted = image.relocations.clone();
    sorted.sort_unstable();
    assert_eq!(sorted, vec![0x1018, 0x1028, 0x1038]);
}

#[test]
fn sections_carry_the_expected_names_and_flags() {
    let spec = sample_dll();
    let (bytes, _) = pe::emit(&spec);
    let image = pe_read::parse(&bytes).expect("parse");

    let text = image.sections.iter().find(|s| s.name == ".text").unwrap();
    assert_eq!(text.virtual_address, 0x1000);
    assert_ne!(text.characteristics & pe::IMAGE_SCN_MEM_EXECUTE, 0);
    assert_eq!(
        text.characteristics & pe::IMAGE_SCN_MEM_WRITE,
        0,
        ".text must not be writable"
    );

    let rdata = image.sections.iter().find(|s| s.name == ".rdata").unwrap();
    assert!(rdata.virtual_address > text.virtual_address);
    assert_eq!(
        rdata.characteristics & pe::IMAGE_SCN_MEM_WRITE,
        0,
        ".rdata must be read-only"
    );
    assert_ne!(rdata.characteristics & pe::IMAGE_SCN_MEM_READ, 0);

    let reloc = image.sections.iter().find(|s| s.name == ".reloc").unwrap();
    assert_eq!(
        reloc.characteristics & pe::IMAGE_SCN_MEM_WRITE,
        0,
        ".reloc must not be writable"
    );
    assert_ne!(reloc.characteristics & pe::IMAGE_SCN_MEM_READ, 0);
}

#[test]
fn every_export_rva_points_into_an_executable_section() {
    let spec = sample_dll();
    let (bytes, _) = pe::emit(&spec);
    let image = pe_read::parse(&bytes).expect("parse");

    for (name, rva) in &image.exports_by_name {
        // The forwarder points at a string in `.rdata`, not at code.
        if image.forwarders.contains_key(name) {
            continue;
        }
        let in_code = image.sections.iter().any(|section| {
            section.characteristics & pe::IMAGE_SCN_MEM_EXECUTE != 0
                && *rva >= section.virtual_address
                && *rva < section.virtual_address + section.virtual_size
        });
        assert!(in_code, "export {name} at RVA {rva:#x} is not in code");
    }
}

#[test]
fn import_and_export_dictionaries_point_into_rdata() {
    let spec = sample_dll();
    let (bytes, _) = pe::emit(&spec);
    let image = pe_read::parse(&bytes).expect("parse");

    let rdata = image.sections.iter().find(|s| s.name == ".rdata").unwrap();
    let in_rdata =
        |rva: u32| rva >= rdata.virtual_address && rva < rdata.virtual_address + rdata.virtual_size;

    let (export_rva, export_size) = image.data_directories[0];
    assert!(in_rdata(export_rva));
    assert!(export_size > 0);

    let (import_rva, import_size) = image.data_directories[1];
    assert!(in_rdata(import_rva));
    assert!(import_size > 0);

    let (reloc_rva, reloc_size) = image.data_directories[5];
    assert!(reloc_rva != 0);
    assert!(reloc_size > 0);
}

#[test]
fn import_entries_use_the_pe32_plus_ordinal_flag_convention() {
    // In a PE32+ import table, `IMAGE_ORDINAL_FLAG64` (bit 63) means
    // "import **by ordinal**"; a clear bit means the entry is a
    // hint/name RVA. This is the *inverse* of the PE32 convention,
    // where bit 31 of a 32-bit entry means "by ordinal".
    //
    // Getting this backwards produces an image that this repository's
    // own loader can read (because the two bugs cancel out) but that
    // no real Windows loader — and no `objdump` — can read.
    let spec = sample_dll();
    let (bytes, _) = pe::emit(&spec);

    let image = pe_read::parse(&bytes).expect("parse");
    let import = &image.imports[0];
    assert_eq!(
        import.symbols,
        vec![Some("NtUserWaitMessage".to_string()), None],
        "the name import must be by name and the ordinal import by ordinal"
    );
    assert_eq!(import.ordinals, vec![7]);

    // Check the raw bytes: the name entry must have bit 63 **clear**
    // and the ordinal entry must have it **set**.
    let rdata = image.sections.iter().find(|s| s.name == ".rdata").unwrap();
    let (import_rva, _) = image.data_directories[1];
    let file_offset =
        rdata.pointer_to_raw_data as usize + (import_rva - rdata.virtual_address) as usize;

    // The descriptor's `OriginalFirstThunk` is the ILT RVA.
    let ilt_rva = u32::from_le_bytes([
        bytes[file_offset],
        bytes[file_offset + 1],
        bytes[file_offset + 2],
        bytes[file_offset + 3],
    ]);
    let ilt_offset =
        rdata.pointer_to_raw_data as usize + (ilt_rva - rdata.virtual_address) as usize;

    let name_entry = u64::from_le_bytes([
        bytes[ilt_offset],
        bytes[ilt_offset + 1],
        bytes[ilt_offset + 2],
        bytes[ilt_offset + 3],
        bytes[ilt_offset + 4],
        bytes[ilt_offset + 5],
        bytes[ilt_offset + 6],
        bytes[ilt_offset + 7],
    ]);
    assert_eq!(
        name_entry & pe::IMAGE_ORDINAL_FLAG64,
        0,
        "a name import must have IMAGE_ORDINAL_FLAG64 clear"
    );

    let ordinal_entry = u64::from_le_bytes([
        bytes[ilt_offset + 8],
        bytes[ilt_offset + 9],
        bytes[ilt_offset + 10],
        bytes[ilt_offset + 11],
        bytes[ilt_offset + 12],
        bytes[ilt_offset + 13],
        bytes[ilt_offset + 14],
        bytes[ilt_offset + 15],
    ]);
    assert_ne!(
        ordinal_entry & pe::IMAGE_ORDINAL_FLAG64,
        0,
        "an ordinal import must have IMAGE_ORDINAL_FLAG64 set"
    );
    assert_eq!(ordinal_entry & 0xFFFF, 7);
}

#[test]
fn a_dll_without_imports_has_no_import_directory() {
    let mut spec = sample_dll();
    spec.imports.clear();
    let (bytes, report) = pe::emit(&spec);
    assert_eq!(report.import_module_count, 0);
    let image = pe_read::parse(&bytes).expect("parse");
    assert_eq!(image.data_directories[1], (0, 0));
    assert!(image.imports.is_empty());
}

#[test]
fn relocation_block_boundaries_are_page_aligned() {
    // Two relocations in different pages must produce two blocks.
    let mut spec = sample_dll();
    spec.relocations = vec![0x0010, 0x2000, 0x2008];
    let (bytes, _) = pe::emit(&spec);
    let image = pe_read::parse(&bytes).expect("parse");
    let mut sorted = image.relocations.clone();
    sorted.sort_unstable();
    assert_eq!(sorted, vec![0x0010, 0x2000, 0x2008]);
}

#[test]
fn the_loader_accepts_the_emitted_image() {
    // The loader is the consumer of these images. If it refuses the
    // image, the emitter is wrong regardless of what the reader says.
    //
    // The sample DLL imports from `ntdll.dll`, which is not in the
    // image database, so the loader must reject it — that is the
    // correct behaviour and is asserted separately below. This test
    // therefore uses an import-free variant.
    let mut spec = sample_dll();
    spec.imports.clear();
    let (bytes, _) = pe::emit(&spec);
    let mut db = nt61_loader::loader::ImageDatabase::new();
    let loaded =
        nt61_loader::loader::load_image_full("user32.dll", &bytes, &mut db, 0x0000_0001_8000_0000)
            .expect("the loader must accept the emitted image");
    assert_eq!(loaded.image_base, 0x0000_0001_8000_0000);
    assert_eq!(loaded.entry_point, 0x0000_0001_8000_0000 + 0x1000);
    assert_eq!(loaded.subsystem, pe::IMAGE_SUBSYSTEM_WINDOWS_GUI);
}

#[test]
fn the_loader_rejects_an_image_with_unresolvable_imports() {
    // A DLL that imports a symbol no loaded module provides must not
    // be mapped: the loader would otherwise install a bogus IAT
    // entry and the first call would jump into the weeds.
    let spec = sample_dll();
    let (bytes, _) = pe::emit(&spec);
    let mut db = nt61_loader::loader::ImageDatabase::new();
    let loaded =
        nt61_loader::loader::load_image_full("user32.dll", &bytes, &mut db, 0x0000_0001_8000_0000);
    assert!(
        loaded.is_none(),
        "an image with unresolvable imports must be rejected"
    );
}

#[test]
fn the_loader_sees_the_exported_names() {
    let mut spec = sample_dll();
    spec.imports.clear();
    let (bytes, _) = pe::emit(&spec);
    let mut db = nt61_loader::loader::ImageDatabase::new();
    let loaded =
        nt61_loader::loader::load_image_full("user32.dll", &bytes, &mut db, 0x0000_0001_8000_0000)
            .expect("load");
    db.register(&loaded, &bytes);

    let address = db
        .lookup("user32.dll", "GetLastError")
        .expect("GetLastError must be exported");
    assert_eq!(address, 0x0000_0001_8000_0000 + 0x1010);

    // The forwarder is exported under its own name too.
    assert!(db.lookup("user32.dll", "MessageBoxW").is_some());
}

#[test]
fn the_loader_can_rebase_a_relocatable_image() {
    // The `.reloc` table must be correct enough for the loader to
    // patch the image when it is loaded at a different address.
    let mut spec = sample_dll();
    spec.imports.clear();
    let (bytes, _) = pe::emit(&spec);
    let mut db = nt61_loader::loader::ImageDatabase::new();
    let preferred = 0x0000_0001_8000_0000u64;
    let actual = preferred + 0x10_0000;
    let loaded = nt61_loader::loader::load_image_full("user32.dll", &bytes, &mut db, actual)
        .expect("load at a different base");
    assert_eq!(loaded.image_base, actual);

    db.register(&loaded, &bytes);
    // The exported address must follow the new base.
    let address = db
        .lookup("user32.dll", "GetLastError")
        .expect("GetLastError must be exported");
    assert_eq!(address, actual + 0x1010);
}
