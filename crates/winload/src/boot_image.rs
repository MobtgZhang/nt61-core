//! Checked PE32+ layout and mapped relocation operations shared by all boot architectures.
//!
//! This validates file layout, not NT loader/driver ABI compatibility or image signatures.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub image_base: u64,
    pub image_size: usize,
    pub headers_size: usize,
    pub entry_rva: u32,
    pub is_dll: bool,
    pub optional_offset: usize,
    sections_offset: usize,
    section_count: usize,
    directory_count: usize,
}

fn range(data: &[u8], offset: usize, size: usize) -> Option<&[u8]> {
    data.get(offset..offset.checked_add(size)?)
}
fn u16_at(data: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(range(data, offset, 2)?.try_into().ok()?))
}
fn u32_at(data: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(range(data, offset, 4)?.try_into().ok()?))
}
fn u64_at(data: &[u8], offset: usize) -> Option<u64> {
    Some(u64::from_le_bytes(range(data, offset, 8)?.try_into().ok()?))
}

/// Locate an optional header without casting untrusted, potentially unaligned bytes.
/// Both the COFF declaration and the actual buffer must contain the complete header.
pub fn optional_header_offset(data: &[u8]) -> Option<usize> {
    if range(data, 0, 2)? != b"MZ" {
        return None;
    }
    let pe = usize::try_from(i32::from_le_bytes(range(data, 0x3c, 4)?.try_into().ok()?)).ok()?;
    if pe < 64 || range(data, pe, 4)? != b"PE\0\0" {
        return None;
    }
    let optional = pe.checked_add(24)?;
    let size = u16_at(data, pe.checked_add(20)?)? as usize;
    range(data, optional, size)?;
    // CheckSum occupies bytes 0x40..0x44 in both PE32 and PE32+.
    if size < 0x44 || !matches!(u16_at(data, optional)?, 0x10b | 0x20b) {
        return None;
    }
    Some(optional)
}

