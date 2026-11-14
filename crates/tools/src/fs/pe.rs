//! PE32+ image factory.
//!
//! The repository cannot use an external PE toolchain (`link.exe`,
//! `mingw` and the `x86_64-pc-windows-*` targets are all unavailable),
//! so every Windows image on the disk is produced by this pure-Rust
//! emitter.
//!
//! The emitter grew out of `fs::build::build_pe_image`, which could
//! only produce a single-section `.text` blob with a name-based export
//! table. That is enough for a bare driver but not for a Win32 DLL:
//! a real `user32.dll` needs
//!
//!   * an **import table** so the loader can bind `ntdll!NtUser*`;
//!   * **ordinal exports** and **forwarders**;
//!   * a **`.reloc` section** so the image can be rebased;
//!   * the **GUI subsystem** (`IMAGE_SUBSYSTEM_WINDOWS_GUI` = 2), not
//!     the EFI-application subsystem the driver images use;
//!   * multiple sections so code and read-only data get the right
//!     permissions.
//!
//! # Layout
//!
//! ```text
//! 0x0000  DOS header + PE signature + COFF header
//! 0x0098  Optional header (PE32+, 240 bytes)
//! 0x0188  Section table
//! 0x1000  .text   (code, executable)
//! 0x2000  .rdata  (export directory, import directory, strings)
//! 0x3000  .reloc  (base relocations)
//! 0x4000  .rsrc   (resources, optional)
//! ```
//!
//! # How the directories are positioned
//!
//! The export and import directories must contain absolute RVAs, but
//! they themselves live inside `.rdata`, whose RVA is only known once
//! the sections are laid out. The emitter resolves this by building
//! each directory as a *self-contained blob* whose internal pointers
//! are **blob-relative offsets**, and then adding the blob's final
//! RVA to every pointer in one pass. That keeps the layout
//! deterministic and avoids the "patch twice" fragility that a
//! cursor-based builder invites.

use std::collections::BTreeMap;

// =====================================================================
//  Constants
// =====================================================================

/// `IMAGE_DOS_SIGNATURE`.
pub const IMAGE_DOS_SIGNATURE: u16 = 0x5A4D;
/// `IMAGE_NT_SIGNATURE`.
pub const IMAGE_NT_SIGNATURE: u32 = 0x0000_4550;
/// `IMAGE_FILE_EXECUTABLE_IMAGE`.
pub const IMAGE_FILE_EXECUTABLE_IMAGE: u16 = 0x0002;
/// `IMAGE_FILE_LARGE_ADDRESS_AWARE`.
pub const IMAGE_FILE_LARGE_ADDRESS_AWARE: u16 = 0x0020;
/// `IMAGE_FILE_DLL`.
pub const IMAGE_FILE_DLL: u16 = 0x2000;
/// `IMAGE_FILE_RELOCS_STRIPPED`.
pub const IMAGE_FILE_RELOCS_STRIPPED: u16 = 0x0001;

/// Section alignment used by every image we emit.
pub const SECTION_ALIGNMENT: u32 = 0x1000;
/// File alignment used by every image we emit.
pub const FILE_ALIGNMENT: u32 = 0x200;

/// `IMAGE_SUBSYSTEM_NATIVE` — kernel drivers.
pub const IMAGE_SUBSYSTEM_NATIVE: u16 = 1;
/// `IMAGE_SUBSYSTEM_WINDOWS_GUI` — Win32 GUI applications and DLLs.
pub const IMAGE_SUBSYSTEM_WINDOWS_GUI: u16 = 2;
/// `IMAGE_SUBSYSTEM_WINDOWS_CUI` — console applications.
pub const IMAGE_SUBSYSTEM_WINDOWS_CUI: u16 = 3;
/// `IMAGE_SUBSYSTEM_EFI_APPLICATION` — the value the driver images
/// used before the Win32 work started.
pub const IMAGE_SUBSYSTEM_EFI_APPLICATION: u16 = 0x0A;

/// `IMAGE_DLLCHARACTERISTICS_DYNAMIC_BASE`.
pub const IMAGE_DLLCHARACTERISTICS_DYNAMIC_BASE: u16 = 0x0040;
/// `IMAGE_DLLCHARACTERISTICS_NX_COMPAT`.
pub const IMAGE_DLLCHARACTERISTICS_NX_COMPAT: u16 = 0x0100;
/// `IMAGE_DLLCHARACTERISTICS_TERMINAL_SERVER_AWARE`.
pub const IMAGE_DLLCHARACTERISTICS_TERMINAL_SERVER_AWARE: u16 = 0x8000;

/// `IMAGE_SCN_CNT_CODE`.
pub const IMAGE_SCN_CNT_CODE: u32 = 0x0000_0020;
/// `IMAGE_SCN_CNT_INITIALIZED_DATA`.
pub const IMAGE_SCN_CNT_INITIALIZED_DATA: u32 = 0x0000_0040;
/// `IMAGE_SCN_CNT_UNINITIALIZED_DATA`.
pub const IMAGE_SCN_CNT_UNINITIALIZED_DATA: u32 = 0x0000_0080;
/// `IMAGE_SCN_MEM_DISCARDABLE`.
pub const IMAGE_SCN_MEM_DISCARDABLE: u32 = 0x0200_0000;
/// `IMAGE_SCN_MEM_EXECUTE`.
pub const IMAGE_SCN_MEM_EXECUTE: u32 = 0x2000_0000;
/// `IMAGE_SCN_MEM_READ`.
pub const IMAGE_SCN_MEM_READ: u32 = 0x4000_0000;
/// `IMAGE_SCN_MEM_WRITE`.
pub const IMAGE_SCN_MEM_WRITE: u32 = 0x8000_0000;

