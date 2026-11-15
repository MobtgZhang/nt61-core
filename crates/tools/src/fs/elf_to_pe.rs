//! Checked ELF64 to PE32+ conversion preserving the linked virtual addresses.
//! PE headers occupy their own page; PT_LOAD pages become PE sections. Segments
//! sharing a hardware page share its union of permissions, but overlapping ELF
//! memory ranges are rejected. This does not translate ELF code or its ABI.

const PAGE: u64 = 0x1000;
const HEADERS: usize = 0x1000;
const COFF: usize = 0x84;
const OPT: usize = COFF + 20;
const SECTIONS: usize = OPT + 0xf0;
const MAX_OUTPUT: usize = 512 * 1024 * 1024;

pub struct MachineIds {
    pub pe_machine: u16,
    pub elf_machine: u16,
}

pub fn machine_ids_for(arch: &str) -> MachineIds {
    let (pe_machine, elf_machine) = match arch {
        "x86_64" => (0x8664, 62),
        "aarch64" => (0xaa64, 183),
        "riscv64" => (0x5064, 243),
        // Verified against this toolchain's ELF64 LoongArch output.
        "loongarch64" => (0x6264, 258),
        _ => (0, 0),
    };
    MachineIds {
        pe_machine,
        elf_machine,
    }
}

fn u16_at(bytes: &[u8], off: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(off..off.checked_add(2)?)?.try_into().ok()?,
    ))
}
fn u32_at(bytes: &[u8], off: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(off..off.checked_add(4)?)?.try_into().ok()?,
    ))
}
fn u64_at(bytes: &[u8], off: usize) -> Option<u64> {
    Some(u64::from_le_bytes(
        bytes.get(off..off.checked_add(8)?)?.try_into().ok()?,
    ))
}
fn put16(bytes: &mut [u8], off: usize, value: u16) {
    bytes[off..off + 2].copy_from_slice(&value.to_le_bytes());
}
fn put32(bytes: &mut [u8], off: usize, value: u32) {
    bytes[off..off + 4].copy_from_slice(&value.to_le_bytes());
}
fn put64(bytes: &mut [u8], off: usize, value: u64) {
    bytes[off..off + 8].copy_from_slice(&value.to_le_bytes());
}
fn align(value: u64, alignment: u64) -> Option<u64> {
    Some(value.checked_add(alignment.checked_sub(1)?)? & !(alignment - 1))
}
fn as32(value: u64) -> Option<u32> {
    value.try_into().ok()
}

struct Segment {
    va: u64,
    end: u64,
    offset: usize,
    filesz: usize,
    flags: u32,
}
struct Section {
    va: u64,
    end: u64,
    flags: u32,
    data: Vec<u8>,
}

