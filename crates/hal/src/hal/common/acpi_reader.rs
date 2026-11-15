//! Bounded ACPI physical addressing and table validation, independent of ISA.
extern crate alloc;
use alloc::vec::Vec;

pub const HEADER_SIZE: usize = 36;
pub const MAX_TABLE_LENGTH: usize = 1024 * 1024;
pub const MAX_RSDP_LENGTH: usize = 4096;

#[derive(Debug, PartialEq, Eq)]
pub struct MappingRange {
    pub physical_base: u64,
    pub offset: u64,
    pub pages: u64,
}

pub fn mapping_range(physical: u64, length: usize) -> Option<MappingRange> {
    if physical == 0 || length == 0 || length > MAX_TABLE_LENGTH {
        return None;
    }
    physical.checked_add(length as u64)?;
    let offset = physical & 4095;
    let extent = offset.checked_add(length as u64)?;
    Some(MappingRange {
        physical_base: physical & !4095,
        offset,
        pages: extent.checked_add(4095)? / 4096,
    })
}
pub fn u32_at(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?,
    ))
}
pub fn u64_at(bytes: &[u8], offset: usize) -> Option<u64> {
    Some(u64::from_le_bytes(
        bytes.get(offset..offset.checked_add(8)?)?.try_into().ok()?,
    ))
}
pub fn checksum_valid(bytes: &[u8]) -> bool {
    bytes.iter().fold(0u8, |sum, &b| sum.wrapping_add(b)) == 0
}
/// The second complete read must still describe exactly the probed length.
pub fn table(
    read: &mut impl FnMut(u64, usize) -> Option<Vec<u8>>,
    physical: u64,
) -> Option<Vec<u8>> {
    mapping_range(physical, HEADER_SIZE)?;
    let header = read(physical, HEADER_SIZE)?;
    if header.len() != HEADER_SIZE {
        return None;
    }
    let length = u32_at(&header, 4)? as usize;
    if !(HEADER_SIZE..=MAX_TABLE_LENGTH).contains(&length) {
        return None;
    }
    mapping_range(physical, length)?;
    let bytes = read(physical, length)?;
    if bytes.len() != length
        || bytes.get(..HEADER_SIZE) != Some(header.as_slice())
        || !checksum_valid(&bytes)
    {
        return None;
    }
    Some(bytes)
}

/// RSDP 1.0 has an independent 20-byte checksum, required even for 2.0+.
/// Prefer a valid XSDT, with the ACPI 1.0 RSDT retained as fallback.
pub fn roots(
    read: &mut impl FnMut(u64, usize) -> Option<Vec<u8>>,
    physical: u64,
) -> Option<Vec<(u64, usize)>> {
    mapping_range(physical, 20)?;
    let first = read(physical, 20)?;
    if first.len() != 20 || first.get(..8) != Some(b"RSD PTR ") || !checksum_valid(&first) {
        return None;
    }
    let mut roots = Vec::new();
    if first[15] >= 2 {
        let header = read(physical, HEADER_SIZE)?;
        if header.len() != HEADER_SIZE || header.get(..20) != Some(first.as_slice()) {
            return None;
        }
        let length = u32_at(&header, 20)? as usize;
        if !(HEADER_SIZE..=MAX_RSDP_LENGTH).contains(&length) {
            return None;
        }
        mapping_range(physical, length)?;
        let full = read(physical, length)?;
        if full.len() != length
            || full.get(..HEADER_SIZE) != Some(header.as_slice())
            || !checksum_valid(&full)
        {
            return None;
        }
        let xsdt = u64_at(&full, 24)?;
        if xsdt != 0 {
            roots.push((xsdt, 8));
        }
    }
    let rsdt = u64::from(u32_at(&first, 16)?);
    if rsdt != 0 {
        roots.push((rsdt, 4));
    }
    Some(roots)
}

pub fn root_entries(bytes: &[u8], stride: usize) -> Option<Vec<u64>> {
    let signature = match stride {
        8 => b"XSDT",
        4 => b"RSDT",
        _ => return None,
    };
    if bytes.len() < HEADER_SIZE
        || bytes.get(..4) != Some(signature)
        || u32_at(bytes, 4)? as usize != bytes.len()
        || !checksum_valid(bytes)
        || (bytes.len() - HEADER_SIZE) % stride != 0
    {
        return None;
    }
    let mut entries = Vec::new();
    for offset in (HEADER_SIZE..bytes.len()).step_by(stride) {
        let address = if stride == 8 {
            u64_at(bytes, offset)?
        } else {
            u64::from(u32_at(bytes, offset)?)
        };
        if address != 0 {
            entries.push(address);
        }
    }
    Some(entries)
}