// =====================================================================
//  Public description types
// =====================================================================

/// How a symbol is imported from another module.
///
/// The name is a `&'static str` rather than a `String` so the whole
/// import table can be a `const`: the tables in `fs::win32_dlls` are
/// large enough that building them at run time on every build would
/// be wasteful, and `const` guarantees they cannot drift.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImportSymbol {
    /// Import by name.
    ByName(&'static str),
    /// Import by ordinal.
    ByOrdinal(u16),
}

/// One imported module and the symbols taken from it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportModule {
    /// The DLL name as it appears in the import directory.
    pub dll_name: String,
    /// The symbols imported from this DLL.
    pub symbols: Vec<ImportSymbol>,
}

/// One exported symbol.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExportSymbol {
    /// Exported by name.
    ByName {
        /// The exported symbol name.
        name: String,
        /// The 1-based ordinal.
        ordinal: u16,
        /// The RVA of the implementation.
        rva: u32,
    },
    /// Exported by ordinal only. Windows uses this for the "internal"
    /// exports of the C runtime.
    ByOrdinal {
        /// The 1-based ordinal.
        ordinal: u16,
        /// The RVA of the implementation.
        rva: u32,
    },
    /// A forwarder: the RVA points at the string `"DLL.Symbol"` and
    /// the loader resolves the target in the named module.
    Forwarder {
        /// The exported symbol name.
        name: String,
        /// The 1-based ordinal.
        ordinal: u16,
        /// The forwarder target, e.g. `"ntdll.RtlAllocateHeap"`.
        target: String,
    },
}

/// A section of the emitted image.
#[derive(Clone, Debug)]
pub struct SectionSpec {
    /// The section name (at most 8 bytes, NUL padded).
    pub name: String,
    /// The section characteristics flags.
    pub characteristics: u32,
    /// The section payload.
    pub data: Vec<u8>,
}

impl SectionSpec {
    /// A code section.
    pub fn code(data: Vec<u8>) -> Self {
        Self {
            name: ".text".into(),
            characteristics: IMAGE_SCN_CNT_CODE | IMAGE_SCN_MEM_EXECUTE | IMAGE_SCN_MEM_READ,
            data,
        }
    }

    /// A read-only data section.
    pub fn rdata(data: Vec<u8>) -> Self {
        Self {
            name: ".rdata".into(),
            characteristics: IMAGE_SCN_CNT_INITIALIZED_DATA | IMAGE_SCN_MEM_READ,
            data,
        }
    }

    /// A writable data section.
    pub fn data(data: Vec<u8>) -> Self {
        Self {
            name: ".data".into(),
            characteristics: IMAGE_SCN_CNT_INITIALIZED_DATA
                | IMAGE_SCN_MEM_READ
                | IMAGE_SCN_MEM_WRITE,
            data,
        }
    }

    /// A relocation section.
    pub fn reloc(data: Vec<u8>) -> Self {
        Self {
            name: ".reloc".into(),
            characteristics: IMAGE_SCN_CNT_INITIALIZED_DATA
                | IMAGE_SCN_MEM_DISCARDABLE
                | IMAGE_SCN_MEM_READ,
            data,
        }
    }
}

/// The complete description of a PE image to emit.
#[derive(Clone, Debug)]
pub struct PeImageSpec {
    /// Preferred load address.
    pub image_base: u64,
    /// RVA of the entry point.
    pub entry_point_rva: u32,
    /// `IMAGE_SUBSYSTEM_*`.
    pub subsystem: u16,
    /// Whether the image is a DLL.
    pub is_dll: bool,
    /// The PE machine identifier.
    pub machine: u16,
    /// The module name written into the export directory.
    pub module_name: String,
    /// Exported symbols.
    pub exports: Vec<ExportSymbol>,
    /// Imported modules.
    pub imports: Vec<ImportModule>,
    /// Sections, in the order they should appear in the section table.
    pub sections: Vec<SectionSpec>,
    /// RVAs that need a base relocation.
    pub relocations: Vec<u32>,
}

impl PeImageSpec {
    /// Start a new image description.
    pub fn new(image_base: u64, subsystem: u16, is_dll: bool, machine: u16) -> Self {
        Self {
            image_base,
            entry_point_rva: 0,
            subsystem,
            is_dll,
            machine,
            module_name: String::new(),
            exports: Vec::new(),
            imports: Vec::new(),
            sections: Vec::new(),
            relocations: Vec::new(),
        }
    }

    /// Add an export that is resolved by name.
    pub fn export_named(&mut self, name: &str, ordinal: u16, rva: u32) -> &mut Self {
        self.exports.push(ExportSymbol::ByName {
            name: name.to_string(),
            ordinal,
            rva,
        });
        self
    }