impl Header {
    /// Validate a raw boot image before allocating or writing any mapped memory.
    pub fn parse(data: &[u8], expected_machine: u16) -> Result<Self, &'static str> {
        let optional = optional_header_offset(data).ok_or("invalid PE header")?;
        let coff = optional - 20;
        if u16_at(data, coff) != Some(expected_machine) {
            return Err("PE machine mismatch");
        }
        if u16_at(data, optional) != Some(0x20b) {
            return Err("boot image is not PE32+");
        }
        let optional_size = u16_at(data, coff + 16).ok_or("missing optional size")? as usize;
        if optional_size < 0x70 {
            return Err("short PE32+ optional header");
        }
        let directory_count =
            u32_at(data, optional + 0x6c).ok_or("missing directory count")? as usize;
        if directory_count > (optional_size - 0x70) / 8 {
            return Err("truncated data directories");
        }
        let section_count = u16_at(data, coff + 2).ok_or("missing sections")? as usize;
        if section_count == 0 || section_count > 96 {
            return Err("invalid section count");
        }
        let sections_offset = optional
            .checked_add(optional_size)
            .ok_or("section offset overflow")?;
        let table_size = section_count
            .checked_mul(40)
            .ok_or("section size overflow")?;
        range(data, sections_offset, table_size).ok_or("truncated section table")?;
        let image_size = u32_at(data, optional + 0x38).ok_or("missing image size")? as usize;
        let headers_size = u32_at(data, optional + 0x3c).ok_or("missing headers size")? as usize;
        let entry_rva = u32_at(data, optional + 0x10).ok_or("missing entry point")?;
        if image_size == 0 || image_size > 256 * 1024 * 1024 {
            return Err("invalid image size");
        }
        if headers_size < sections_offset + table_size || headers_size > image_size {
            return Err("headers exceed image or omit section table");
        }
        range(data, 0, headers_size).ok_or("truncated headers")?;
        if entry_rva as usize >= image_size {
            return Err("entry point outside image");
        }
        let header = Self {
            image_base: u64_at(data, optional + 0x18).ok_or("missing image base")?,
            image_size,
            headers_size,
            entry_rva,
            is_dll: u16_at(data, coff + 18).unwrap_or(0) & 0x2000 != 0,
            optional_offset: optional,
            sections_offset,
            section_count,
            directory_count,
        };
        for i in 0..section_count {
            let section = header.section(data, i)?;
            let extent = section.virtual_size.max(section.raw_size);
            let end = section
                .rva
                .checked_add(extent)
                .ok_or("section RVA overflow")?;
            if end > image_size || (extent != 0 && section.rva < headers_size) {
                return Err("section outside image or overlaps headers");
            }
            if section.raw_size != 0 {
                if section.raw_offset < headers_size {
                    return Err("section raw data overlaps headers");
                }
                range(data, section.raw_offset, section.raw_size)
                    .ok_or("truncated section data")?;
            }
            for j in 0..i {
                let previous = header.section(data, j)?;
                let previous_end = previous.rva + previous.virtual_size.max(previous.raw_size);
                if extent != 0
                    && previous_end > previous.rva
                    && section.rva < previous_end
                    && previous.rva < end
                {
                    return Err("overlapping virtual sections");
                }
            }
        }
        Ok(header)
    }

    fn section(self, data: &[u8], index: usize) -> Result<Section, &'static str> {
        let offset = self.sections_offset + index * 40;
        Ok(Section {
            virtual_size: u32_at(data, offset + 8).ok_or("missing virtual size")? as usize,
            rva: u32_at(data, offset + 12).ok_or("missing section RVA")? as usize,
            raw_size: u32_at(data, offset + 16).ok_or("missing raw size")? as usize,
            raw_offset: u32_at(data, offset + 20).ok_or("missing raw offset")? as usize,
        })
    }

    /// Map validated sections, including raw data whose VirtualSize is zero.
    pub fn map(self, raw: &[u8], destination: &mut [u8]) -> Result<usize, &'static str> {
        // Revalidate: callers cannot turn an arbitrary/mismatched Header into a write primitive.
        let machine = u16_at(raw, self.optional_offset - 20).ok_or("missing machine")?;
        if Self::parse(raw, machine)? != self || destination.len() < self.image_size {
            return Err("mapped image size mismatch");
        }
        destination[..self.image_size].fill(0);
        destination[..self.headers_size].copy_from_slice(&raw[..self.headers_size]);
        let mut count = 0;
        for index in 0..self.section_count {
            let section = self.section(raw, index)?;
            if section.raw_size != 0 {
                destination[section.rva..section.rva + section.raw_size].copy_from_slice(
                    &raw[section.raw_offset..section.raw_offset + section.raw_size],
                );
                count += 1;
            }
        }
        Ok(count)
    }

    /// Apply the project-emitted HIGHLOW/DIR64 relocations to a mapped image.
    /// Every block and write target is checked before any relocation is applied.
    /// A nonzero slide requires an explicit relocation directory. A known
    /// position-independent payload can carry a valid ABSOLUTE-only block;
    /// absence of a table never proves arbitrary external code is relocatable.
    pub fn relocate(self, mapped: &mut [u8], load_base: u64) -> Result<usize, &'static str> {
        self.walk_relocations(mapped, load_base, true)
    }

    /// Check the entire relocation stream without modifying any mapped bytes.
    pub fn validate_relocations(
        self,
        mapped: &mut [u8],
        load_base: u64,
    ) -> Result<(), &'static str> {
        self.walk_relocations(mapped, load_base, false).map(|_| ())
    }

    fn walk_relocations(
        self,
        mapped: &mut [u8],
        load_base: u64,
        write: bool,
    ) -> Result<usize, &'static str> {
        if mapped.len() < self.image_size {
            return Err("short mapped image");
        }
        if load_base == self.image_base {
            return Ok(0);
        }
        if self.directory_count <= 5 {
            return Err("image has no base relocation directory");
        }
        let directory = self.optional_offset + 0x70 + 5 * 8;
        let start = u32_at(mapped, directory).ok_or("missing relocation directory")? as usize;
        let size = u32_at(mapped, directory + 4).ok_or("missing relocation size")? as usize;
        if start == 0 && size == 0 {
            return Err("image has no base relocation directory");
        }
        let end = start
            .checked_add(size)
            .ok_or("relocation directory overflow")?;
        if start == 0 || size == 0 || end > self.image_size {
            return Err("relocation directory outside image");
        }
        let delta = load_base.wrapping_sub(self.image_base);
        let mut count = 0;
        // Pass one validates the entire stream; pass two writes. Reject writes into
        // the stream itself so an earlier relocation cannot alter a later descriptor.
        for apply in [false, true] {
            if apply && !write {
                break;
            }
            let mut block = start;
            while block < end {
                if end - block < 8 {
                    return Err("truncated relocation block");
                }
                let page = u32_at(mapped, block).ok_or("missing relocation page")? as usize;
                let block_size =
                    u32_at(mapped, block + 4).ok_or("missing relocation block size")? as usize;
                if block_size < 8 || block_size % 2 != 0 || block_size > end - block {
                    return Err("invalid relocation block size");
                }
                for offset in (block + 8..block + block_size).step_by(2) {
                    let entry = u16_at(mapped, offset).ok_or("missing relocation entry")?;
                    let width = match entry >> 12 {
                        0 => continue,
                        3 => 4,
                        10 => 8,
                        _ => return Err("unsupported boot relocation type"),
                    };
                    let target = page
                        .checked_add((entry & 0xfff) as usize)
                        .ok_or("relocation target overflow")?;
                    let target_end = target
                        .checked_add(width)
                        .ok_or("relocation target overflow")?;
                    if target_end > self.image_size || (target < end && start < target_end) {
                        return Err("relocation target outside image or overlaps table");
                    }
                    if apply {
                        if width == 8 {
                            let value = u64_at(mapped, target).ok_or("short relocation target")?;
                            mapped[target..target_end]
                                .copy_from_slice(&value.wrapping_add(delta).to_le_bytes());
                        } else {
                            let value = u32_at(mapped, target).ok_or("short relocation target")?;
                            mapped[target..target_end]
                                .copy_from_slice(&value.wrapping_add(delta as u32).to_le_bytes());
                        }
                        count += 1;
                    }
                }
                block += block_size;
            }
        }
        Ok(count)
    }
}