/// Convert a matching little-endian executable ELF64. Malformed, overlapping,
/// overflowing, non-executable-entry and unsupported images return `None`.
pub fn elf_to_pe(elf: &[u8], arch: &str) -> Option<Vec<u8>> {
    let ids = machine_ids_for(arch);
    if ids.pe_machine == 0
        || elf.len() < 64
        || elf.get(..7)? != b"\x7fELF\x02\x01\x01"
        || u16_at(elf, 16)? != 2
        || u16_at(elf, 18)? != ids.elf_machine
        || u32_at(elf, 20)? != 1
        || u16_at(elf, 52)? != 64
    {
        return None;
    }
    let entry = u64_at(elf, 24)?;
    let phoff = usize::try_from(u64_at(elf, 32)?).ok()?;
    let phsize = usize::from(u16_at(elf, 54)?);
    let phnum = usize::from(u16_at(elf, 56)?);
    if phsize < 56
        || phoff < 64
        || phnum == 0
        || phoff.checked_add(phnum.checked_mul(phsize)?)? > elf.len()
    {
        return None;
    }
    let mut segments = Vec::new();
    for i in 0..phnum {
        let off = phoff.checked_add(i.checked_mul(phsize)?)?;
        if u32_at(elf, off)? != 1 {
            continue;
        }
        let flags = u32_at(elf, off + 4)?;
        let file_off = usize::try_from(u64_at(elf, off + 8)?).ok()?;
        let va = u64_at(elf, off + 16)?;
        let filesz = usize::try_from(u64_at(elf, off + 32)?).ok()?;
        let memsz = u64_at(elf, off + 40)?;
        if filesz as u64 > memsz || file_off.checked_add(filesz)? > elf.len() {
            return None;
        }
        if memsz == 0 {
            continue;
        }
        let end = va.checked_add(memsz)?;
        segments.push(Segment {
            va,
            end,
            offset: file_off,
            filesz,
            flags,
        });
    }
    segments.sort_by_key(|s| s.va);
    if segments.is_empty()
        || segments.windows(2).any(|s| s[0].end > s[1].va)
        || !segments
            .iter()
            .any(|s| s.flags & 1 != 0 && entry >= s.va && entry < s.va + s.filesz as u64)
    {
        return None;
    }
    // PE ImageBase is 64 KiB aligned. Reserving at least one header page
    // below the first segment keeps every linked VA unchanged.
    let image_base = (segments[0].va & !(PAGE - 1)).checked_sub(PAGE)? & !0xffff;
    let mut sections: Vec<Section> = Vec::new();
    for seg in segments {
        let page_va = seg.va & !(PAGE - 1);
        let page_end = align(seg.end, PAGE)?;
        let overlap = sections.last().is_some_and(|s| page_va < s.end);
        if !overlap {
            sections.push(Section {
                va: page_va,
                end: page_end,
                flags: seg.flags,
                data: Vec::new(),
            });
        }
        let section = sections.last_mut()?;
        section.end = section.end.max(page_end);
        section.flags |= seg.flags;
        if seg.filesz != 0 {
            let data_off = usize::try_from(seg.va.checked_sub(section.va)?).ok()?;
            let data_end = data_off.checked_add(seg.filesz)?;
            if data_end > MAX_OUTPUT {
                return None;
            }
            section.data.resize(data_end, 0);
            section.data[data_off..data_end]
                .copy_from_slice(&elf[seg.offset..seg.offset + seg.filesz]);
        }
    }
    // Reserve one extra section header for .reloc.
    if SECTIONS.checked_add(sections.len().checked_add(1)?.checked_mul(40)?)? > HEADERS {
        return None;
    }
    let size_of_image = as32(sections.last()?.end.checked_sub(image_base)?)?;
    let mut file_size = HEADERS;
    let mut raw_sizes = Vec::new();
    for section in &sections {
        let size = usize::try_from(align(section.data.len() as u64, PAGE)?).ok()?;
        file_size = file_size.checked_add(size)?;
        raw_sizes.push(size);
    }
    if file_size > MAX_OUTPUT {
        return None;
    }
    let mut pe = vec![0u8; file_size];
    pe[..2].copy_from_slice(b"MZ");
    put32(&mut pe, 0x3c, 0x80);
    pe[0x80..0x84].copy_from_slice(b"PE\0\0");
    put16(&mut pe, COFF, ids.pe_machine);
    put16(&mut pe, COFF + 2, sections.len().try_into().ok()?);
    put16(&mut pe, COFF + 16, 0xf0);
    // Until real base relocations are appended this image is fixed-base.
    put16(&mut pe, COFF + 18, 0x23);
    put16(&mut pe, OPT, 0x20b);
    put32(&mut pe, OPT + 0x10, as32(entry.checked_sub(image_base)?)?);
    put32(
        &mut pe,
        OPT + 0x14,
        as32(sections.iter().find(|s| s.flags & 1 != 0)?.va - image_base)?,
    );
    put64(&mut pe, OPT + 0x18, image_base);
    put32(&mut pe, OPT + 0x20, PAGE as u32);
    put32(&mut pe, OPT + 0x24, PAGE as u32);
    for field in [0x28, 0x2c, 0x30] {
        put16(&mut pe, OPT + field, 6);
        put16(&mut pe, OPT + field + 2, 1);
    }
    put32(&mut pe, OPT + 0x38, size_of_image);
    put32(&mut pe, OPT + 0x3c, HEADERS as u32);
    put16(&mut pe, OPT + 0x44, 10);
    for (off, value) in [
        (0x48, 0x100000),
        (0x50, 0x1000),
        (0x58, 0x100000),
        (0x60, 0x1000),
    ] {
        put64(&mut pe, OPT + off, value);
    }
    put32(&mut pe, OPT + 0x6c, 16);
    let mut raw_off = HEADERS;
    let mut code_size = 0u32;
    let mut data_size = 0u32;
    let mut bss_size = 0u32;
    for (i, section) in sections.iter().enumerate() {
        let hdr = SECTIONS + i * 40;
        let raw_size = u32::try_from(raw_sizes[i]).ok()?;
        let mem_size = as32(section.end - section.va)?;
        let name: &[u8] = if section.flags & 1 != 0 {
            b".text"
        } else if section.flags & 2 != 0 {
            b".data"
        } else {
            b".rdata"
        };
        pe[hdr..hdr + name.len()].copy_from_slice(name);
        put32(&mut pe, hdr + 8, mem_size);
        put32(&mut pe, hdr + 12, as32(section.va - image_base)?);
        put32(&mut pe, hdr + 16, raw_size);
        put32(
            &mut pe,
            hdr + 20,
            if raw_size == 0 {
                0
            } else {
                raw_off.try_into().ok()?
            },
        );
        let mut characteristics = if section.flags & 1 != 0 {
            0x20
        } else if raw_size == 0 {
            0x80
        } else {
            0x40
        };
        if section.flags & 4 != 0 {
            characteristics |= 0x4000_0000;
        }
        if section.flags & 2 != 0 {
            characteristics |= 0x8000_0000;
        }
        if section.flags & 1 != 0 {
            characteristics |= 0x2000_0000;
        }
        put32(&mut pe, hdr + 36, characteristics);
        pe[raw_off..raw_off + section.data.len()].copy_from_slice(&section.data);
        raw_off += raw_sizes[i];
        if section.flags & 1 != 0 {
            code_size = code_size.checked_add(raw_size)?;
        } else {
            data_size = data_size.checked_add(raw_size)?;
        }
        bss_size = bss_size.checked_add(mem_size.saturating_sub(raw_size))?;
    }
    put32(&mut pe, OPT + 4, code_size);
    put32(&mut pe, OPT + 8, data_size);
    put32(&mut pe, OPT + 12, bss_size);
    Some(pe)
}