    /// Add an export that is resolved by ordinal only.
    pub fn export_ordinal(&mut self, ordinal: u16, rva: u32) -> &mut Self {
        self.exports.push(ExportSymbol::ByOrdinal { ordinal, rva });
        self
    }

    /// Add an export that forwards to another module.
    pub fn export_forwarder(&mut self, name: &str, ordinal: u16, target: &str) -> &mut Self {
        self.exports.push(ExportSymbol::Forwarder {
            name: name.to_string(),
            ordinal,
            target: target.to_string(),
        });
        self
    }

    /// Add an imported module.
    pub fn import(&mut self, dll_name: &str, symbols: Vec<ImportSymbol>) -> &mut Self {
        self.imports.push(ImportModule {
            dll_name: dll_name.to_string(),
            symbols,
        });
        self
    }
}

// =====================================================================
//  Emission
// =====================================================================

/// A section that has been laid out in the image.
struct PlacedSection {
    name: String,
    characteristics: u32,
    virtual_address: u32,
    virtual_size: u32,
    raw_offset: u32,
    raw_size: u32,
    data: Vec<u8>,
}

/// What the emitter did, so the caller can assert on it.
#[derive(Clone, Debug, Default)]
pub struct PeEmitReport {
    /// Number of exported symbols.
    pub export_count: usize,
    /// Number of exports that carry a name.
    pub named_export_count: usize,
    /// Number of forwarder exports.
    pub forwarder_count: usize,
    /// Number of imported modules.
    pub import_module_count: usize,
    /// Number of imported symbols.
    pub import_symbol_count: usize,
    /// Number of base relocations.
    pub relocation_count: usize,
    /// Set when a section payload did not fit in the file.
    pub overflow: bool,
}

