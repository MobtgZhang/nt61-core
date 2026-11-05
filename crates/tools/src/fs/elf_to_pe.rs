//! Freestanding-ELF → PE32+ converter for Rust userland images.
//!
//! The historic path wrapped bare-metal userland ELFs with
//! objcopy -O pei-*, whose output section table cannot be trusted
//! (mangled VMAs, wrong raw sizes and characteristics). The kernel's
//! user-page mapper derives per-page permissions from the section
//! table, so a mangled table maps the entry page as non-executable
//! and the process NX-faults on its first instruction fetch — the
//! "black desktop, nothing drawn" failure mode. This converter keeps
//! the ELF's own VA layout (RVA = vaddr - image base, so absolute
//! pointers stay valid with delta = 0) and emits one section per
//! PT_LOAD segment with correct characteristics.

/// Per-target PE/ELF machine identifiers.
pub struct MachineIds {
    pub pe_machine: u16,
    pub elf_machine: u16,
}

pub fn machine_ids_for(arch: &str) -> MachineIds {
    match arch {
        "x86_64" => MachineIds { pe_machine: 0x8664, elf_machine: 62 },
        "aarch64" => MachineIds { pe_machine: 0xAA64, elf_machine: 183 },
        "riscv64" => MachineIds { pe_machine: 0x5064, elf_machine: 243 },
        _ => MachineIds { pe_machine: 0x6264, elf_machine: 258 },
    }
}

