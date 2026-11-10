//! A minimal PE32+ reader used by the regression tests.
//!
//! The emitter in `fs::pe` and this reader are deliberately
//! independent implementations: the reader follows the PE/COFF spec
//! directly, so a mistake in the emitter cannot hide behind a shared
//! helper. The reader is test-only, but it lives in the library so
//! the build tools can also use it to inspect images they emit.

use std::collections::BTreeMap;

/// A parsed PE32+ image.
#[derive(Clone, Debug)]
pub struct PeImage {
    /// The optional header magic (`0x20B` for PE32+).
    pub magic: u16,
    /// The COFF machine identifier.
    pub machine: u16,
    /// The number of sections.
    pub number_of_sections: u16,
    /// The COFF characteristics.
    pub characteristics: u16,
    /// The preferred load address.
    pub image_base: u64,
    /// The RVA of the entry point.
    pub entry_point: u32,
    /// The total size of the mapped image.
    pub size_of_image: u32,
    /// The size of the headers.
    pub size_of_headers: u32,
    /// The subsystem.
    pub subsystem: u16,
    /// The DLL characteristics.
    pub dll_characteristics: u16,
    /// The data directories.
    pub data_directories: Vec<(u32, u32)>,
    /// The section table.
    pub sections: Vec<PeSection>,
    /// The parsed exports, keyed by name.
    pub exports_by_name: BTreeMap<String, u32>,
    /// The parsed exports, keyed by ordinal.
    pub exports_by_ordinal: BTreeMap<u16, u32>,
    /// The forwarder targets, keyed by name.
    pub forwarders: BTreeMap<String, String>,
    /// The import modules and their symbols.
    pub imports: Vec<PeImport>,
    /// The base relocation entries, as RVAs.
    pub relocations: Vec<u32>,
}

/// One section of a parsed image.
#[derive(Clone, Debug)]
pub struct PeSection {
    /// The section name.
    pub name: String,
    /// The virtual size.
    pub virtual_size: u32,
    /// The RVA.
    pub virtual_address: u32,
    /// The raw size on disk.
    pub size_of_raw_data: u32,
    /// The file offset of the raw data.
    pub pointer_to_raw_data: u32,
    /// The characteristics.
    pub characteristics: u32,
}

/// One imported module.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeImport {
    /// The DLL name.
    pub dll_name: String,
    /// The imported symbols. `None` means "by ordinal".
    pub symbols: Vec<Option<String>>,
    /// The ordinals, for the ordinal-only imports.
    pub ordinals: Vec<u16>,
}

