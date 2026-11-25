use alloc::vec::Vec;

pub const PAGE_SIZE: usize = 4096;
const MAX_TABLE_PAGES: usize = 16;
const MAX_DATA_PAGES: usize = 4096;
const ENTRY_SIZE: usize = 12;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PhysicalPages {
    pub base: u64,
    pub count: usize,
}

impl PhysicalPages {
    fn new(base: u64, count: usize) -> Option<Self> {
        if count == 0 || base == 0 || base % PAGE_SIZE as u64 != 0 {
            return None;
        }
        base.checked_add((count * PAGE_SIZE) as u64)?;
        Some(Self { base, count })
    }

    fn overlaps(self, other: Self) -> bool {
        self.base < other.base + (other.count * PAGE_SIZE) as u64
            && other.base < self.base + (self.count * PAGE_SIZE) as u64
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PersistentRange {
    pub pages: PhysicalPages,
    pub length: usize,
}

pub fn valid_record_header(header: &[u8], length: usize) -> bool {
    let Some(header) = header.get(..0x2c) else {
        return false;
    };
    let version = u32::from_le_bytes(header[0x20..0x24].try_into().unwrap());
    let data_length = u32::from_le_bytes(header[0x24..0x28].try_into().unwrap()) as usize;
    let data_offset = u32::from_le_bytes(header[0x28..0x2c].try_into().unwrap()) as usize;
    version == 1
        && data_length != 0
        && data_offset >= header.len()
        && data_offset.checked_add(data_length) == Some(length)
}

pub fn table_pages(arguments: &[u8]) -> Result<Option<PhysicalPages>, &'static str> {
    if arguments.len() != 0x1c || arguments[..4] != 1u32.to_le_bytes() {
        return Err("invalid Windows boot application return arguments");
    }
    let count = u64::from_le_bytes(arguments[0xc..0x14].try_into().unwrap());
    let frame = u64::from_le_bytes(arguments[0x14..0x1c].try_into().unwrap());
    if count == 0 && frame == 0 {
        return Ok(None);
    }
    if count > MAX_TABLE_PAGES as u64 {
        return Err("Windows persistent data table exceeds the page limit");
    }
    frame
        .checked_mul(PAGE_SIZE as u64)
        .and_then(|base| PhysicalPages::new(base, count as usize))
        .map(Some)
        .ok_or("invalid Windows persistent data table address")
}

pub fn data_pages(
    table: PhysicalPages,
    bytes: &[u8],
) -> Result<Vec<PersistentRange>, &'static str> {
    let count = bytes
        .get(..4)
        .map(|value| u32::from_le_bytes(value.try_into().unwrap()))
        .ok_or("truncated Windows persistent data table")? as usize;
    if count > 64 || bytes.len() != table.count * PAGE_SIZE || 4 + count * ENTRY_SIZE > bytes.len()
    {
        return Err("invalid Windows persistent data entry count");
    }
    let mut total = 0;
    let mut ranges: Vec<PersistentRange> = Vec::with_capacity(count);
    for entry in bytes[4..4 + count * ENTRY_SIZE].chunks_exact(ENTRY_SIZE) {
        let base = u64::from_le_bytes(entry[..8].try_into().unwrap());
        let length = u32::from_le_bytes(entry[8..].try_into().unwrap()) as usize;
        let count = length.div_ceil(PAGE_SIZE);
        total += count;
        if length < 0x2c || total > MAX_DATA_PAGES {
            return Err("invalid Windows persistent data entry size");
        }
        let range =
            PhysicalPages::new(base, count).ok_or("invalid Windows persistent data address")?;
        if range.overlaps(table) || ranges.iter().any(|previous| previous.pages.overlaps(range)) {
            return Err("overlapping Windows persistent data ranges");
        }
        ranges.push(PersistentRange {
            pages: range,
            length,
        });
    }
    Ok(ranges)
}
