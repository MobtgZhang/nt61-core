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
            ImportSymbol::ByName("NtWaitForSingleObject".into()),
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
fn sparse_exports_preserve_original_ordinals_sorted_names_and_forwarder_slots() {
    for machine in [MACHINE_AMD64, 0x14c] {
        let mut spec = sample_dll();
        spec.machine = machine;
        spec.image_base = 0x6000_0000;
        // Input order, ordinal order, name order and dense EAT indices all
        // differ. In particular, the forwarders must use slots 3 and 13,
        // rather than their indices in the list of supplied exports.
        spec.exports = vec![
            ExportSymbol::Forwarder {
                name: "Beta".into(), ordinal: 1325, target: "kernel32.#17".into(),
            },
            ExportSymbol::ByName {
                name: "Zebra".into(), ordinal: 1312, rva: 0x1010,
            },
            ExportSymbol::ByName {
                name: "Middle".into(), ordinal: 1320, rva: 0x1020,
            },
            ExportSymbol::ByOrdinal { ordinal: 1314, rva: 0x1030 },
            ExportSymbol::Forwarder {
                name: "Alpha".into(), ordinal: 1315, target: "ntdll.RtlZeroMemory".into(),
            },
        ];
        let (bytes, report) = pe::emit(&spec);
        assert_eq!(report.export_count, 5);
        assert_eq!(report.forwarder_count, 2);
        let image = pe_read::parse(&bytes).expect("parse sparse exports");
        let file_offset = |rva: u32| {
            let section = image.sections.iter().find(|s| {
                rva >= s.virtual_address && rva < s.virtual_address + s.size_of_raw_data
            }).expect("RVA has file backing");
            (section.pointer_to_raw_data + rva - section.virtual_address) as usize
        };
        let u32_at = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        let directory = file_offset(image.data_directories[0].0);
        assert_eq!(u32_at(directory + 16), 1312, "Base must preserve the first ordinal");
        assert_eq!(u32_at(directory + 20), 14, "EAT includes unassigned ordinals");
        assert_eq!(u32_at(directory + 24), 4);
        let functions = file_offset(u32_at(directory + 28));
        let names = file_offset(u32_at(directory + 32));
        let ordinals = file_offset(u32_at(directory + 36));
        for index in [1usize, 4, 5, 6, 7, 9, 10, 11, 12] {
            assert_eq!(u32_at(functions + index * 4), 0, "hole {index} stays null");
        }
        for (index, (name, slot)) in [("Alpha", 3u16), ("Beta", 13), ("Middle", 8), ("Zebra", 0)]
            .into_iter().enumerate()
        {
            let at = file_offset(u32_at(names + index * 4));
            let end = bytes[at..].iter().position(|b| *b == 0).unwrap() + at;
            assert_eq!(&bytes[at..end], name.as_bytes(), "name table is lexically sorted");
            let actual_slot = u16::from_le_bytes(bytes[ordinals + index * 2..ordinals + index * 2 + 2]
                .try_into().unwrap());
            assert_eq!(actual_slot, slot, "name ordinal is unbiased EAT slot");
        }
        for (ordinal, rva) in [(1312, 0x1010), (1314, 0x1030), (1320, 0x1020)] {
            assert_eq!(image.exports_by_ordinal.get(&ordinal), Some(&rva));
        }
        assert_eq!(image.exports_by_name.get("Zebra"), Some(&0x1010));
        assert_eq!(image.exports_by_name.get("Middle"), Some(&0x1020));
        for (name, ordinal, target) in [("Alpha", 1315, "ntdll.RtlZeroMemory"),
                                      ("Beta", 1325, "kernel32.#17")] {
            assert_eq!(image.forwarders.get(name).map(String::as_str), Some(target));
            assert_eq!(image.exports_by_name.get(name), image.exports_by_ordinal.get(&ordinal));
        }
    }
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
        vec![Some("NtWaitForSingleObject".to_string()), None]
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
        vec![Some("NtWaitForSingleObject".to_string()), None],
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

    assert_eq!(db.lookup("user32.dll", "MessageBoxW"), None);
}