/// Parse a PE32+ image.
pub fn parse(bytes: &[u8]) -> Option<PeImage> {
    if bytes.len() < 0x40 || &bytes[0..2] != b"MZ" {
        return None;
    }
    let e_lfanew =
        u32::from_le_bytes([bytes[0x3C], bytes[0x3D], bytes[0x3E], bytes[0x3F]]) as usize;
    if e_lfanew + 24 > bytes.len() {
        return None;
    }
    if &bytes[e_lfanew..e_lfanew + 4] != b"PE\0\0" {
        return None;
    }

    let coff = e_lfanew + 4;
    let machine = u16::from_le_bytes([bytes[coff], bytes[coff + 1]]);
    let number_of_sections = u16::from_le_bytes([bytes[coff + 2], bytes[coff + 3]]);
    let characteristics = u16::from_le_bytes([bytes[coff + 18], bytes[coff + 19]]);
    let size_of_optional_header = u16::from_le_bytes([bytes[coff + 16], bytes[coff + 17]]) as usize;

    let opt = coff + 20;
    if opt + size_of_optional_header > bytes.len() {
        return None;
    }
    let magic = u16::from_le_bytes([bytes[opt], bytes[opt + 1]]);
    if magic != 0x20B {
        return None;
    }

    let entry_point = read_u32(bytes, opt + 0x10);
    let image_base = read_u64(bytes, opt + 0x18);
    let size_of_image = read_u32(bytes, opt + 0x38);
    let size_of_headers = read_u32(bytes, opt + 0x3C);
    let subsystem = u16::from_le_bytes([bytes[opt + 0x44], bytes[opt + 0x45]]);
    let dll_characteristics = u16::from_le_bytes([bytes[opt + 0x46], bytes[opt + 0x47]]);
    let number_of_rva_and_sizes = read_u32(bytes, opt + 0x6C) as usize;

    let mut data_directories = Vec::with_capacity(number_of_rva_and_sizes);
    for index in 0..number_of_rva_and_sizes {
        let at = opt + 0x70 + index * 8;
        if at + 8 > bytes.len() {
            break;
        }
        data_directories.push((read_u32(bytes, at), read_u32(bytes, at + 4)));
    }

    let sect_table = opt + size_of_optional_header;
    let mut sections = Vec::with_capacity(number_of_sections as usize);
    for index in 0..number_of_sections as usize {
        let at = sect_table + index * 40;
        if at + 40 > bytes.len() {
            break;
        }
        let name_bytes = &bytes[at..at + 8];
        let name_end = name_bytes.iter().position(|b| *b == 0).unwrap_or(8);
        sections.push(PeSection {
            name: String::from_utf8_lossy(&name_bytes[..name_end]).to_string(),
            virtual_size: read_u32(bytes, at + 8),
            virtual_address: read_u32(bytes, at + 12),
            size_of_raw_data: read_u32(bytes, at + 16),
            pointer_to_raw_data: read_u32(bytes, at + 20),
            characteristics: read_u32(bytes, at + 36),
        });
    }

    // Resolve an RVA to a file offset using the section table.
    let rva_to_file = |rva: u32| -> Option<usize> {
        for section in &sections {
            let start = section.virtual_address;
            let end = start + section.size_of_raw_data.max(section.virtual_size);
            if rva >= start && rva < end {
                let offset = section.pointer_to_raw_data as usize + (rva - start) as usize;
                if offset < bytes.len() {
                    return Some(offset);
                }
            }
        }
        // The headers live at RVA 0.
        if (rva as usize) < bytes.len() {
            return Some(rva as usize);
        }
        None
    };

    let read_cstr = |rva: u32| -> Option<String> {
        let offset = rva_to_file(rva)?;
        let end = bytes[offset..].iter().position(|b| *b == 0)?;
        Some(String::from_utf8_lossy(&bytes[offset..offset + end]).to_string())
    };

    // -----------------------------------------------------------------
    //  Exports
    // -----------------------------------------------------------------
    let mut exports_by_name = BTreeMap::new();
    let mut exports_by_ordinal = BTreeMap::new();
    let mut forwarders = BTreeMap::new();

    if let Some(&(export_rva, export_size)) = data_directories.first() {
        if export_rva != 0 && export_size != 0 {
            if let Some(dir) = rva_to_file(export_rva) {
                let number_of_functions = read_u32(bytes, dir + 0x14);
                let number_of_names = read_u32(bytes, dir + 0x18);
                let functions_rva = read_u32(bytes, dir + 0x1C);
                let names_rva = read_u32(bytes, dir + 0x20);
                let ordinals_rva = read_u32(bytes, dir + 0x24);
                let ordinal_base = read_u32(bytes, dir + 0x10);

                // Every function slot, indexed by ordinal.
                for index in 0..number_of_functions as usize {
                    let at = match rva_to_file(functions_rva + index as u32 * 4) {
                        Some(at) => at,
                        None => break,
                    };
                    let rva = read_u32(bytes, at);
                    let ordinal = (ordinal_base as usize + index) as u16;
                    exports_by_ordinal.insert(ordinal, rva);
                }

                // The named exports.
                for index in 0..number_of_names as usize {
                    let name_at = match rva_to_file(names_rva + index as u32 * 4) {
                        Some(at) => at,
                        None => break,
                    };
                    let name_rva = read_u32(bytes, name_at);
                    let ordinal_at = match rva_to_file(ordinals_rva + index as u32 * 2) {
                        Some(at) => at,
                        None => break,
                    };
                    let slot = u16::from_le_bytes([bytes[ordinal_at], bytes[ordinal_at + 1]]);
                    let Some(name) = read_cstr(name_rva) else {
                        continue;
                    };
                    let Some(&rva) = exports_by_ordinal.get(&(ordinal_base as u16 + slot)) else {
                        continue;
                    };
                    exports_by_name.insert(name.clone(), rva);
                    // A forwarder points inside the export directory.
                    if (rva as usize) >= export_rva as usize
                        && (rva as usize) < (export_rva + export_size) as usize
                    {
                        if let Some(target) = read_cstr(rva) {
                            forwarders.insert(name, target);
                        }
                    }
                }
            }
        }
    }

    // -----------------------------------------------------------------
    //  Imports
    // -----------------------------------------------------------------
    let mut imports = Vec::new();
    if let Some(&(import_rva, import_size)) = data_directories.get(1) {
        if import_rva != 0 && import_size != 0 {
            let mut at = rva_to_file(import_rva);
            while let Some(offset) = at {
                if offset + 20 > bytes.len() {
                    break;
                }
                let ilt_rva = read_u32(bytes, offset);
                let name_rva = read_u32(bytes, offset + 12);
                let iat_rva = read_u32(bytes, offset + 16);
                if ilt_rva == 0 && name_rva == 0 && iat_rva == 0 {
                    break;
                }
                let dll_name = read_cstr(name_rva).unwrap_or_default();

                let mut symbols = Vec::new();
                let mut ordinals = Vec::new();
                let table = if ilt_rva != 0 { ilt_rva } else { iat_rva };
                let mut entry_index = 0u32;
                loop {
                    let entry = rva_to_file(table + entry_index * 8);
                    let Some(entry_offset) = entry else {
                        break;
                    };
                    if entry_offset + 8 > bytes.len() {
                        break;
                    }
                    let value = read_u64(bytes, entry_offset);
                    if value == 0 {
                        break;
                    }
                    if value & (1u64 << 63) == 0 {
                        let rva = (value & 0x7FFF_FFFF) as u32;
                        symbols.push(read_cstr(rva + 2));
                    } else {
                        symbols.push(None);
                        ordinals.push((value & 0xFFFF) as u16);
                    }
                    entry_index += 1;
                }
                imports.push(PeImport {
                    dll_name,
                    symbols,
                    ordinals,
                });
                at = rva_to_file(import_rva + imports.len() as u32 * 20);
            }
        }
    }

    // -----------------------------------------------------------------
    //  Base relocations
    // -----------------------------------------------------------------
    let mut relocations = Vec::new();
    if let Some(&(reloc_rva, reloc_size)) = data_directories.get(5) {
        if reloc_rva != 0 && reloc_size != 0 {
            if let Some(start) = rva_to_file(reloc_rva) {
                let end = start + reloc_size as usize;
                let mut at = start;
                while at + 8 <= end.min(bytes.len()) {
                    let page = read_u32(bytes, at);
                    let block_size = read_u32(bytes, at + 4) as usize;
                    if block_size < 8 {
                        break;
                    }
                    let entries = (block_size - 8) / 2;
                    for index in 0..entries {
                        let entry_at = at + 8 + index * 2;
                        if entry_at + 2 > bytes.len() {
                            break;
                        }
                        let value = u16::from_le_bytes([bytes[entry_at], bytes[entry_at + 1]]);
                        let kind = value >> 12;
                        if kind == 0 {
                            continue; // IMAGE_REL_BASED_ABSOLUTE
                        }
                        relocations.push(page | u32::from(value & 0x0FFF));
                    }
                    at += block_size;
                }
            }
        }
    }

    Some(PeImage {
        magic,
        machine,
        number_of_sections,
        characteristics,
        image_base,
        entry_point,
        size_of_image,
        size_of_headers,
        subsystem,
        dll_characteristics,
        data_directories,
        sections,
        exports_by_name,
        exports_by_ordinal,
        forwarders,
        imports,
        relocations,
    })
}

fn read_u32(bytes: &[u8], at: usize) -> u32 {
    if at + 4 > bytes.len() {
        return 0;
    }
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

fn read_u64(bytes: &[u8], at: usize) -> u64 {
    if at + 8 > bytes.len() {
        return 0;
    }
    let mut buf = [0u8; 8];
    buf.copy_from_slice(&bytes[at..at + 8]);
    u64::from_le_bytes(buf)
}
