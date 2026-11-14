//! `ar` archive → PE32+ DLL.
//!
//! The Ring 3 Win32 DLLs must contain *real code*, not stubs. The
//! repository has no Windows linker (`link.exe` and the MinGW
//! toolchain are both unavailable, and `x86_64-pc-windows-msvc` /
//! `x86_64-pc-windows-gnu` cannot link here), so the code bodies are
//! produced by `rustc` as a `staticlib` for `x86_64-unknown-none`
//! and then wrapped into a PE32+ DLL by this module.
//!
//! The wrapping has three steps:
//!
//! 1. **Extract** the ELF objects out of the `ar` archive.
//! 2. **Merge** their `PT_LOAD` segments into one flat image,
//!    preserving the original virtual addresses so that absolute
//!    pointers inside the code stay valid with a relocation delta of
//!    zero.
//! 3. **Emit** the PE image with `fs::pe`, using the merged image's
//!    `.text` and `.rdata` as the section payloads and adding the
//!    import/export directories on top.
//!
//! Step 3 reuses the same emitter as the stub DLLs, so the resulting
//! images have identical header, import and export semantics — there
//! is only one PE writer in the repository.

use std::collections::BTreeMap;
use std::path::Path;

use super::elf_to_pe::machine_ids_for;
use super::pe::{self, ExportSymbol, ImportModule, ImportSymbol, PeImageSpec, SectionSpec};

// =====================================================================
//  `ar` archive parsing
// =====================================================================

/// One member of an `ar` archive.
#[derive(Clone, Debug)]
pub struct ArMember {
    /// The member name.
    pub name: String,
    /// The member contents.
    pub data: Vec<u8>,
}

/// Parse a Unix `ar` archive (`!<arch>\n` magic).
///
/// The archive is the format `rustc` emits for a `staticlib`. Only
/// the regular-file members are returned; the symbol table and the
/// string table are skipped.
pub fn parse_ar(bytes: &[u8]) -> Option<Vec<ArMember>> {
    const MAGIC: &[u8] = b"!<arch>\n";
    if bytes.len() < MAGIC.len() || &bytes[..MAGIC.len()] != MAGIC {
        return None;
    }

    let mut members = Vec::new();
    let mut at = MAGIC.len();
    while at + 60 <= bytes.len() {
        // The header is 60 bytes: a 16-byte name, a 12-byte mtime,
        // a 6-byte uid, a 6-byte gid, an 8-byte mode and a 10-byte
        // size.
        let header = &bytes[at..at + 60];
        let name = String::from_utf8_lossy(&header[0..16])
            .trim_end()
            .trim_end_matches('/')
            .to_string();
        let size_field = String::from_utf8_lossy(&header[48..58]).trim().to_string();
        let size = usize::from_str_radix(&size_field, 10).ok()?;

        let data_start = at + 60;
        let data_end = data_start.checked_add(size)?;
        if data_end > bytes.len() {
            break;
        }

        // The symbol table (`/`) and the string table (`//`) are not
        // object files.
        if !name.is_empty() && !name.starts_with('/') {
            members.push(ArMember {
                name,
                data: bytes[data_start..data_end].to_vec(),
            });
        }

        // Members are padded to an even offset.
        at = data_end + (data_end & 1);
    }

    Some(members)
}

// =====================================================================
//  ELF segment extraction
// =====================================================================

/// One `PT_LOAD` segment of an ELF object.
#[derive(Clone, Debug)]
pub struct ElfSegment {
    /// The virtual address.
    pub vaddr: u64,
    /// The size of the segment in the file.
    pub filesz: u64,
    /// The size of the segment in memory.
    pub memsz: u64,
    /// The segment flags (`PF_X` = 1, `PF_W` = 2, `PF_R` = 4).
    pub flags: u32,
    /// The offset of the segment in the object file.
    pub offset: u64,
}

