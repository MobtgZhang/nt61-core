use super::{
    attribute, entry, hash, indexed_descriptor, named_attribute, resident_value, BLOCK_SIZE,
};
use alloc::vec::Vec;

const HEADER_SIZE: usize = 20;
const SII: &[u8; 8] = b"$\0S\0I\0I\0";
const SDH: &[u8; 8] = b"$\0S\0D\0H\0";

pub(in super::super) enum Insertion {
    Existing(u32),
    Append {
        id: u32,
        mft_record: Vec<u8>,
        primary: u64,
        mirror: u64,
        bytes: Vec<u8>,
    },
}

struct Leaf<'a> {
    attribute_offset: usize,
    value_offset: usize,
    value: &'a [u8],
    entries: Vec<[u8; HEADER_SIZE]>,
    hash_order: bool,
}

fn u16_at(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

fn u32_at(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn u64_at(bytes: &[u8], offset: usize) -> Option<u64> {
    Some(u64::from_le_bytes(
        bytes.get(offset..offset + 8)?.try_into().ok()?,
    ))
}

fn key(header: &[u8; HEADER_SIZE], hash_order: bool) -> (u32, u32) {
    let hash = if hash_order {
        u32_at(header, 0).unwrap()
    } else {
        0
    };
    (hash, u32_at(header, 4).unwrap())
}

impl<'a> Leaf<'a> {
    fn parse(record: &'a [u8], hash_order: bool) -> Option<Self> {
        let name = if hash_order { SDH } else { SII };
        if named_attribute(record, 0xa0, name).is_some()
            || named_attribute(record, 0xb0, name).is_some()
        {
            return None;
        }
        let (attribute_offset, attr) = named_attribute(record, 0x90, name)?;
        let value_offset = usize::from(u16_at(attr, 20)?);
        let value = resident_value(attr)?;
        let collation = if hash_order { 0x12 } else { 0x10 };
        if u32_at(value, 0)? != 0
            || u32_at(value, 4)? != collation
            || u32_at(value, 16)? != 16
            || u32_at(value, 20)? as usize != value.len().checked_sub(16)?
            || u32_at(value, 24)? != u32_at(value, 20)?
            || u32_at(value, 28)? != 0
        {
            return None;
        }
        let mut entries = Vec::new();
        let mut remaining = value.get(32..)?;
        let key_length = if hash_order { 8 } else { 4 };
        let entry_length = if hash_order { 48 } else { 40 };
        loop {
            let flags = u16_at(remaining, 12)?;
            if flags == 2 {
                if remaining.len() != 16
                    || u16_at(remaining, 8)? != 16
                    || u16_at(remaining, 10)? != 0
                {
                    return None;
                }
                return Some(Self {
                    attribute_offset,
                    value_offset,
                    value,
                    entries,
                    hash_order,
                });
            }
            let data_offset = 16 + key_length;
            if flags != 0
                || usize::from(u16_at(remaining, 0)?) != data_offset
                || u16_at(remaining, 2)? != HEADER_SIZE as u16
                || usize::from(u16_at(remaining, 8)?) != entry_length
                || usize::from(u16_at(remaining, 10)?) != key_length
            {
                return None;
            }
            let header: [u8; HEADER_SIZE] = remaining
                .get(data_offset..data_offset + HEADER_SIZE)?
                .try_into()
                .ok()?;
            let (hash, id) = key(&header, hash_order);
            if u32_at(remaining, data_offset - 4)? != id
                || id < 256
                || (hash_order && u32_at(remaining, 16)? != hash)
                || entries
                    .last()
                    .is_some_and(|last| key(last, hash_order) >= (hash, id))
            {
                return None;
            }
            entries.push(header);
            remaining = remaining.get(entry_length..)?;
        }
    }

    fn insert(&self, record: &mut [u8], header: &[u8; HEADER_SIZE], used: usize) -> Option<usize> {
        let entry_length = if self.hash_order { 48 } else { 40 };
        let key_length = if self.hash_order { 8 } else { 4 };
        let new_key = key(header, self.hash_order);
        let index = self
            .entries
            .partition_point(|other| key(other, self.hash_order) < new_key);
        let start = self.attribute_offset + self.value_offset;
        let at = start + 32 + index * entry_length;
        let enlarged = used.checked_add(entry_length)?;
        if enlarged > record.len() || at > used {
            return None;
        }
        record.copy_within(at..used, at + entry_length);
        let target = &mut record[at..at + entry_length];
        target.fill(0);
        target[..2].copy_from_slice(&((16 + key_length) as u16).to_le_bytes());
        target[2..4].copy_from_slice(&(HEADER_SIZE as u16).to_le_bytes());
        target[8..10].copy_from_slice(&(entry_length as u16).to_le_bytes());
        target[10..12].copy_from_slice(&(key_length as u16).to_le_bytes());
        target[16..16 + key_length].copy_from_slice(&header[8 - key_length..8]);
        target[16 + key_length..16 + key_length + HEADER_SIZE].copy_from_slice(header);
        if self.hash_order {
            target[44..48].copy_from_slice(&0x0049_0049u32.to_le_bytes());
        }
        let attr_len = u32_at(record, self.attribute_offset + 4)? as usize;
        write_u32(record, self.attribute_offset + 4, attr_len + entry_length)?;
        let value_size = self.value.len() + entry_length;
        write_u32(record, self.attribute_offset + 16, value_size)?;
        write_u32(record, start + 20, value_size - 16)?;
        write_u32(record, start + 24, value_size - 16)?;
        Some(enlarged)
    }
}

fn write_u32(record: &mut [u8], offset: usize, value: usize) -> Option<()> {
    record
        .get_mut(offset..offset + 4)?
        .copy_from_slice(&u32::try_from(value).ok()?.to_le_bytes());
    Some(())
}

pub(in super::super) fn plan(
    record: &[u8],
    descriptor: &[u8],
    mut read: impl FnMut(&[u8], u64, &mut [u8]) -> Option<()>,
) -> Option<Insertion> {
    let used = u32_at(record, 24)? as usize;
    let first_attribute = usize::from(u16_at(record, 20)?);
    let fixup_end = usize::from(u16_at(record, 4)?) + 2 * usize::from(u16_at(record, 6)?);
    if record.get(..4)? != b"FILE"
        || used > record.len()
        || u32_at(record, 28)? as usize != record.len()
        || u64_at(record, 32)? != 0
        || u16_at(record, 22)? & 1 == 0
        || first_attribute < fixup_end.max(48)
        || !first_attribute.is_multiple_of(8)
        || u32_at(record, used.checked_sub(8)?)? != u32::MAX
    {
        return None;
    }
    let attributes = &record[..used];
    let mut cursor = first_attribute;
    loop {
        match u32_at(attributes, cursor)? {
            u32::MAX if cursor == used - 8 => break,
            u32::MAX | 0x20 => return None,
            _ => (),
        }
        let length = u32_at(attributes, cursor + 4)? as usize;
        if length < 24 || !length.is_multiple_of(8) {
            return None;
        }
        cursor = cursor.checked_add(length)?;
    }
    let (sds_offset, sds) = attribute(attributes)?;
    let sii = Leaf::parse(attributes, false)?;
    let sdh = Leaf::parse(attributes, true)?;
    if sds[8] != 1
        || sds_offset >= sii.attribute_offset
        || sds_offset >= sdh.attribute_offset
        || sii.entries.len() != sdh.entries.len()
        || !sii.entries.iter().all(|header| {
            sdh.entries
                .binary_search_by_key(&key(header, true), |other| key(other, true))
                .is_ok_and(|index| sdh.entries[index] == *header)
        })
    {
        return None;
    }
    let size = u64_at(sds, 48)?;
    if size != u64_at(sds, 56)? || size > u64_at(sds, 40)? {
        return None;
    }
    let wanted_hash = hash(descriptor);
    for header in &sdh.entries {
        let (header_hash, id) = key(header, true);
        if header_hash == wanted_hash {
            let stored = indexed_descriptor(header, |offset, out| read(sds, offset, out))?;
            if stored == descriptor {
                return Some(Insertion::Existing(id));
            }
        }
    }
    let last = sii.entries.last()?;
    let id = u32_at(last, 4)?.checked_add(1)?;
    let block = BLOCK_SIZE as u64;
    let primary = u64_at(last, 8)?.checked_add(u64::from(u32_at(last, 16)?))?;
    indexed_descriptor(last, |offset, out| read(sds, offset, out))?;
    indexed_descriptor(last, |offset, out| {
        read(sds, offset.checked_add(block)?, out)
    })?;
    if primary.checked_add(block)? != size || primary % (2 * block) >= block {
        return None;
    }
    let offset = primary.checked_add(15)? & !15;
    let mut bytes = entry(id, offset, descriptor)?;
    let length = u32_at(&bytes, 16)? as usize;
    let header: [u8; HEADER_SIZE] = bytes[..HEADER_SIZE].try_into().ok()?;
    bytes.truncate(length);
    let end = offset.checked_add(length as u64)?;
    let new_size = end.checked_add(block)?;
    if end > (primary / (2 * block)) * (2 * block) + block || new_size > u64_at(sds, 40)? {
        return None;
    }
    let mut rebuilt = record.to_vec();
    let (later, earlier) = if sii.attribute_offset > sdh.attribute_offset {
        (&sii, &sdh)
    } else {
        (&sdh, &sii)
    };
    let used = later.insert(&mut rebuilt, &header, used)?;
    let used = earlier.insert(&mut rebuilt, &header, used)?;
    write_u32(&mut rebuilt, 24, used)?;
    for field in [48, 56] {
        rebuilt[sds_offset + field..sds_offset + field + 8]
            .copy_from_slice(&new_size.to_le_bytes());
    }
    let mut padded = alloc::vec![0; (offset - primary) as usize];
    padded.extend_from_slice(&bytes);
    Some(Insertion::Append {
        id,
        mft_record: rebuilt,
        primary,
        mirror: size,
        bytes: padded,
    })
}