#[test]
fn boot_linker_rebases_and_binds_sparse_exports_in_place() {
    use nt61_boot_types::boot_image::Header;
    use nt61_loader::loader::{link_boot_image, ImageDatabase};

    let preferred = 0x1_8000_0000u64;
    let actual = preferred + 0x20000;
    let mut provider = sample_dll();
    provider.imports.clear();
    provider.exports.clear();
    provider
        .export_named("Zulu", 731, 0x1010)
        .export_named("Alpha", 733, 0x1020)
        .export_ordinal(736, 0x1030)
        .export_forwarder("Forwarded", 739, "USER32.#736");
    provider.relocations = vec![0x1018];
    let (raw, _) = pe::emit(&provider);
    let header = Header::parse(&raw, MACHINE_AMD64).unwrap();
    let mut mapped = vec![0; header.image_size];
    header.map(&raw, &mut mapped).unwrap();
    let database = ImageDatabase::new();
    assert_eq!(
        link_boot_image("user32.dll", header, &mut mapped, actual, &database),
        Ok(1)
    );
    assert_eq!(database.find_image_base("USER32.DLL"), Some(actual));
    assert_eq!(database.lookup("user32.dll", "Zulu"), Some(actual + 0x1010));
    assert_eq!(
        database.lookup("user32.dll", "Alpha"),
        Some(actual + 0x1020)
    );
    assert_eq!(database.lookup_ordinal("user32.dll", 732), None);
    assert_eq!(
        database.lookup_ordinal("user32.dll", 736),
        Some(actual + 0x1030)
    );
    assert_eq!(
        database.lookup("user32.dll", "Forwarded"),
        Some(actual + 0x1030)
    );

    let mut consumer = sample_dll();
    consumer.exports.clear();
    consumer.imports.clear();
    consumer.relocations = vec![0x1018];
    consumer.sections[0].data[0x18..0x20].copy_from_slice(&(preferred + 0x1010).to_le_bytes());
    consumer.import(
        "user32.dll",
        vec![
            ImportSymbol::ByName("Alpha".into()),
            ImportSymbol::ByOrdinal(732),
        ],
    );
    let (raw, _) = pe::emit(&consumer);
    let header = Header::parse(&raw, MACHINE_AMD64).unwrap();
    let mut mapped = vec![0; header.image_size];
    header.map(&raw, &mut mapped).unwrap();
    let before = mapped.clone();
    let lower_base = preferred - 0x20000;
    assert!(link_boot_image("consumer.dll", header, &mut mapped, lower_base, &database).is_err());
    assert_eq!(mapped, before);
    assert_eq!(database.find_image_base("consumer.dll"), None);

    consumer.imports[0].symbols[1] = ImportSymbol::ByOrdinal(739);
    let (raw, _) = pe::emit(&consumer);
    let header = Header::parse(&raw, MACHINE_AMD64).unwrap();
    header.map(&raw, &mut mapped).unwrap();
    let (_, _, _, optional) = nt61_loader::loader::parse_headers(&mapped).unwrap();
    let (imports, _) = nt61_loader::loader::read_data_directory_pe32plus(&mapped, optional, 1);
    let imports = imports as usize;
    let iat = u32::from_le_bytes(mapped[imports + 16..imports + 20].try_into().unwrap()) as usize;
    mapped[imports..imports + 4].fill(0);
    assert_eq!(
        link_boot_image("consumer.dll", header, &mut mapped, lower_base, &database),
        Ok(1)
    );
    assert_eq!(
        &mapped[0x1018..0x1020],
        &(lower_base + 0x1010).to_le_bytes()
    );
    assert_eq!(&mapped[iat..iat + 8], &(actual + 0x1020).to_le_bytes());
    assert_eq!(&mapped[iat + 8..iat + 16], &(actual + 0x1030).to_le_bytes());
    assert_eq!(&mapped[iat + 16..iat + 24], &[0; 8]);
}