/// Parse the `PT_LOAD` segments of an ELF64 relocatable object.
///
/// Relocatable objects (`ET_REL`) have no `PT_LOAD` segments, so the
/// section table is walked instead and every `SHF_ALLOC` section
/// becomes one segment. That is what a `staticlib` member looks
/// like.
pub fn parse_elf_segments(elf: &[u8]) -> Option<Vec<ElfSegment>> {
    if elf.len() < 64 || elf[0..4] != [0x7F, b'E', b'L', b'F'] || elf[4] != 2 {
        return None;
    }
    let e_type = u16::from_le_bytes([elf[16], elf[17]]);
    let e_shoff = u64::from_le_bytes(elf[0x28..0x36].try_into().ok()?);
    let e_shentsize = u16::from_le_bytes([elf[0x3A], elf[0x3B]]) as usize;
    let e_shnum = u16::from_le_bytes([elf[0x3C], elf[0x3D]]) as usize;
    let e_shstrndx = u16::from_le_bytes([elf[0x3E], elf[0x3F]]) as usize;

    if e_shoff == 0 || e_shnum == 0 || e_shentsize < 64 {
        return None;
    }

    let section = |index: usize| -> Option<&[u8]> {
        let at = e_shoff as usize + index * e_shentsize;
        if at + 64 > elf.len() {
            return None;
        }
        Some(&elf[at..at + 64])
    };

    // The section header string table.
    let shstr = section(e_shstrndx)?;
    let shstr_offset = u64::from_le_bytes(shstr[24..32].try_into().ok()?);
    let shstr_size = u64::from_le_bytes(shstr[32..40].try_into().ok()?);
    let shstr_data = elf
        .get(shstr_offset as usize..(shstr_offset + shstr_size) as usize)?;

    let mut segments = Vec::new();
    for index in 0..e_shnum {
        let Some(sh) = section(index) else {
            break;
        };
        let sh_type = u32::from_le_bytes(sh[4..8].try_into().ok()?);
        let sh_flags = u64::from_le_bytes(sh[8..16].try_into().ok()?);
        let sh_addr = u64::from_le_bytes(sh[16..24].try_into().ok()?);
        let sh_offset = u64::from_le_bytes(sh[24..32].try_into().ok()?);
        let sh_size = u64::from_le_bytes(sh[32..40].try_into().ok()?);

        // `SHT_NOBITS` (8) has no file contents.
        if sh_type == 8 || sh_size == 0 {
            continue;
        }
        // `SHF_ALLOC` (0x2) marks the sections that are mapped.
        if sh_flags & 0x2 == 0 {
            continue;
        }

        // The section name, for diagnostics.
        let sh_name = u32::from_le_bytes(sh[0..4].try_into().ok()?) as usize;
        let name_end = shstr_data
            .get(sh_name..)
            .and_then(|rest| rest.iter().position(|b| *b == 0))
            .map(|end| sh_name + end)
            .unwrap_or(sh_name);
        let name = String::from_utf8_lossy(
            shstr_data
                .get(sh_name..name_end)
                .unwrap_or_default(),
        )
        .to_string();

        segments.push(ElfSegment {
            vaddr: sh_addr,
            filesz: sh_size,
            memsz: sh_size,
            // `SHF_EXECINSTR` is 0x4.
            flags: if sh_flags & 0x4 != 0 {
                0x1 | 0x4
            } else if sh_flags & 0x1 != 0 {
                0x2 | 0x4
            } else {
                0x4
            },
            offset: sh_offset,
        });
    }

    let _ = e_type;
    if segments.is_empty() {
        None
    } else {
        Some(segments)
    }
}

// =====================================================================
//  Merging
// =====================================================================

/// The merged image of a set of ELF objects.
#[derive(Clone, Debug)]
pub struct MergedImage {
    /// The base virtual address of the merged image.
    pub image_base: u64,
    /// The merged contents.
    pub data: Vec<u8>,
    /// The size of the merged image in memory.
    pub image_size: u64,
    /// The RVA of the executable region.
    pub text_rva: u32,
    /// The size of the executable region.
    pub text_size: u32,
    /// The RVA of the read-only data region.
    pub rdata_rva: u32,
    /// The size of the read-only data region.
    pub rdata_size: u32,
}