/// Convert a freestanding ELF64 image into a well-formed PE32+ image.
/// Returns None when the input is not a matching ELF64 executable.
pub fn elf_to_pe(elf: &[u8], arch: &str) -> Option<Vec<u8>> {
    if elf.len() < 64 || elf[0..4] != [0x7F, b'E', b'L', b'F'] || elf[4] != 2 {
        return None;
    }
    let ids = machine_ids_for(arch);
    let elf_machine = u16::from_le_bytes([elf[18], elf[19]]);
    if elf_machine != ids.elf_machine {
        return None;
    }
    let entry = u64::from_le_bytes(elf[0x18..0x20].try_into().ok()?);
    let phoff = u64::from_le_bytes(elf[0x20..0x28].try_into().ok()?);
    let phentsize = u16::from_le_bytes(elf[0x36..0x38].try_into().ok()?) as usize;
    let phnum = u16::from_le_bytes(elf[0x38..0x3A].try_into().ok()?) as usize;
    if phentsize < 56 || phoff == 0 || phnum == 0 {
        return None;
    }

    // Collect PT_LOAD segments (p_type = 1):
    // (vaddr, filesz, memsz, flags, file_offset).
    let mut segments: Vec<(u64, u64, u64, u32, u64)> = Vec::new();
    for i in 0..phnum {
        let base = phoff as usize + i * phentsize;
        if base + 56 > elf.len() {
            return None;
        }
        let p_type = u32::from_le_bytes(elf[base..base + 4].try_into().ok()?);
        if p_type != 1 {
            continue;
        }
        let p_flags = u32::from_le_bytes(elf[base + 4..base + 8].try_into().ok()?);
        let p_offset = u64::from_le_bytes(elf[base + 8..base + 16].try_into().ok()?);
        let p_vaddr = u64::from_le_bytes(elf[base + 16..base + 24].try_into().ok()?);
        let p_filesz = u64::from_le_bytes(elf[base + 32..base + 40].try_into().ok()?);
        let p_memsz = u64::from_le_bytes(elf[base + 40..base + 48].try_into().ok()?);
        segments.push((p_vaddr, p_filesz, p_memsz, p_flags, p_offset));
    }
    if segments.is_empty() {
        return None;
    }
    // Reserve the RVA 0 page for the PE headers. In a PE image the
    // headers live at RVA 0 and every section must start at or above
    // SizeOfHeaders; an ELF's first PT_LOAD typically starts exactly at
    // its image base (RVA 0), which would make load_image_full's
    // section copy overwrite the mapped PE headers. Shift the PE image
    // base one page below the lowest ELF vaddr so every section RVA is
    // >= 0x1000 while runtime VAs (pe_base + rva) still equal the ELF
    // vaddrs (delta stays 0).
    let min_vaddr = *segments.iter().map(|s| &s.0).min()?;
    if min_vaddr < 0x1000 {
        return None;
    }
    let image_base = min_vaddr - 0x1000;
    let image_top = segments
        .iter()
        .map(|s| s.0 + s.2)
        .max()?
        .next_multiple_of(0x1000);
    let size_of_image = image_top.checked_sub(image_base)?;
    let entry_rva = entry.checked_sub(image_base)?;

    // File layout: one header page, then one aligned run per
    // segment that carries file bytes.
    const HEADER_SIZE: u64 = 0x1000;
    const FILE_ALIGN: u64 = 0x1000;
    let mut file_size = HEADER_SIZE;
    let mut raw_offsets: Vec<u64> = Vec::with_capacity(segments.len());
    for (_, filesz, _, _, _) in &segments {
        if *filesz == 0 {
            raw_offsets.push(0);
            continue;
        }
        raw_offsets.push(file_size);
        file_size = file_size.next_multiple_of(FILE_ALIGN)
            + filesz.next_multiple_of(FILE_ALIGN);
    }

    let mut pe = vec![0u8; file_size as usize];
    pe[0..2].copy_from_slice(b"MZ");
    pe[0x3C..0x40].copy_from_slice(&0x80u32.to_le_bytes());
    pe[0x80] = b'P';
    pe[0x81] = b'E';
    pe[0x82] = 0;
    pe[0x83] = 0;
    let coff = 0x84usize;
    pe[coff..coff + 2].copy_from_slice(&ids.pe_machine.to_le_bytes());
    pe[coff + 2..coff + 4]
        .copy_from_slice(&(segments.len() as u16).to_le_bytes());
    pe[coff + 16..coff + 18].copy_from_slice(&0xF0u16.to_le_bytes());
    pe[coff + 18..coff + 20].copy_from_slice(&0x0022u16.to_le_bytes());

    let opt = coff + 20;
    pe[opt..opt + 2].copy_from_slice(&0x020Bu16.to_le_bytes());
    let size_of_code: u32 = segments
        .iter()
        .filter(|s| s.3 & 0x1 != 0)
        .map(|s| s.1 as u32)
        .sum();
    pe[opt + 4..opt + 8].copy_from_slice(&size_of_code.to_le_bytes());
    pe[opt + 0x10..opt + 0x14]
        .copy_from_slice(&(entry_rva as u32).to_le_bytes());
    pe[opt + 0x18..opt + 0x20]
        .copy_from_slice(&image_base.to_le_bytes());
    pe[opt + 0x20..opt + 0x24]
        .copy_from_slice(&0x1000u32.to_le_bytes());
    pe[opt + 0x24..opt + 0x28]
        .copy_from_slice(&(FILE_ALIGN as u32).to_le_bytes());
    // NT 6.1 version fields (OS 6.1 / Subsystem 6.1).
    pe[opt + 0x28] = 6;
    pe[opt + 0x2A] = 1;
    pe[opt + 0x2C] = 6;
    // SizeOfHeaders: one full header page. load_image_full uses this to
    // copy the DOS/PE headers and section table into the mapped image;
    // a zero here makes the mapped image start with a zeroed header.
    pe[opt + 0x2E] = 1;
    pe[opt + 0x30] = 6;
    pe[opt + 0x32] = 1;
    pe[opt + 0x38..opt + 0x3C]
        .copy_from_slice(&(size_of_image as u32).to_le_bytes());
    pe[opt + 0x3C..opt + 0x40]
        .copy_from_slice(&(HEADER_SIZE as u32).to_le_bytes());
    pe[opt + 0x44..opt + 0x46].copy_from_slice(&0x000Au16.to_le_bytes());
    pe[opt + 0x48..opt + 0x50].copy_from_slice(&0x100000u64.to_le_bytes());
    pe[opt + 0x50..opt + 0x58].copy_from_slice(&0x1000u64.to_le_bytes());
    pe[opt + 0x58..opt + 0x60].copy_from_slice(&0x100000u64.to_le_bytes());
    pe[opt + 0x60..opt + 0x68].copy_from_slice(&0x1000u64.to_le_bytes());
    pe[opt + 0x6C..opt + 0x70].copy_from_slice(&16u32.to_le_bytes());

    let sect_table = opt + 0xF0;
    const CODE: u32 = 0x6000_0020; // CNT_CODE | EXECUTE | READ
    const DATA_RW: u32 = 0xC000_0040; // CNT_DATA | READ | WRITE
    const DATA_RO: u32 = 0x4000_0040; // CNT_DATA | READ
    for (index, (vaddr, filesz, memsz, flags, file_off)) in
        segments.iter().enumerate()
    {
        let header = sect_table as usize + index * 40;
        if header + 40 > HEADER_SIZE as usize {
            return None;
        }
        let name: &[u8] = if flags & 0x1 != 0 {
            b".text"
        } else if flags & 0x2 != 0 {
            b".data"
        } else {
            b".rdata"
        };
        pe[header..header + name.len()]
            .copy_from_slice(name);
        pe[header + 8..header + 12]
            .copy_from_slice(&(*memsz as u32).to_le_bytes());
        pe[header + 12..header + 16].copy_from_slice(
            &((*vaddr - image_base) as u32).to_le_bytes(),
        );
        pe[header + 16..header + 20]
            .copy_from_slice(&(*filesz as u32).to_le_bytes());
        pe[header + 20..header + 24]
            .copy_from_slice(&(raw_offsets[index] as u32).to_le_bytes());
        let chars = if flags & 0x1 != 0 {
            CODE
        } else if flags & 0x2 != 0 {
            DATA_RW
        } else {
            DATA_RO
        };
        pe[header + 36..header + 40]
            .copy_from_slice(&chars.to_le_bytes());
        if *filesz > 0 {
            let src = usize::try_from(*file_off).ok()?;
            let src_end = src.checked_add(*filesz as usize)?;
            if src_end > elf.len() {
                return None;
            }
            let dst = raw_offsets[index] as usize;
            pe[dst..dst + *filesz as usize]
                .copy_from_slice(&elf[src..src_end]);
        }
    }
    Some(pe)
}
