//! Attach a real PE export address table to an ELF-converted PE without moving
//! linked code/data. This performs no ABI translation: the explicit manifest
//! must only name wrappers audited for the importing platform's native ABI.
use nt61_boot_types::native_exports::{ExportTarget, NativeExport};
use std::collections::{BTreeMap, BTreeSet};

fn u16_at(b: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        b.get(o..o.checked_add(2)?)?.try_into().ok()?,
    ))
}
fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        b.get(o..o.checked_add(4)?)?.try_into().ok()?,
    ))
}
fn u64_at(b: &[u8], o: usize) -> Option<u64> {
    Some(u64::from_le_bytes(
        b.get(o..o.checked_add(8)?)?.try_into().ok()?,
    ))
}
fn put16(b: &mut [u8], o: usize, n: u16) {
    b[o..o + 2].copy_from_slice(&n.to_le_bytes());
}
fn put32(b: &mut [u8], o: usize, n: u32) {
    b[o..o + 4].copy_from_slice(&n.to_le_bytes());
}
fn align(n: usize, a: usize) -> Option<usize> {
    if !a.is_power_of_two() {
        return None;
    }
    Some(n.checked_add(a - 1)? & !(a - 1))
}
fn text_at(b: &[u8], o: usize) -> Option<&str> {
    let s = b.get(o..)?;
    std::str::from_utf8(s.get(..s.iter().position(|&b| b == 0)?)?).ok()
}
fn valid_text(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|c| (0x21..=0x7e).contains(&c))
}

#[derive(Clone, Copy)]
struct Symbol {
    value: u64,
    size: u64,
    kind: u8,
    file_offset: Option<usize>,
}

fn symbols(elf: &[u8]) -> Option<BTreeMap<&str, Symbol>> {
    if elf.get(..7)? != b"\x7fELF\x02\x01\x01" || u16_at(elf, 16)? != 2 {
        return None;
    }
    let offset = usize::try_from(u64_at(elf, 40)?).ok()?;
    let size = usize::from(u16_at(elf, 58)?);
    let count = usize::from(u16_at(elf, 60)?);
    if size < 64 || count == 0 || offset.checked_add(count.checked_mul(size)?)? > elf.len() {
        return None;
    }
    let mut found = BTreeMap::new();
    for i in 0..count {
        let sh = offset + i * size;
        if u32_at(elf, sh + 4)? != 2 {
            continue;
        } // SYMTAB, never infer stripped/dynamic symbols.
        let start = usize::try_from(u64_at(elf, sh + 24)?).ok()?;
        let len = usize::try_from(u64_at(elf, sh + 32)?).ok()?;
        let stride = usize::try_from(u64_at(elf, sh + 56)?).ok()?;
        let link = usize::try_from(u32_at(elf, sh + 40)?).ok()?;
        if stride < 24 || len % stride != 0 || start.checked_add(len)? > elf.len() || link >= count
        {
            return None;
        }
        let strsh = offset + link * size;
        if u32_at(elf, strsh + 4)? != 3 {
            return None;
        }
        let strstart = usize::try_from(u64_at(elf, strsh + 24)?).ok()?;
        let strlen = usize::try_from(u64_at(elf, strsh + 32)?).ok()?;
        let strings = elf.get(strstart..strstart.checked_add(strlen)?)?;
        for off in (start..start + len).step_by(stride) {
            let info = *elf.get(off + 4)?;
            let section = usize::from(u16_at(elf, off + 6)?);
            if !matches!(info >> 4, 1 | 2) || !matches!(info & 15, 1 | 2) || section == 0 {
                continue;
            }
            // COMMON, ABS, TLS and synthetic definitions are not callable/image data.
            if section >= count {
                continue;
            }
            let name = text_at(strings, usize::try_from(u32_at(elf, off)?).ok()?)?;
            let mut symbol = Symbol {
                value: u64_at(elf, off + 8)?,
                size: u64_at(elf, off + 16)?,
                kind: info & 15,
                file_offset: None,
            };
            if name.is_empty() {
                continue;
            }
            let owner = offset + section * size;
            let flags = u64_at(elf, owner + 8)?;
            let va = u64_at(elf, owner + 16)?;
            let extent = u64_at(elf, owner + 32)?;
            if flags & 0x400 != 0 {
                continue;
            } // SHF_TLS is not ordinary exported data.
            if flags & 2 == 0
                || symbol.value < va
                || symbol.value.checked_add(symbol.size.max(1))? > va.checked_add(extent)?
            {
                return None;
            }
            if symbol.kind == 2 {
                if flags & 4 == 0 || u32_at(elf, owner + 4)? != 1 {
                    return None;
                }
                let file_start =
                    u64_at(elf, owner + 24)?.checked_add(symbol.value.checked_sub(va)?)?;
                let file_end = file_start.checked_add(symbol.size)?;
                if file_end > elf.len() as u64 {
                    return None;
                }
                symbol.file_offset = Some(usize::try_from(file_start).ok()?);
            }
            if let Some(old) = found.insert(name, symbol) {
                if old.value != symbol.value || old.size != symbol.size || old.kind != symbol.kind {
                    return None;
                }
            }
        }
    }
    Some(found)
}