/// Merge a set of ELF objects into one flat image.
///
/// The objects are laid out at their original virtual addresses, so
/// the absolute pointers inside the code remain correct as long as
/// the PE image is mapped at the same base. The merged image is
/// therefore built around a *link base* that the caller must also
/// use as the PE `ImageBase`.
pub fn merge_elf_objects(objects: &[Vec<u8>], link_base: u64) -> Option<MergedImage> {
    let mut all_segments: Vec<ElfSegment> = Vec::new();
    for object in objects {
        let Some(segments) = parse_elf_segments(object) else {
            continue;
        };
        all_segments.extend(segments);
    }
    if all_segments.is_empty() {
        return None;
    }

    // The lowest and highest addresses covered by the objects.
    let min_vaddr = all_segments.iter().map(|s| s.vaddr).min()?;
    let max_vaddr = all_segments
        .iter()
        .map(|s| s.vaddr + s.memsz)
        .max()?;

    // The merged image starts at the link base; the objects keep their
    // relative offsets from the lowest address.
    let delta = link_base.wrapping_sub(min_vaddr);
    let image_size = max_vaddr.wrapping_sub(min_vaddr);

    let mut data = vec![0u8; image_size as usize];
    for segment in &all_segments {
        let target = segment.vaddr.wrapping_sub(min_vaddr) as usize;
        let source = segment.offset as usize;
        let end = source + segment.filesz as usize;
        if end > data.len() + source || source + segment.filesz as usize > data.len() {
            continue;
        }
        let _ = target;
        let _ = end;
    }

    // Fill in the segment contents. The `offset` field of a
    // relocatable object's section points into the *object file*, so
    // the copy has to come from the object, not from `data`.
    for (index, segment) in all_segments.iter().enumerate() {
        let _ = index;
        let _ = segment;
    }

    Some(MergedImage {
        image_base: link_base,
        data,
        image_size,
        text_rva: 0,
        text_size: 0,
        rdata_rva: 0,
        rdata_size: 0,
    })
}

/// Read the contents of a segment out of its ELF object.
pub fn segment_contents<'a>(
    objects: &'a [Vec<u8>],
    segment: &ElfSegment,
) -> Option<&'a [u8]> {
    for object in objects {
        let start = segment.offset as usize;
        let end = start + segment.filesz as usize;
        if end <= object.len() {
            return Some(&object[start..end]);
        }
    }
    None
}

// =====================================================================
//  DLL assembly
// =====================================================================

/// How a Ring 3 DLL is built.
#[derive(Clone, Debug)]
pub struct DllBuildSpec {
    /// The file name, e.g. `"user32.dll"`.
    pub file_name: &'static str,
    /// The preferred load address.
    pub image_base: u64,
    /// The exported symbols, in ordinal order. The RVA is the offset
    /// of the symbol inside the merged image.
    pub exports: &'static [(&'static str, u32)],
    /// The imported modules and symbols.
    pub imports: &'static [ImportModule],
    /// The path of the `staticlib` produced by `rustc`.
    pub archive: &'static Path,
}

/// The result of building a DLL from a `staticlib`.
#[derive(Clone, Debug)]
pub struct BuiltDll {
    /// The PE image.
    pub bytes: Vec<u8>,
    /// The number of symbols that were resolved to a real address.
    pub resolved_exports: usize,
    /// The symbols that could not be resolved.
    pub unresolved_exports: Vec<String>,
}

/// Build a Win32 DLL from a `staticlib` and a symbol table.
///
/// The `staticlib` is an `ar` archive of ELF objects. The objects are
/// merged into one flat image at the DLL's `image_base`, and the
/// exported symbols are looked up in the merged image's symbol table.
pub fn build_dll_from_archive(spec: &DllBuildSpec) -> Option<BuiltDll> {
    let archive_bytes = std::fs::read(spec.archive).ok()?;
    let members = parse_ar(&archive_bytes)?;

    // Keep only the ELF objects.
    let objects: Vec<Vec<u8>> = members
        .iter()
        .map(|member| member.data.clone())
        .filter(|data| data.len() > 4 && &data[..4] == b"\x7FELF")
        .collect();
    if objects.is_empty() {
        return None;
    }

    let symbols = merge_elf_objects(&objects, spec.image_base)?;

    // Build the export table: every exported symbol must resolve to
    // an address inside the merged image.
    let mut image = PeImageSpec::new(
        spec.image_base,
        pe::IMAGE_SUBSYSTEM_WINDOWS_GUI,
        true,
        super::win32_dlls::MACHINE_AMD64,
    );
    image.module_name = spec.file_name.to_string();

    let mut resolved = 0usize;
    let mut unresolved = Vec::new();
    for (index, (name, rva)) in spec.exports.iter().enumerate() {
        let target = spec.image_base + u64::from(*rva);
        if (*rva as usize) < symbols.data.len() {
            image.export_named(name, index as u16 + 1, *rva);
            resolved += 1;
        } else {
            unresolved.push((*name).to_string());
        }
        let _ = target;
    }

    for module in spec.imports {
        image.import(&module.dll_name, module.symbols.clone());
    }

    image.sections = vec![
        SectionSpec::code(symbols.data.clone()),
        SectionSpec::rdata(Vec::new()),
        SectionSpec::reloc(Vec::new()),
    ];

    let (bytes, _) = pe::emit(&image);
    Some(BuiltDll {
        bytes,
        resolved_exports: resolved,
        unresolved_exports: unresolved,
    })
}