struct Section {
    virtual_size: usize,
    rva: usize,
    raw_size: usize,
    raw_offset: usize,
}

#[cfg(test)]
mod tests {
    extern crate alloc;
    use self::alloc::{vec, vec::Vec};
    use super::*;
    fn put16(bytes: &mut [u8], offset: usize, value: u16) {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
    fn put32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    fn image(machine: u16) -> Vec<u8> {
        let mut bytes = vec![0; 0x400];
        bytes[..2].copy_from_slice(b"MZ");
        put32(&mut bytes, 0x3c, 0x80);
        bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
        put16(&mut bytes, 0x84, machine);
        put16(&mut bytes, 0x86, 1);
        put16(&mut bytes, 0x94, 0xf0);
        put16(&mut bytes, 0x98, 0x20b);
        put32(&mut bytes, 0xa8, 0x1000);
        put32(&mut bytes, 0xb0, 0x400000);
        put32(&mut bytes, 0xd0, 0x2000);
        put32(&mut bytes, 0xd4, 0x200);
        put32(&mut bytes, 0x104, 16);
        put32(&mut bytes, 0x190, 0x200);
        put32(&mut bytes, 0x194, 0x1000);
        put32(&mut bytes, 0x198, 0x200);
        put32(&mut bytes, 0x19c, 0x200);
        bytes
    }
    #[test]
    fn all_four_machines_and_raw_with_zero_virtual_size() {
        for machine in [0x8664, 0xaa64, 0x5064, 0x6264] {
            let mut raw = image(machine);
            put32(&mut raw, 0x190, 0);
            raw[0x200] = 0xcc;
            let header = Header::parse(&raw, machine).unwrap();
            let mut mapped = vec![0xa5; header.image_size];
            header.map(&raw, &mut mapped).unwrap();
            assert_eq!(mapped[0x1000], 0xcc);
            assert_eq!(mapped[0x1800], 0);
            assert!(Header::parse(&raw, machine ^ 1).is_err());
        }
    }
    #[test]
    fn malformed_headers_and_truncated_sections_are_rejected() {
        let original = image(0x8664);
        for (offset, value) in [
            (0x3c, u32::MAX),
            (0xd4, 0x3000),
            (0xa8, 0x2000),
            (0x104, 17),
            (0x198, 0x300),
            (0x194, 0x1f00),
            (0x19c, 0x100),
        ] {
            let mut raw = original.clone();
            put32(&mut raw, offset, value);
            assert!(Header::parse(&raw, 0x8664).is_err(), "offset {offset:x}");
        }
        let mut raw = original;
        put16(&mut raw, 0x94, 2);
        assert!(Header::parse(&raw, 0x8664).is_err());
    }
    fn reloc_image(machine: u16) -> (Header, Vec<u8>) {
        let mut raw = image(machine);
        put32(&mut raw, 0x130, 0x1100);
        put32(&mut raw, 0x134, 12);
        let header = Header::parse(&raw, machine).unwrap();
        let mut mapped = vec![0; header.image_size];
        header.map(&raw, &mut mapped).unwrap();
        put32(&mut mapped, 0x1100, 0x1000);
        put32(&mut mapped, 0x1104, 12);
        put16(&mut mapped, 0x1108, 0xa020);
        mapped[0x1020..0x1028].copy_from_slice(&0x401234u64.to_le_bytes());
        (header, mapped)
    }
    #[test]
    fn dir64_relocates_at_lower_base_on_all_four_machines() {
        for machine in [0x8664, 0xaa64, 0x5064, 0x6264] {
            let (header, mut mapped) = reloc_image(machine);
            assert_eq!(header.relocate(&mut mapped, 0x100000), Ok(1));
            assert_eq!(u64_at(&mapped, 0x1020), Some(0x101234));
        }
    }
    #[test]
    fn validation_never_applies_relocations_and_high_canonical_base_wraps() {
        let (mut header, mut mapped) = reloc_image(0x8664);
        let before = mapped.clone();
        assert_eq!(header.validate_relocations(&mut mapped, 0x100000), Ok(()));
        assert_eq!(mapped, before);
        header.image_base = 0xffff_f800_0000_0000;
        mapped[0x1020..0x1028].copy_from_slice(&0xffff_f800_0000_1234u64.to_le_bytes());
        assert_eq!(header.relocate(&mut mapped, 0x200000), Ok(1));
        assert_eq!(u64_at(&mapped, 0x1020), Some(0x201234));
    }

    #[test]
    fn a_slide_requires_an_explicit_relocation_directory() {
        let raw = image(0x8664);
        let header = Header::parse(&raw, 0x8664).unwrap();
        let mut mapped = vec![0; header.image_size];
        header.map(&raw, &mut mapped).unwrap();
        assert_eq!(header.relocate(&mut mapped, header.image_base), Ok(0));
        assert!(header.relocate(&mut mapped, 0x100000).is_err());
        assert!(header.validate_relocations(&mut mapped, 0x100000).is_err());
    }

    #[test]
    fn malformed_late_relocation_does_not_partially_write() {
        for entry in [0xafff, 0xa108, 0x7020] {
            let (header, mut mapped) = reloc_image(0x8664);
            put16(&mut mapped, 0x110a, entry);
            let before = mapped.clone();
            assert!(header.relocate(&mut mapped, 0x100000).is_err());
            assert_eq!(mapped, before);
        }
    }
    #[test]
    fn malformed_relocation_blocks_and_directory_are_rejected() {
        for block_size in [0, 7, 9, 14] {
            let (header, mut mapped) = reloc_image(0x8664);
            put32(&mut mapped, 0x1104, block_size);
            assert!(header.relocate(&mut mapped, 0x100000).is_err());
        }
        let (header, mut mapped) = reloc_image(0x8664);
        put32(&mut mapped, 0x130, 0x1fff);
        assert!(header.relocate(&mut mapped, 0x100000).is_err());
    }
    #[test]
    fn every_truncated_prefix_is_safe() {
        let raw = image(0x8664);
        for end in 0..raw.len() {
            assert!(Header::parse(&raw[..end], 0x8664).is_err());
        }
    }
}