#[test]
fn boot_set_links_mutual_kernel_and_hal_imports_before_publishing() {
    use nt61_boot_types::boot_image::Header;
    use nt61_loader::loader::{link_boot_images, read_data_directory_pe32plus, MappedBootImage};

    let preferred = 0x1_8000_0000u64;
    let hal_base = preferred + 0x10000;
    let kernel_base = preferred - 0x10000;
    for machine in [MACHINE_AMD64, 0xaa64, 0x5064, 0x6264] {
        let mut hal = sample_dll();
        hal.machine = machine;
        hal.module_name = "hal.dll".into();
        hal.imports.clear();
        hal.exports.clear();
        hal.export_named("HalRoutine", 4, 0x1010);
        hal.import("ntoskrnl.exe", vec![ImportSymbol::ByName("KernelRoutine".into())]);
        hal.relocations = vec![0x1018];
        hal.sections[0].data[0x18..0x20].copy_from_slice(&(preferred + 0x1010).to_le_bytes());

        let mut kernel = hal.clone();
        kernel.module_name = "ntoskrnl.exe".into();
        kernel.imports.clear();
        kernel.exports.clear();
        kernel.export_named("KernelRoutine", 10, 0x1030);
        kernel.import("HAL.dll", vec![ImportSymbol::ByOrdinal(4)]);

        let map = |spec: &PeImageSpec| {
            let (raw, _) = pe::emit(spec);
            let header = Header::parse(&raw, machine).unwrap();
            let mut mapped = vec![0; header.image_size];
            header.map(&raw, &mut mapped).unwrap();
            (header, mapped)
        };
        let (hal_header, mut hal_bytes) = map(&hal);
        let (kernel_header, mut kernel_bytes) = map(&kernel);
        let link = |hal_bytes: &mut [u8], kernel_bytes: &mut [u8]| {
            link_boot_images(&mut [
                MappedBootImage { name: "hal.dll", header: hal_header, bytes: hal_bytes, load_base: hal_base },
                MappedBootImage { name: "ntoskrnl.exe", header: kernel_header, bytes: kernel_bytes, load_base: kernel_base },
            ])
        };

        let hal_before = hal_bytes.clone();
        let (reloc_rva, _) = read_data_directory_pe32plus(&kernel_bytes, kernel_header.optional_offset, 5);
        let block_size = reloc_rva as usize + 4;
        let valid_size: [u8; 4] = kernel_bytes[block_size..block_size + 4].try_into().unwrap();
        kernel_bytes[block_size..block_size + 4].fill(0);
        let malformed = kernel_bytes.clone();
        assert_eq!(link(&mut hal_bytes, &mut kernel_bytes), Err(("ntoskrnl.exe", "invalid relocation block size")));
        assert_eq!(hal_bytes, hal_before);
        assert_eq!(kernel_bytes, malformed);

        kernel_bytes[block_size..block_size + 4].copy_from_slice(&valid_size);
        link(&mut hal_bytes, &mut kernel_bytes).unwrap();
        for (header, mapped, base, target) in [
            (hal_header, hal_bytes, hal_base, kernel_base + 0x1030),
            (kernel_header, kernel_bytes, kernel_base, hal_base + 0x1010),
        ] {
            let (imports, _) = read_data_directory_pe32plus(&mapped, header.optional_offset, 1);
            let slot = imports as usize + 16;
            let iat = u32::from_le_bytes(mapped[slot..slot + 4].try_into().unwrap()) as usize;
            assert_eq!(&mapped[iat..iat + 8], &target.to_le_bytes());
            assert_eq!(&mapped[iat + 8..iat + 16], &[0; 8]);
            assert_eq!(&mapped[0x1018..0x1020], &(base + 0x1010).to_le_bytes());
        }
    }
}

#[test]
fn native_pe32_linker_binds_four_byte_thunks() {
    use nt61_loader::loader::{
        load_image_full, parse_headers, read_data_directory_pe32, ImageDatabase,
    };

    let mut provider = sample_dll();
    provider.machine = 0x14c;
    provider.image_base = 0x6000_0000;
    provider.imports.clear();
    let (raw, _) = pe::emit(&provider);
    let mut database = ImageDatabase::new();
    let dll = load_image_full("user32.dll", &raw, &mut database, 0).unwrap();
    database.register(&dll, &raw);
    let mut consumer = provider;
    consumer.exports.clear();
    consumer.image_base = 0x5000_0000;
    consumer.import(
        "user32.dll",
        vec![
            ImportSymbol::ByName("SetLastError".into()),
            ImportSymbol::ByOrdinal(3),
        ],
    );
    let (raw, _) = pe::emit(&consumer);
    let image = load_image_full("consumer.dll", &raw, &mut database, 0).unwrap();
    let (_, _, _, optional) = parse_headers(&image.bytes).unwrap();
    let (imports, _) = read_data_directory_pe32(&image.bytes, optional, 1);
    let slot = imports as usize + 16;
    let iat = u32::from_le_bytes(image.bytes[slot..slot + 4].try_into().unwrap()) as usize;
    assert_eq!(&image.bytes[iat..iat + 4], &0x6000_1020u32.to_le_bytes());
    assert_eq!(
        &image.bytes[iat + 4..iat + 8],
        &0x6000_1030u32.to_le_bytes()
    );
    assert_eq!(&image.bytes[iat + 8..iat + 12], &[0; 4]);
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

#[test]
fn pe32_uses_i386_headers_four_byte_imports_and_highlow_relocations() {
    let mut spec = sample_dll();
    spec.machine = 0x14c;
    spec.image_base = 0x6000_0000;
    let (bytes, _) = pe::emit(&spec);
    let image = pe_read::parse(&bytes).expect("parse PE32");
    assert_eq!(image.magic, 0x10b);
    assert_eq!(image.machine, 0x14c);
    assert_eq!(image.image_base, 0x6000_0000);
    assert_eq!(image.imports.len(), 1);
    assert_eq!(
        image.imports[0].symbols[0].as_deref(),
        Some("NtWaitForSingleObject")
    );
    assert_eq!(image.imports[0].ordinals, vec![7]);
    assert_eq!(image.relocations, vec![0x1018, 0x1028, 0x1038]);
    let section = image.sections.iter().find(|s| s.name == ".reloc").unwrap();
    let at = section.pointer_to_raw_data as usize + 8;
    assert_eq!(
        u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap()) >> 12,
        3
    );
}