/// Emit a PE32+ image from `spec`.
///
/// The function never fails. A malformed description is reported
/// through the returned [`PeEmitReport`].
pub fn emit(spec: &PeImageSpec) -> (Vec<u8>, PeEmitReport) {
    let mut report = PeEmitReport::default();

    // -----------------------------------------------------------------
    //  Phase 1 — build the directory blobs
    // -----------------------------------------------------------------
    // The blobs are built with *blob-relative* pointers, so their
    // size does not depend on where they end up. That lets the
    // section layout be computed exactly, in one pass, before the
    // blobs are relocated.
    let export_blob = if spec.exports.is_empty() {
        ExportBlob {
            blob: Vec::new(),
            forwarder_slots: Vec::new(),
            function_table_len: 0,
            name_count: 0,
        }
    } else {
        build_export_directory(spec)
    };
    let import_blob = if spec.imports.is_empty() {
        Vec::new()
    } else {
        build_import_directory(spec)
    };
    let reloc_blob = if spec.relocations.is_empty() {
        Vec::new()
    } else {
        build_base_relocation_table(&spec.relocations)
    };

    report.export_count = spec.exports.len();
    report.named_export_count = spec
        .exports
        .iter()
        .filter(|e| !matches!(e, ExportSymbol::ByOrdinal { .. }))
        .count();
    report.forwarder_count = spec
        .exports
        .iter()
        .filter(|e| matches!(e, ExportSymbol::Forwarder { .. }))
        .count();
    report.import_module_count = spec.imports.len();
    report.import_symbol_count = spec.imports.iter().map(|m| m.symbols.len()).sum();
    report.relocation_count = spec.relocations.len();

    // -----------------------------------------------------------------
    //  Phase 2 — lay the sections out
    // -----------------------------------------------------------------
    let mut placed: Vec<PlacedSection> = Vec::with_capacity(spec.sections.len());
    let mut next_rva: u32 = SECTION_ALIGNMENT;

    for section in &spec.sections {
        // The `.rdata` section also carries the export and import
        // directories, and `.reloc` carries the relocation table, so
        // their sizes are only known now.
        let mut data = section.data.clone();
        match section.name.as_str() {
            ".rdata" => {
                data.extend_from_slice(&export_blob.blob);
                data.extend_from_slice(&import_blob);
            }
            ".reloc" => {
                data = reloc_blob.clone();
            }
            _ => {}
        }

        let virtual_size = data.len() as u32;
        let raw_size = align_up(virtual_size, FILE_ALIGNMENT);
        placed.push(PlacedSection {
            name: section.name.clone(),
            characteristics: section.characteristics,
            virtual_address: next_rva,
            virtual_size,
            raw_offset: 0,
            raw_size,
            data,
        });
        next_rva = align_up(next_rva + virtual_size.max(1), SECTION_ALIGNMENT);
    }

    let size_of_image = next_rva;

    let rdata_index = spec
        .sections
        .iter()
        .position(|s| s.name == ".rdata")
        .unwrap_or(usize::MAX);
    let rdata_rva = if rdata_index == usize::MAX {
        0
    } else {
        placed[rdata_index].virtual_address
    };
    // Bytes of `.rdata` that were already present before the export
    // and import blobs are appended.
    let rdata_prefix_len = if rdata_index == usize::MAX {
        0
    } else {
        spec.sections[rdata_index].data.len() as u32
    };

    let export_base = rdata_rva + rdata_prefix_len;
    let import_base = export_base + export_blob.blob.len() as u32;

    // -----------------------------------------------------------------
    //  Phase 3 — relocate the directory pointers
    // -----------------------------------------------------------------
    let mut export_dir_rva = 0u32;
    let mut export_dir_size = 0u32;
    if !export_blob.blob.is_empty() {
        export_dir_rva = export_base;
        export_dir_size = export_blob.blob.len() as u32;
        let mut blob = export_blob.blob.clone();
        relocate_export_blob(&mut blob, export_base, &export_blob);
        placed[rdata_index].data[rdata_prefix_len as usize..rdata_prefix_len as usize + blob.len()]
            .copy_from_slice(&blob);
    }

    let mut import_dir_rva = 0u32;
    let mut import_dir_size = 0u32;
    if !import_blob.is_empty() {
        import_dir_rva = import_base;
        import_dir_size = import_blob.len() as u32;
        let mut blob = import_blob;
        relocate_import_blob(&mut blob, import_base);
        let at = (rdata_prefix_len + export_blob.blob.len() as u32) as usize;
        placed[rdata_index].data[at..at + blob.len()].copy_from_slice(&blob);
    }

    let mut reloc_rva = 0u32;
    let mut reloc_size = 0u32;
    if !reloc_blob.is_empty() {
        let index = spec
            .sections
            .iter()
            .position(|s| s.name == ".reloc")
            .unwrap_or(usize::MAX);
        if index != usize::MAX {
            reloc_rva = placed[index].virtual_address;
            reloc_size = reloc_blob.len() as u32;
        }
    }

    // -----------------------------------------------------------------
    //  Phase 4 — file layout
    // -----------------------------------------------------------------
    let headers_size: u32 = 0x400;
    let mut file_cursor = headers_size;
    for section in placed.iter_mut() {
        if section.raw_size == 0 {
            section.raw_offset = file_cursor;
            continue;
        }
        file_cursor = align_up(file_cursor, FILE_ALIGNMENT);
        section.raw_offset = file_cursor;
        file_cursor += section.raw_size;
    }
    let total_file_size = file_cursor;

    let mut image = vec![0u8; total_file_size as usize];

    // -----------------------------------------------------------------
    //  DOS header
    // -----------------------------------------------------------------
    image[0x00..0x02].copy_from_slice(&IMAGE_DOS_SIGNATURE.to_le_bytes());
    image[0x3C..0x40].copy_from_slice(&0x80u32.to_le_bytes());
    // A minimal DOS stub: the loader only checks the magic and
    // `e_lfanew`, but `file` and `objdump` expect a stub program.
    image[0x40..0x4E].copy_from_slice(b"\x0e\x1f\xba\x0e\x00\xb4\x09\xcd\x21\xb8\x01\x4c\xcd\x21");
    image[0x80..0x84].copy_from_slice(&IMAGE_NT_SIGNATURE.to_le_bytes());

    // -----------------------------------------------------------------
    //  COFF header
    // -----------------------------------------------------------------
    let coff = 0x84usize;
    image[coff..coff + 2].copy_from_slice(&spec.machine.to_le_bytes());
    image[coff + 2..coff + 4].copy_from_slice(&(placed.len() as u16).to_le_bytes());
    image[coff + 4..coff + 8].copy_from_slice(&0x2024_1118u32.to_le_bytes());
    image[coff + 16..coff + 18].copy_from_slice(&240u16.to_le_bytes());
    let mut characteristics = IMAGE_FILE_EXECUTABLE_IMAGE | IMAGE_FILE_LARGE_ADDRESS_AWARE;
    if spec.is_dll {
        characteristics |= IMAGE_FILE_DLL;
    } else {
        characteristics |= IMAGE_FILE_RELOCS_STRIPPED;
    }
    image[coff + 18..coff + 20].copy_from_slice(&characteristics.to_le_bytes());

    // -----------------------------------------------------------------
    //  Optional header (PE32+)
    // -----------------------------------------------------------------
    let opt = 0x98usize;
    image[opt..opt + 2].copy_from_slice(&0x020Bu16.to_le_bytes()); // PE32+
    image[opt + 2..opt + 3].copy_from_slice(&[14u8]); // linker major
    image[opt + 3..opt + 4].copy_from_slice(&[0u8]); // linker minor
    let size_of_code: u32 = placed
        .iter()
        .filter(|s| s.characteristics & IMAGE_SCN_CNT_CODE != 0)
        .map(|s| s.raw_size)
        .sum();
    let size_of_init_data: u32 = placed
        .iter()
        .filter(|s| s.characteristics & IMAGE_SCN_CNT_INITIALIZED_DATA != 0)
        .map(|s| s.raw_size)
        .sum();
    image[opt + 4..opt + 8].copy_from_slice(&size_of_code.to_le_bytes());
    image[opt + 8..opt + 12].copy_from_slice(&size_of_init_data.to_le_bytes());
    image[opt + 16..opt + 20].copy_from_slice(&spec.entry_point_rva.to_le_bytes());
    image[opt + 20..opt + 24].copy_from_slice(&SECTION_ALIGNMENT.to_le_bytes());
    image[opt + 24..opt + 32].copy_from_slice(&spec.image_base.to_le_bytes());
    image[opt + 32..opt + 36].copy_from_slice(&SECTION_ALIGNMENT.to_le_bytes());
    image[opt + 36..opt + 40].copy_from_slice(&FILE_ALIGNMENT.to_le_bytes());
    // Windows 7 SP1 version numbers.
    image[opt + 40..opt + 42].copy_from_slice(&6u16.to_le_bytes()); // OS major
    image[opt + 42..opt + 44].copy_from_slice(&1u16.to_le_bytes()); // OS minor
    image[opt + 48..opt + 50].copy_from_slice(&6u16.to_le_bytes()); // subsystem major
    image[opt + 50..opt + 52].copy_from_slice(&1u16.to_le_bytes()); // subsystem minor
    image[opt + 56..opt + 60].copy_from_slice(&size_of_image.to_le_bytes());
    image[opt + 60..opt + 64].copy_from_slice(&headers_size.to_le_bytes());
    image[opt + 68..opt + 70].copy_from_slice(&spec.subsystem.to_le_bytes());
    let dll_characteristics = if spec.relocations.is_empty() {
        IMAGE_DLLCHARACTERISTICS_NX_COMPAT | IMAGE_DLLCHARACTERISTICS_TERMINAL_SERVER_AWARE
    } else {
        IMAGE_DLLCHARACTERISTICS_DYNAMIC_BASE
            | IMAGE_DLLCHARACTERISTICS_NX_COMPAT
            | IMAGE_DLLCHARACTERISTICS_TERMINAL_SERVER_AWARE
    };
    image[opt + 70..opt + 72].copy_from_slice(&dll_characteristics.to_le_bytes());
    image[opt + 72..opt + 80].copy_from_slice(&0x0010_0000u64.to_le_bytes());
    image[opt + 80..opt + 88].copy_from_slice(&0x1000u64.to_le_bytes());
    image[opt + 88..opt + 96].copy_from_slice(&0x0010_0000u64.to_le_bytes());
    image[opt + 96..opt + 104].copy_from_slice(&0x1000u64.to_le_bytes());
    image[opt + 108..opt + 112].copy_from_slice(&16u32.to_le_bytes());

    // Data directories (16 entries, starting at opt + 112).
    let dd = opt + 112;
    let mut set_dir = |index: usize, rva: u32, size: u32| {
        if rva == 0 && size == 0 {
            return;
        }
        let at = dd + index * 8;
        image[at..at + 4].copy_from_slice(&rva.to_le_bytes());
        image[at + 4..at + 8].copy_from_slice(&size.to_le_bytes());
    };
    set_dir(0, export_dir_rva, export_dir_size); // export
    set_dir(1, import_dir_rva, import_dir_size); // import
    set_dir(5, reloc_rva, reloc_size); // base relocation

    // -----------------------------------------------------------------
    //  Section table
    // -----------------------------------------------------------------
    let sect_table = opt + 240;
    for (index, section) in placed.iter().enumerate() {
        let at = sect_table + index * 40;
        let mut name_bytes = [0u8; 8];
        let name_src = section.name.as_bytes();
        let copy = name_src.len().min(8);
        name_bytes[..copy].copy_from_slice(&name_src[..copy]);
        image[at..at + 8].copy_from_slice(&name_bytes);
        image[at + 8..at + 12].copy_from_slice(&section.virtual_size.to_le_bytes());
        image[at + 12..at + 16].copy_from_slice(&section.virtual_address.to_le_bytes());
        image[at + 16..at + 20].copy_from_slice(&section.raw_size.to_le_bytes());
        image[at + 20..at + 24].copy_from_slice(&section.raw_offset.to_le_bytes());
        image[at + 36..at + 40].copy_from_slice(&section.characteristics.to_le_bytes());
    }

    // -----------------------------------------------------------------
    //  Section payloads
    // -----------------------------------------------------------------
    for section in &placed {
        if section.raw_size == 0 || section.data.is_empty() {
            continue;
        }
        let at = section.raw_offset as usize;
        let end = at + section.data.len();
        if end > image.len() {
            report.overflow = true;
            continue;
        }
        image[at..end].copy_from_slice(&section.data);
    }

    (image, report)
}