/// Add `.edata` using defined ELF FUNC/OBJECT symbols. Fails atomically on any
/// malformed input, missing symbol, architecture mismatch, stale EAT or target
/// outside the mapped section. The original relocation directory is preserved.
pub fn append_exports_from_elf(
    pe: &mut Vec<u8>,
    elf: &[u8],
    dll: &str,
    manifest: &[NativeExport],
) -> Result<(), &'static str> {
    let updated =
        append(pe, elf, dll, manifest).ok_or("invalid PE/ELF export layout, symbol or manifest")?;
    *pe = updated;
    Ok(())
}

fn append(pe: &[u8], elf: &[u8], dll: &str, exports: &[NativeExport]) -> Option<Vec<u8>> {
    if !valid_text(dll) || exports.is_empty() {
        return None;
    }
    let symbols = symbols(elf)?;
    let peoff = usize::try_from(u32_at(pe, 0x3c)?).ok()?;
    if pe.get(..2)? != b"MZ" || pe.get(peoff..peoff.checked_add(4)?)? != b"PE\0\0" {
        return None;
    }
    let coff = peoff.checked_add(4)?;
    let opt = coff.checked_add(20)?;
    let optsize = usize::from(u16_at(pe, coff + 16)?);
    if optsize < 0xf0 || u16_at(pe, opt)? != 0x20b || u32_at(pe, opt + 108)? < 16 {
        return None;
    }
    let machine = u16_at(pe, coff)?;
    if !matches!(
        (machine, u16_at(elf, 18)?),
        (0x8664, 62) | (0xaa64, 183) | (0x5064, 243) | (0x6264, 258)
    ) {
        return None;
    }
    // Existing certificates would be invalidated by a new section; require unsigned input.
    if u32_at(pe, opt + 112)? != 0
        || u32_at(pe, opt + 116)? != 0
        || u32_at(pe, opt + 144)? != 0
        || u32_at(pe, opt + 148)? != 0
    {
        return None;
    }
    let base = u64_at(pe, opt + 24)?;
    let section_alignment = usize::try_from(u32_at(pe, opt + 32)?).ok()?;
    let file_alignment = usize::try_from(u32_at(pe, opt + 36)?).ok()?;
    if section_alignment < 4096 || file_alignment < 512 || file_alignment > section_alignment {
        return None;
    }
    let headers = usize::try_from(u32_at(pe, opt + 60)?).ok()?;
    let table = opt.checked_add(optsize)?;
    let count = usize::from(u16_at(pe, coff + 2)?);
    if count == 0
        || count >= u16::MAX as usize
        || table.checked_add((count + 1).checked_mul(40)?)? > headers
        || headers > pe.len()
    {
        return None;
    }
    let mut ranges = Vec::new();
    let mut end = headers;
    for i in 0..count {
        let h = table + i * 40;
        let va = u32_at(pe, h + 12)?;
        let virtual_size = u32_at(pe, h + 8)?;
        let raw_size = u32_at(pe, h + 16)?;
        let raw = u32_at(pe, h + 20)?;
        if virtual_size == 0
            || va < headers as u32
            || usize::try_from(raw.checked_add(raw_size)?).ok()? > pe.len()
        {
            return None;
        }
        let last = va.checked_add(virtual_size)?;
        if ranges.iter().any(|&(lo, hi, _)| va < hi && last > lo) {
            return None;
        }
        end = end.max(usize::try_from(last).ok()?);
        ranges.push((va, last, u32_at(pe, h + 36)?));
    }
    let rva = u32::try_from(align(end, section_alignment)?).ok()?;
    let mut names = BTreeSet::new();
    let mut ordinals = BTreeSet::new();
    let mut resolved = Vec::new();
    for e in exports {
        if !valid_text(e.name)
            || e.ordinal == 0
            || !names.insert(e.name)
            || !ordinals.insert(e.ordinal)
        {
            return None;
        }
        let value = match e.target {
            ExportTarget::Forwarder(target) => {
                let (module, name) = target.rsplit_once('.')?;
                if !valid_text(module)
                    || !valid_text(name)
                    || (name.starts_with('#')
                        && (!name[1..].bytes().all(|b| b.is_ascii_digit())
                            || name[1..].parse::<u16>().ok()? == 0))
                {
                    return None;
                }
                None
            }
            ExportTarget::Function(name) | ExportTarget::Data(name) => {
                let s = symbols.get(name)?;
                let expected = if matches!(e.target, ExportTarget::Function(_)) {
                    2
                } else {
                    1
                };
                if s.kind != expected || s.size == 0 {
                    return None;
                }
                let target = u32::try_from(s.value.checked_sub(base)?).ok()?;
                let last = u32::try_from(u64::from(target).checked_add(s.size)?).ok()?;
                if !ranges.iter().any(|&(lo, hi, flags)| {
                    target >= lo
                        && last <= hi
                        && flags & 0x4000_0000 != 0
                        && (expected != 2 || flags & 0x2000_0000 != 0)
                }) {
                    return None;
                }
                if let Some(file_start) = s.file_offset {
                    // A FUNC must resolve to the original file-backed code bytes,
                    // never padding/BSS or a PE from a different ELF build.
                    let mut matching = false;
                    for i in 0..count {
                        let h = table + i * 40;
                        let va = u32_at(pe, h + 12)?;
                        let raw_size = u32_at(pe, h + 16)?;
                        if target < va || last > va.checked_add(raw_size)? {
                            continue;
                        }
                        let raw =
                            usize::try_from(u32_at(pe, h + 20)?.checked_add(target - va)?).ok()?;
                        let length = usize::try_from(s.size).ok()?;
                        matching = pe.get(raw..raw.checked_add(length)?)?
                            == elf.get(file_start..file_start.checked_add(length)?)?;
                        break;
                    }
                    if !matching {
                        return None;
                    }
                }
                if target >= rva {
                    return None;
                }
                Some(target)
            }
        };
        resolved.push((e, value));
    }
    resolved.sort_by_key(|(e, _)| e.name);
    let first = *ordinals.first()?;
    let functions = usize::from(*ordinals.last()? - first) + 1;
    let eat = 40usize;
    let npt = eat.checked_add(functions.checked_mul(4)?)?;
    let ord = npt.checked_add(exports.len().checked_mul(4)?)?;
    let mut data = vec![0; ord.checked_add(exports.len().checked_mul(2)?)?];
    let push_string = |data: &mut Vec<u8>, value: &str| -> Option<u32> {
        let address = rva.checked_add(u32::try_from(data.len()).ok()?)?;
        data.extend_from_slice(value.as_bytes());
        data.push(0);
        Some(address)
    };
    let dll_rva = push_string(&mut data, dll)?;
    put32(&mut data, 12, dll_rva);
    put32(&mut data, 16, u32::from(first));
    put32(&mut data, 20, u32::try_from(functions).ok()?);
    put32(&mut data, 24, u32::try_from(exports.len()).ok()?);
    put32(&mut data, 28, rva.checked_add(eat as u32)?);
    put32(&mut data, 32, rva.checked_add(u32::try_from(npt).ok()?)?);
    put32(&mut data, 36, rva.checked_add(u32::try_from(ord).ok()?)?);
    for (i, (e, target)) in resolved.iter().enumerate() {
        let name = push_string(&mut data, e.name)?;
        let address = match (target, e.target) {
            (Some(va), _) => *va,
            (None, ExportTarget::Forwarder(s)) => push_string(&mut data, s)?,
            _ => return None,
        };
        put32(&mut data, eat + usize::from(e.ordinal - first) * 4, address);
        put32(&mut data, npt + i * 4, name);
        put16(&mut data, ord + i * 2, e.ordinal - first);
    }
    let size = u32::try_from(data.len()).ok()?;
    let image_size = u32::try_from(align(
        usize::try_from(rva).ok()?.checked_add(data.len())?,
        section_alignment,
    )?)
    .ok()?;
    let raw = align(pe.len(), file_alignment)?;
    let raw_size = align(data.len(), file_alignment)?;
    let output_size = raw.checked_add(raw_size)?;
    if output_size > 512 * 1024 * 1024 {
        return None;
    }
    let initialized = u32_at(pe, opt + 8)?.checked_add(u32::try_from(raw_size).ok()?)?;
    let mut out = pe.to_vec();
    out.resize(output_size, 0);
    out[raw..raw + data.len()].copy_from_slice(&data);
    let h = table + count * 40;
    out[h..h + 40].fill(0);
    out[h..h + 6].copy_from_slice(b".edata");
    put32(&mut out, h + 8, size);
    put32(&mut out, h + 12, rva);
    put32(&mut out, h + 16, u32::try_from(raw_size).ok()?);
    put32(&mut out, h + 20, u32::try_from(raw).ok()?);
    put32(&mut out, h + 36, 0x4000_0040);
    put16(&mut out, coff + 2, (count + 1) as u16);
    put32(&mut out, opt + 8, initialized);
    put32(&mut out, opt + 56, image_size);
    put32(&mut out, opt + 64, 0); // caller recalculates PE checksum after all mutations
    put32(&mut out, opt + 112, rva);
    put32(&mut out, opt + 116, size);
    Some(out)
}