/// Append real DIR64 base relocation blocks to a converter-produced PE.
/// RVAs must designate complete 8-byte words within mapped section memory.
/// Invalid input leaves `pe` unchanged. An empty list emits no dummy section.
pub fn append_base_relocations(pe: &mut Vec<u8>, relocations: &[u32]) -> Option<()> {
    if pe.get(..2)? != b"MZ"
        || u32_at(pe, 0x3c)? != 0x80
        || pe.get(0x80..0x84)? != b"PE\0\0"
        || u16_at(pe, OPT)? != 0x20b
        || u16_at(pe, COFF + 16)? != 0xf0
        || u32_at(pe, OPT + 0x20)? != PAGE as u32
        || u32_at(pe, OPT + 0x24)? != PAGE as u32
    {
        return None;
    }
    let count = usize::from(u16_at(pe, COFF + 2)?);
    let headers = usize::try_from(u32_at(pe, OPT + 0x3c)?).ok()?;
    if count == 0
        || SECTIONS.checked_add(count.checked_add(1)?.checked_mul(40)?)? > headers
        || headers > pe.len()
        || u32_at(pe, OPT + 0x98)? != 0
    {
        return None;
    }
    let mut ranges = Vec::new();
    for i in 0..count {
        let hdr = SECTIONS + i * 40;
        let start = u64::from(u32_at(pe, hdr + 12)?);
        let end = start.checked_add(u64::from(u32_at(pe, hdr + 8)?))?;
        if start < headers as u64 || end <= start {
            return None;
        }
        ranges.push((start, end));
    }
    let mut rvas = relocations.to_vec();
    rvas.sort_unstable();
    rvas.dedup();
    for &rva in &rvas {
        let start = u64::from(rva);
        let end = start.checked_add(8)?;
        if !ranges.iter().any(|&(lo, hi)| start >= lo && end <= hi) {
            return None;
        }
    }
    if rvas.is_empty() {
        return Some(());
    }
    let mut data = Vec::new();
    let mut i = 0;
    while i < rvas.len() {
        let page = rvas[i] & !0xfff;
        let block = data.len();
        data.extend_from_slice(&page.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        while i < rvas.len() && rvas[i] & !0xfff == page {
            data.extend_from_slice(&(0xa000u16 | (rvas[i] & 0xfff) as u16).to_le_bytes());
            i += 1;
        }
        if (data.len() - block) % 4 != 0 {
            data.extend_from_slice(&0u16.to_le_bytes());
        }
        let size = u32::try_from(data.len() - block).ok()?;
        put32(&mut data, block + 4, size);
    }
    let rva = align(ranges.iter().map(|r| r.1).max()?, PAGE)?;
    let image_end = align(rva.checked_add(data.len() as u64)?, PAGE)?;
    let raw_off = usize::try_from(align(pe.len() as u64, PAGE)?).ok()?;
    let raw_size = usize::try_from(align(data.len() as u64, PAGE)?).ok()?;
    let final_size = raw_off.checked_add(raw_size)?;
    if final_size > MAX_OUTPUT {
        return None;
    }
    let image_size = as32(image_end)?;
    let reloc_rva = as32(rva)?;
    let raw_off32 = u32::try_from(raw_off).ok()?;
    let raw_size32 = u32::try_from(raw_size).ok()?;
    let data_size = u32::try_from(data.len()).ok()?;
    let new_count = u16::try_from(count.checked_add(1)?).ok()?;
    let initialized = u32_at(pe, OPT + 8)?.checked_add(raw_size32)?;
    // All fallible validation is above; commit the new section atomically.
    pe.resize(final_size, 0);
    pe[raw_off..raw_off + data.len()].copy_from_slice(&data);
    let hdr = SECTIONS + count * 40;
    pe[hdr..hdr + 40].fill(0);
    pe[hdr..hdr + 6].copy_from_slice(b".reloc");
    put32(pe, hdr + 8, data_size);
    put32(pe, hdr + 12, reloc_rva);
    put32(pe, hdr + 16, raw_size32);
    put32(pe, hdr + 20, raw_off32);
    put32(pe, hdr + 36, 0x4200_0040);
    put16(pe, COFF + 2, new_count);
    let chars = u16_at(pe, COFF + 18)? & !1;
    put16(pe, COFF + 18, chars);
    put32(pe, OPT + 8, initialized);
    put32(pe, OPT + 0x38, image_size);
    put32(pe, OPT + 0x98, reloc_rva);
    put32(pe, OPT + 0x9c, data_size);
    Some(())
}