/// `IMAGE_ORDINAL_FLAG64` — in a PE32+ import table, an entry with
/// this bit set is an **ordinal import**; a clear bit means the entry
/// is a hint/name RVA.
///
/// This is the exact inverse of the PE32 convention, where the flag
/// lives in bit 31 of a 32-bit entry and means "import by ordinal".
/// The 64-bit flag was moved to bit 63 so that the 32-bit
/// `IMAGE_ORDINAL_FLAG` and the 64-bit `IMAGE_ORDINAL_FLAG64` could
/// coexist in the same slot with different meanings.
pub const IMAGE_ORDINAL_FLAG64: u64 = 1u64 << 63;

/// `IMAGE_ORDINAL_FLAG32` — the PE32 ordinal flag, kept for
/// reference when reading 32-bit images.
pub const IMAGE_ORDINAL_FLAG32: u32 = 0x8000_0000;

/// Round `value` up to the next multiple of `alignment`.
pub fn align_up(value: u32, alignment: u32) -> u32 {
    if alignment == 0 {
        return value;
    }
    value.div_ceil(alignment) * alignment
}

/// Build the export directory blob.
///
/// The blob is self-contained: every pointer inside it is a
/// **blob-relative offset**. [`relocate_export_blob`] turns those into
/// absolute RVAs once the blob's final RVA is known.
///
/// The returned [`ExportBlob`] also records which slots in the
/// address-of-functions table hold forwarders, so the relocation pass
/// knows exactly which entries are pointers and which are code.
fn build_export_directory(spec: &PeImageSpec) -> ExportBlob {
    // Collect the exports in ordinal order. The ordinal is the index
    // into the address-of-functions table.
    let mut slots: Vec<Slot> = Vec::new();
    for export in &spec.exports {
        match export {
            ExportSymbol::ByName { name, ordinal, rva } => slots.push(Slot {
                ordinal: *ordinal,
                name: Some(name.clone()),
                kind: SlotKind::Code(*rva),
            }),
            ExportSymbol::ByOrdinal { ordinal, rva } => slots.push(Slot {
                ordinal: *ordinal,
                name: None,
                kind: SlotKind::Code(*rva),
            }),
            ExportSymbol::Forwarder {
                name,
                ordinal,
                target,
            } => slots.push(Slot {
                ordinal: *ordinal,
                name: Some(name.clone()),
                kind: SlotKind::Forwarder(target.clone()),
            }),
        }
    }
    slots.sort_by_key(|entry| entry.ordinal);

    // The named exports, in the same order.
    let named_indices: Vec<usize> = slots
        .iter()
        .enumerate()
        .filter_map(|(index, slot)| slot.name.as_ref().map(|_| index))
        .collect();

    // Layout of the blob:
    //   0x00  IMAGE_EXPORT_DIRECTORY (40 bytes)
    //   0x28  AddressOfFunctions   (4 * slot_count)
    //         AddressOfNames        (4 * named_count)
    //         AddressOfNameOrdinals (2 * named_count)
    //         forwarder strings
    //         module name string
    //         export name strings
    const DIRECTORY_SIZE: usize = 40;
    let functions_off = DIRECTORY_SIZE;
    let names_off = functions_off + slots.len() * 4;
    let ordinals_off = names_off + named_indices.len() * 4;
    let strings_off = ordinals_off + named_indices.len() * 2;

    let mut blob: Vec<u8> = vec![0; strings_off];

    // Helper that appends a NUL-terminated ASCII string and returns
    // its blob-relative offset.
    fn put_cstr(blob: &mut Vec<u8>, value: &str) -> u32 {
        let offset = blob.len() as u32;
        blob.extend_from_slice(value.as_bytes());
        blob.push(0);
        offset
    }

    // The forwarder strings must live *inside* the export directory
    // range; that is what tells the loader the slot is a forwarder.
    let mut forwarder_offsets: BTreeMap<usize, u32> = BTreeMap::new();
    for (index, slot) in slots.iter().enumerate() {
        if let SlotKind::Forwarder(target) = &slot.kind {
            forwarder_offsets.insert(index, put_cstr(&mut blob, target));
        }
    }

    let module_name_off = put_cstr(&mut blob, &spec.module_name);

    let mut name_offsets: Vec<u32> = Vec::with_capacity(named_indices.len());
    for index in &named_indices {
        let name = slots[*index].name.as_deref().unwrap_or_default();
        name_offsets.push(put_cstr(&mut blob, name));
    }

    // Address of functions.
    for (index, slot) in slots.iter().enumerate() {
        let value = match &slot.kind {
            SlotKind::Code(rva) => *rva,
            SlotKind::Forwarder(_) => forwarder_offsets.get(&index).copied().unwrap_or(0),
        };
        let at = functions_off + index * 4;
        blob[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    // Address of names.
    for (index, offset) in name_offsets.iter().enumerate() {
        let at = names_off + index * 4;
        blob[at..at + 4].copy_from_slice(&offset.to_le_bytes());
    }
    // Address of name ordinals: the slot index for each named export.
    for (index, slot_index) in named_indices.iter().enumerate() {
        let at = ordinals_off + index * 2;
        blob[at..at + 2].copy_from_slice(&(*slot_index as u16).to_le_bytes());
    }

    // The directory header.
    let mut header = [0u8; DIRECTORY_SIZE];
    header[12..16].copy_from_slice(&module_name_off.to_le_bytes());
    header[16..20].copy_from_slice(&1u32.to_le_bytes()); // ordinal base
    header[20..24].copy_from_slice(&(slots.len() as u32).to_le_bytes());
    header[24..28].copy_from_slice(&(named_indices.len() as u32).to_le_bytes());
    header[28..32].copy_from_slice(&(functions_off as u32).to_le_bytes());
    header[32..36].copy_from_slice(&(names_off as u32).to_le_bytes());
    header[36..40].copy_from_slice(&(ordinals_off as u32).to_le_bytes());
    blob[0..DIRECTORY_SIZE].copy_from_slice(&header);

    ExportBlob {
        blob,
        forwarder_slots: forwarder_offsets.keys().copied().collect(),
        function_table_len: slots.len(),
        name_count: named_indices.len(),
    }
}

/// One export slot in the address-of-functions table.
struct Slot {
    /// The 1-based ordinal.
    ordinal: u16,
    /// The exported name, if the export is named.
    name: Option<String>,
    /// What the slot points at.
    kind: SlotKind,
}

/// The export directory blob plus the metadata the relocation pass
/// needs.
struct ExportBlob {
    /// The raw bytes.
    blob: Vec<u8>,
    /// The indices in the address-of-functions table that hold
    /// forwarders.
    forwarder_slots: Vec<usize>,
    /// The number of entries in the address-of-functions table.
    function_table_len: usize,
    /// The number of named exports.
    name_count: usize,
}

/// The kind of an export slot.
enum SlotKind {
    /// A direct code pointer.
    Code(u32),
    /// A forwarder to another module.
    Forwarder(String),
}

/// Turn the blob-relative offsets inside an export directory blob
/// into absolute RVAs.
///
/// The blob layout is fixed, so the pointers that need fixing are
/// known statically:
///
/// ```text
/// +0x0C  Name                     -> module name string
/// +0x1C  AddressOfFunctions       -> 4 * slot_count
/// +0x20  AddressOfNames           -> 4 * named_count
/// +0x24  AddressOfNameOrdinals    -> 2 * named_count
/// ```
///
/// plus every entry of the three tables.
fn relocate_export_blob(blob: &mut [u8], base_rva: u32, meta: &ExportBlob) {
    fn read_u32(blob: &[u8], at: usize) -> u32 {
        u32::from_le_bytes([blob[at], blob[at + 1], blob[at + 2], blob[at + 3]])
    }
    fn write_u32(blob: &mut [u8], at: usize, value: u32) {
        blob[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }

    if blob.len() < 40 {
        return;
    }

    // The directory header pointers.
    write_u32(blob, 12, base_rva + read_u32(blob, 12));
    let functions_at = read_u32(blob, 28) as usize;
    write_u32(blob, 28, base_rva + read_u32(blob, 28));
    let names_at = read_u32(blob, 32) as usize;
    write_u32(blob, 32, base_rva + read_u32(blob, 32));
    let ordinals_at = read_u32(blob, 36) as usize;
    write_u32(blob, 36, base_rva + read_u32(blob, 36));

    // The address-of-names table holds string pointers.
    for index in 0..meta.name_count {
        let at = names_at + index * 4;
        if at + 4 > blob.len() {
            break;
        }
        write_u32(blob, at, base_rva + read_u32(blob, at));
    }

    // The address-of-functions table holds *code* RVAs, which are
    // already absolute and must not be shifted — except for the
    // forwarder slots, which point at a string inside the export
    // directory and therefore are blob-relative.
    for index in &meta.forwarder_slots {
        let at = functions_at + index * 4;
        if at + 4 > blob.len() {
            continue;
        }
        write_u32(blob, at, base_rva + read_u32(blob, at));
    }

    // The address-of-name-ordinals table holds 16-bit slot indices,
    // not pointers, so it needs no fixup. `ordinals_at` is read only
    // to keep the layout documentation honest.
    let _ = ordinals_at;
    let _ = meta.function_table_len;
}

/// Turn the blob-relative offsets inside an import directory blob
/// into absolute RVAs.
///
/// The blob layout is:
///
/// ```text
/// descriptor[i] +0x00  OriginalFirstThunk (ILT)   -> blob offset
///               +0x0C  Name                       -> blob offset
///               +0x10  FirstThunk (IAT)           -> blob offset
/// ```
///
/// plus every ILT and IAT entry that carries a hint/name RVA.
fn relocate_import_blob(blob: &mut [u8], base_rva: u32) {
    fn read_u32(blob: &[u8], at: usize) -> u32 {
        u32::from_le_bytes([blob[at], blob[at + 1], blob[at + 2], blob[at + 3]])
    }
    fn write_u32(blob: &mut [u8], at: usize, value: u32) {
        blob[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    fn read_u64(blob: &[u8], at: usize) -> u64 {
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&blob[at..at + 8]);
        u64::from_le_bytes(bytes)
    }
    fn write_u64(blob: &mut [u8], at: usize, value: u64) {
        blob[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }

    let descriptor_size = 20usize;
    let mut at = 0usize;
    while at + descriptor_size <= blob.len() {
        let ilt = read_u32(blob, at);
        let name = read_u32(blob, at + 12);
        let iat = read_u32(blob, at + 16);
        if ilt == 0 && name == 0 && iat == 0 {
            break;
        }
        write_u32(blob, at, base_rva + ilt);
        write_u32(blob, at + 12, base_rva + name);
        write_u32(blob, at + 16, base_rva + iat);

        // The ILT and the IAT hold the same values.
        for table in [ilt as usize, iat as usize] {
            let mut entry = table;
            while entry + 8 <= blob.len() {
                let value = read_u64(blob, entry);
                if value == 0 {
                    break;
                }
                // `IMAGE_ORDINAL_FLAG64` **clear** => hint/name RVA.
                if value & IMAGE_ORDINAL_FLAG64 == 0 {
                    write_u64(blob, entry, u64::from(base_rva) + (value & 0x7FFF_FFFF));
                }
                entry += 8;
            }
        }
        at += descriptor_size;
    }
}

/// Build the import directory blob.
///
/// As with the export directory, all internal pointers are
/// blob-relative offsets.
fn build_import_directory(spec: &PeImageSpec) -> Vec<u8> {
    // Layout:
    //   descriptors: 20 * (module_count + 1)
    //   then per module: ILT, IAT, hint/name entries, DLL name
    let descriptor_count = spec.imports.len() + 1;
    let mut blob: Vec<u8> = vec![0u8; descriptor_count * 20];

    fn put_cstr(blob: &mut Vec<u8>, value: &str) -> u32 {
        let offset = blob.len() as u32;
        blob.extend_from_slice(value.as_bytes());
        blob.push(0);
        offset
    }

    for (module_index, module) in spec.imports.iter().enumerate() {
        // The hint/name entries (or the ordinal markers).
        let mut entries: Vec<u32> = Vec::with_capacity(module.symbols.len());
        for symbol in &module.symbols {
            match symbol {
                ImportSymbol::ByName(name) => {
                    let at = blob.len() as u32;
                    blob.extend_from_slice(&0u16.to_le_bytes()); // hint
                    blob.extend_from_slice(name.as_bytes());
                    blob.push(0);
                    entries.push(at);
                }
                ImportSymbol::ByOrdinal(_) => {
                    // Ordinal imports put the ordinal in the ILT/IAT
                    // entry, so no hint/name blob is needed.
                    entries.push(u32::MAX);
                }
            }
        }

        // The ILT and the IAT.
        let ilt_off = blob.len() as u32;
        blob.resize(blob.len() + (module.symbols.len() + 1) * 8, 0);
        let iat_off = blob.len() as u32;
        blob.resize(blob.len() + (module.symbols.len() + 1) * 8, 0);

        // The DLL name.
        let name_off = put_cstr(&mut blob, &module.dll_name);

        // Fill the tables.
        for (symbol_index, symbol) in module.symbols.iter().enumerate() {
            let value = match symbol {
                // Import by name: the entry is a hint/name RVA with
                // `IMAGE_ORDINAL_FLAG64` **clear**.
                ImportSymbol::ByName(_) => u64::from(entries[symbol_index]),
                // Import by ordinal: the low 16 bits carry the
                // ordinal and `IMAGE_ORDINAL_FLAG64` is **set**.
                ImportSymbol::ByOrdinal(ordinal) => IMAGE_ORDINAL_FLAG64 | u64::from(*ordinal),
            };
            let ilt_at = ilt_off as usize + symbol_index * 8;
            blob[ilt_at..ilt_at + 8].copy_from_slice(&value.to_le_bytes());
            let iat_at = iat_off as usize + symbol_index * 8;
            blob[iat_at..iat_at + 8].copy_from_slice(&value.to_le_bytes());
        }

        // The descriptor.
        let at = module_index * 20;
        blob[at..at + 4].copy_from_slice(&ilt_off.to_le_bytes());
        blob[at + 4..at + 8].copy_from_slice(&0u32.to_le_bytes()); // TimeDateStamp
        blob[at + 8..at + 12].copy_from_slice(&0u32.to_le_bytes()); // ForwarderChain
        blob[at + 12..at + 16].copy_from_slice(&name_off.to_le_bytes());
        blob[at + 16..at + 20].copy_from_slice(&iat_off.to_le_bytes());
    }

    blob
}

/// Build a PE/COFF base relocation table from a list of RVAs.
pub fn build_base_relocation_table(relocations: &[u32]) -> Vec<u8> {
    if relocations.is_empty() {
        // A header-only block marks an image as relocatable without
        // asking the loader to patch any word.
        return vec![0, 0, 0, 0, 8, 0, 0, 0];
    }

    let mut sorted: Vec<u32> = relocations.to_vec();
    sorted.sort_unstable();
    sorted.dedup();

    let mut body = Vec::new();
    let mut page: u32 = u32::MAX;
    let mut entries: Vec<u16> = Vec::new();

    fn flush(body: &mut Vec<u8>, page: u32, entries: &mut Vec<u16>) {
        if entries.is_empty() {
            return;
        }
        // Absolute entries are legal padding and keep every block
        // 4-byte aligned.
        if entries.len() % 2 != 0 {
            entries.push(0);
        }
        let block_size = 8u32 + entries.len() as u32 * 2;
        body.extend_from_slice(&page.to_le_bytes());
        body.extend_from_slice(&block_size.to_le_bytes());
        for entry in entries.iter() {
            body.extend_from_slice(&entry.to_le_bytes());
        }
        entries.clear();
    }

    for rva in &sorted {
        if *rva & 0xFFFF_F000 != page {
            flush(&mut body, page, &mut entries);
            page = *rva & 0xFFFF_F000;
        }
        entries.push((0xA000 | (*rva & 0xFFF)) as u16);
    }
    flush(&mut body, page, &mut entries);
    body
}
