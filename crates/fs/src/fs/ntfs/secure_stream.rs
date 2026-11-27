//! Bounded resident `$Secure:$SDS` updates for the bootstrap volume.
//! Standard nonresident `$SDS` reads use the `$SII` index.
extern crate alloc;
use alloc::vec::Vec;

#[path = "secure_append.rs"]
pub(super) mod append;

const NAME: &[u8] = b"$SDS";
const BLOCK_SIZE: usize = 256 * 1024;

pub(crate) fn attribute(record: &[u8]) -> Option<(usize, &[u8])> {
    named_attribute(record, 0x80, b"$\0S\0D\0S\0")
}

pub(crate) fn named_attribute<'a>(record: &'a [u8], kind: u32, name: &[u8; 8]) -> Option<(usize, &'a [u8])> {
    let mut at = u16::from_le_bytes(record.get(0x14..0x16)?.try_into().ok()?) as usize;
    let mut found = None;
    loop {
        let attr_kind = u32::from_le_bytes(record.get(at..at + 4)?.try_into().ok()?);
        if attr_kind == u32::MAX { return found; }
        let length = u32::from_le_bytes(record.get(at + 4..at + 8)?.try_into().ok()?) as usize;
        if length < 24 { return None; }
        let attr = record.get(at..at.checked_add(length)?)?;
        let name_offset = u16::from_le_bytes(attr[10..12].try_into().ok()?) as usize;
        if attr_kind == kind && attr[9] == 4 && name_offset >= 24
            && attr.get(name_offset..name_offset + name.len()) == Some(name)
            && found.replace((at, attr)).is_some() {
            return None;
        }
        at += length;
    }
}

pub fn stream(record: &[u8]) -> Option<(usize, usize, &[u8])> {
    let (offset, attr) = attribute(record)?;
    Some((offset, attr.len(), resident_value(attr)?))
}

fn resident_value(attr: &[u8]) -> Option<&[u8]> {
    if attr.len() < 24 || attr[8] != 0 || attr[12..14] != [0, 0] { return None; }
    let name_offset = u16::from_le_bytes(attr[10..12].try_into().ok()?) as usize;
    let size = u32::from_le_bytes(attr[16..20].try_into().ok()?) as usize;
    let start = u16::from_le_bytes(attr[20..22].try_into().ok()?) as usize;
    if name_offset < 24 || start < name_offset + NAME.len() * 2 { return None; }
    attr.get(start..start.checked_add(size)?)
}

pub use super::bootstrap_security::sds_hash as hash;

/// NTFS SDS length includes its 20-byte header; entries are 16-byte aligned.
pub fn entry(id: u32, offset: u64, descriptor: &[u8]) -> Option<Vec<u8>> {
    let length = 20usize.checked_add(descriptor.len())?;
    let length32 = u32::try_from(length).ok()?;
    let mut out = alloc::vec![0; (length.checked_add(15)?) & !15];
    out[..4].copy_from_slice(&hash(descriptor).to_le_bytes());
    out[4..8].copy_from_slice(&id.to_le_bytes());
    out[8..16].copy_from_slice(&offset.to_le_bytes());
    out[16..20].copy_from_slice(&length32.to_le_bytes());
    out[20..length].copy_from_slice(descriptor);
    Some(out)
}

pub fn descriptor(data: &[u8], id: u32) -> Option<&[u8]> {
    scan(data, id).flatten()
}

const SII: &[u8; 8] = b"$\0S\0I\0I\0";

enum IndexMatch {
    Entry([u8; 20]),
    Child(u64),
    Missing,
}

fn index_entry(
    record: &[u8],
    id: u32,
    mut read_index_page: impl FnMut(u64, usize) -> Option<Vec<u8>>,
) -> Option<[u8; 20]> {
    let (_, root_attr) = named_attribute(record, 0x90, SII)?;
    let root = resident_value(root_attr)?;
    if root.get(..8)? != [0, 0, 0, 0, 0x10, 0, 0, 0] { return None; }
    let page_size = u32::from_le_bytes(root.get(8..12)?.try_into().ok()?) as usize;
    if !(512..=65536).contains(&page_size) || !page_size.is_power_of_two() { return None; }
    let mut result = find_index_key(root.get(16..)?, id)?;
    for _ in 0..32 {
        match result {
            IndexMatch::Entry(header) => return Some(header),
            IndexMatch::Missing => return None,
            IndexMatch::Child(vcn) => {
                let page = read_index_page(vcn, page_size)?;
                if page.len() != page_size || page.get(..4)? != b"INDX"
                    || page.get(16..24)? != vcn.to_le_bytes()
                { return None; }
                result = find_index_key(page.get(24..)?, id)?;
            }
        }
    }
    None
}

fn find_index_key(node: &[u8], id: u32) -> Option<IndexMatch> {
    let first = u32::from_le_bytes(node.get(..4)?.try_into().ok()?) as usize;
    let used = u32::from_le_bytes(node.get(4..8)?.try_into().ok()?) as usize;
    let allocated = u32::from_le_bytes(node.get(8..12)?.try_into().ok()?) as usize;
    let node_flags = *node.get(12)?;
    if first < 16 || first % 8 != 0 || used > allocated || allocated > node.len() || node_flags > 1 {
        return None;
    }
    let mut entries = node.get(first..used)?;
    let mut result = None;
    let mut previous = None;
    loop {
        let length = u16::from_le_bytes(entries.get(8..10)?.try_into().ok()?) as usize;
        let key_length = u16::from_le_bytes(entries.get(10..12)?.try_into().ok()?) as usize;
        let flags = u16::from_le_bytes(entries.get(12..14)?.try_into().ok()?);
        if length < 16 || length % 8 != 0 || flags & !3 != 0 || flags & 1 != u16::from(node_flags) {
            return None;
        }
        let (entry, rest) = entries.split_at_checked(length)?;
        entries = rest;
        let child = if flags & 1 != 0 {
            if length < 24 { return None; }
            IndexMatch::Child(u64::from_le_bytes(entry[length - 8..].try_into().ok()?))
        } else {
            IndexMatch::Missing
        };
        if flags & 2 != 0 {
            return (entries.is_empty() && key_length == 0 && length == 16 + 8 * usize::from(node_flags))
                .then(|| result.unwrap_or(child));
        }
        let payload_end = if flags & 1 == 0 { length } else { length - 8 };
        let data_offset = u16::from_le_bytes(entry[..2].try_into().ok()?) as usize;
        let data_size = u16::from_le_bytes(entry[2..4].try_into().ok()?) as usize;
        if key_length != 4 || data_size != 20 || data_offset < 20 || data_offset + data_size > payload_end {
            return None;
        }
        let key = u32::from_le_bytes(entry[16..20].try_into().ok()?);
        let header = &entry[data_offset..data_offset + data_size];
        if previous.is_some_and(|last| last >= key) || header[4..8] != key.to_le_bytes() {
            return None;
        }
        previous = Some(key);
        if result.is_none() && id <= key {
            result = Some(if id == key { IndexMatch::Entry(header.try_into().ok()?) } else { child });
        }
    }
}

pub(crate) fn read_indexed(
    record: &[u8],
    sds: &[u8],
    id: u32,
    cluster_size: u64,
    mut read_range: impl FnMut(&[u8], u64, &mut [u8]) -> Option<()>,
    fixup: impl Fn(&mut [u8]) -> bool,
) -> Option<Vec<u8>> {
    let header = index_entry(record, id, |vcn, page_size| {
        let (_, allocation) = named_attribute(record, 0xa0, SII)?;
        let vcn_unit = if cluster_size <= page_size as u64 { cluster_size } else { 512 };
        let offset = vcn.checked_mul(vcn_unit)?;
        if vcn_unit == 0 || !offset.is_multiple_of(page_size as u64) { return None; }
        let mut page = alloc::vec![0; page_size];
        read_range(allocation, offset, &mut page)?;
        fixup(&mut page).then_some(page)
    })?;
    indexed_descriptor(&header, |offset, output| read_range(sds, offset, output))
}

fn indexed_descriptor(
    header: &[u8; 20],
    read_sds: impl FnOnce(u64, &mut [u8]) -> Option<()>,
) -> Option<Vec<u8>> {
    let offset = u64::from_le_bytes(header[8..16].try_into().ok()?);
    let length = u32::from_le_bytes(header[16..20].try_into().ok()?) as usize;
    let block_offset = (offset % (2 * BLOCK_SIZE) as u64) as usize;
    if !offset.is_multiple_of(16) || length < 40 || block_offset.checked_add(length)? > BLOCK_SIZE {
        return None;
    }
    let mut bytes = alloc::vec![0; length];
    read_sds(offset, &mut bytes)?;
    if bytes[..20] != *header || hash(&bytes[20..]).to_le_bytes() != header[..4] {
        return None;
    }
    bytes.drain(..20);
    Some(bytes)
}

fn scan(data: &[u8], id: u32) -> Option<Option<&[u8]>> {
    let mut found = None;
    for_each_descriptor(data, |entry_id, bytes| {
        if entry_id == id && found.replace(bytes).is_some() { return None; }
        Some(())
    })?;
    Some(found)
}

fn for_each_descriptor<'a>(data: &'a [u8], mut visit: impl FnMut(u32, &'a [u8]) -> Option<()>) -> Option<()> {
    let mut at = 0usize;
    while at < data.len() {
        let remaining = &data[at..];
        let header = remaining.get(..20)?;
        let length = u32::from_le_bytes(header[16..20].try_into().ok()?) as usize;
        let offset = u64::from_le_bytes(header[8..16].try_into().ok()?);
        let next = length.checked_add(15)? & !15;
        if offset != at as u64 || next > remaining.len() {
            return None;
        }
        let bytes = remaining.get(20..length)?;
        if hash(bytes) != u32::from_le_bytes(header[..4].try_into().ok()?) {
            return None;
        }
        visit(u32::from_le_bytes(header[4..8].try_into().ok()?), bytes)?;
        at += next;
    }
    Some(())
}

pub(super) fn intern_resident(record: &[u8], descriptor: &[u8]) -> Option<(u32, Option<Vec<u8>>)> {
    let (_, _, data) = stream(record)?;
    let mut highest = 255;
    let mut found = None;
    for_each_descriptor(data, |id, bytes| {
        if id <= highest { return None; }
        highest = id;
        if bytes == descriptor { found.get_or_insert(id); }
        Some(())
    })?;
    if let Some(id) = found { return Some((id, None)); }
    let id = highest.checked_add(1)?;
    Some((id, Some(upsert(record, id, descriptor)?)))
}

pub fn new_record(size: usize) -> Option<Vec<u8>> {
    if !(512..=4096).contains(&size) || !size.is_multiple_of(512) { return None; }
    let fixups = size / 512 + 1;
    let start = (48 + fixups * 2 + 7) & !7;
    let mut record = alloc::vec![0; size];
    record[..4].copy_from_slice(b"FILE");
    record[4..6].copy_from_slice(&48u16.to_le_bytes());
    record[6..8].copy_from_slice(&(fixups as u16).to_le_bytes());
    record[16..18].copy_from_slice(&1u16.to_le_bytes());
    record[18..20].copy_from_slice(&1u16.to_le_bytes());
    record[20..22].copy_from_slice(&(start as u16).to_le_bytes());
    record[22..24].copy_from_slice(&1u16.to_le_bytes());
    record[24..28].copy_from_slice(&((start + 8) as u32).to_le_bytes());
    record[28..32].copy_from_slice(&(size as u32).to_le_bytes());
    record[44..48].copy_from_slice(&9u32.to_le_bytes());
    record[start..start + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    Some(record)
}

/// Keep the record size, header, fixups and unrelated attributes intact.
/// Identical defaults are shared by all newly created files.
pub fn upsert(record: &[u8], id: u32, sd: &[u8]) -> Option<Vec<u8>> {
    let existing = stream(record);
    if let Some((_, _, data)) = existing {
        if let Some(old) = scan(data, id)? {
            return (old == sd).then(|| record.to_vec());
        }
    }
    let mut data = existing.map_or_else(Vec::new, |(_, _, value)| value.to_vec());
    data.extend_from_slice(&entry(id, data.len() as u64, sd)?);
    let mut attribute = alloc::vec![0; (32 + data.len() + 7) & !7];
    let attr_length = attribute.len();
    attribute[..4].copy_from_slice(&0x80u32.to_le_bytes());
    attribute[4..8].copy_from_slice(&(attr_length as u32).to_le_bytes());
    attribute[9] = 4;
    attribute[10..12].copy_from_slice(&24u16.to_le_bytes());
    let next_id = u16::from_le_bytes(record.get(40..42)?.try_into().ok()?);
    let attr_id = existing.map_or(next_id, |(at, _, _)| u16::from_le_bytes([record[at + 14], record[at + 15]]));
    attribute[14..16].copy_from_slice(&attr_id.to_le_bytes());
    attribute[16..20].copy_from_slice(&(data.len() as u32).to_le_bytes());
    attribute[20..22].copy_from_slice(&32u16.to_le_bytes());
    for (i, b) in NAME.iter().enumerate() { attribute[24 + i * 2] = *b; }
    attribute[32..32 + data.len()].copy_from_slice(&data);
    let start = u16::from_le_bytes(record.get(20..22)?.try_into().ok()?) as usize;
    let mut rebuilt = record.get(..start)?.to_vec();
    let mut at = start;
    loop {
        let header = record.get(at..at + 8)?;
        let kind = u32::from_le_bytes(header[..4].try_into().ok()?);
        if kind == u32::MAX { break; }
        let length = u32::from_le_bytes(header[4..].try_into().ok()?) as usize;
        if length < 24 { return None; }
        let attr = record.get(at..at.checked_add(length)?)?;
        // Do not create a second SDS attribute when the existing one is
        // nonresident or malformed; this bounded writer cannot grow it.
        if existing.is_none() && kind == 0x80 && attr[9] == 4 {
            let name_at = u16::from_le_bytes([attr[10], attr[11]]) as usize;
            if attr.get(name_at..name_at.checked_add(8)?) == Some(&[36, 0, 83, 0, 68, 0, 83, 0]) {
                return None;
            }
        }
        if existing.is_none_or(|(old, _, _)| old != at) { rebuilt.extend_from_slice(attr); }
        at += length;
    }
    rebuilt.extend_from_slice(&attribute);
    rebuilt.extend_from_slice(&[255, 255, 255, 255, 0, 0, 0, 0]);
    if rebuilt.len() > record.len() { return None; }
    let used = rebuilt.len() as u32;
    rebuilt[24..28].copy_from_slice(&used.to_le_bytes());
    if existing.is_none() { rebuilt[40..42].copy_from_slice(&next_id.checked_add(1)?.to_le_bytes()); }
    rebuilt.resize(record.len(), 0);
    Some(rebuilt)
}

#[cfg(test)] mod tests {
    use super::*;

    #[test]
    fn resident_intern_deduplicates_without_replacing_the_default() {
        let original = upsert(&new_record(1024).unwrap(), 256, &[1; 80]).unwrap();
        assert_eq!(intern_resident(&original, &[1; 80]), Some((256, None)));
        let (id, record) = intern_resident(&original, &[2; 80]).unwrap();
        let record = record.unwrap();
        let data = stream(&record).unwrap().2;
        assert_eq!(id, 257);
        assert_eq!(descriptor(data, 256), Some(&[1; 80][..]));
        assert_eq!(descriptor(data, 257), Some(&[2; 80][..]));
        assert_eq!(intern_resident(&record, &[2; 80]), Some((257, None)));
        assert_eq!(intern_resident(&record, &[3; 1024]), None);
    }
    #[test] fn repeated_file_creation_keeps_valid_record_and_one_default() {
        for size in [1024, 4096] {
            let original = new_record(size).unwrap();
            let updated = upsert(&original, 256, &[1; 120]).unwrap();
            assert_eq!(updated.len(), size);
            assert_eq!(&updated[..24], &original[..24]);
            let (_, _, data) = stream(&updated).unwrap();
            assert_eq!(descriptor(data, 256), Some(&[1; 120][..]));
            assert_eq!(u32::from_le_bytes(data[16..20].try_into().unwrap()), 140);
            for _ in 0..1000 { assert_eq!(upsert(&updated, 256, &[1; 120]).unwrap(), updated); }
            assert!(upsert(&updated, 256, &[2; 120]).is_none());
        }
    }
    #[test] fn adding_descriptor_keeps_existing_entries_and_offsets() {
        let record = upsert(&new_record(1024).unwrap(), 256, &[1; 80]).unwrap();
        let updated = upsert(&record, 257, &[2; 100]).unwrap();
        let (_, _, data) = stream(&updated).unwrap();
        assert_eq!(descriptor(data, 256), Some(&[1; 80][..]));
        assert_eq!(descriptor(data, 257), Some(&[2; 100][..]));
        assert_eq!(u64::from_le_bytes(data[120..128].try_into().unwrap()), 112);
        assert!(upsert(&updated, 258, &[0; 1024]).is_none());
    }
    #[test] fn corrupt_streams_fail_without_panicking() {
        assert!(stream(&[]).is_none());
        assert!(upsert(&[0; 128], 256, &[1; 80]).is_none());
        let mut data = entry(256, 0, &[1; 80]).unwrap();
        data[20] ^= 1;
        assert!(descriptor(&data, 256).is_none());
    }

    #[test]
    fn corrupted_descriptor_is_never_treated_as_an_available_id() {
        let mut record = upsert(&new_record(1024).unwrap(), 256, &[1; 80]).unwrap();
        let (offset, _, _) = stream(&record).unwrap();
        record[offset + 32 + 20] ^= 1;
        assert!(upsert(&record, 256, &[1; 80]).is_none());
        assert!(upsert(&record, 257, &[2; 80]).is_none());
    }

    #[test]
    fn invalid_offsets_duplicate_ids_and_truncation_fail_validation() {
        let mut data = entry(256, 0, &[1; 80]).unwrap();
        data[8] = 16;
        assert!(descriptor(&data, 256).is_none());
        data[8] = 0;
        data.extend_from_slice(&entry(256, data.len() as u64, &[2; 80]).unwrap());
        assert!(descriptor(&data, 256).is_none());
        data.pop();
        assert!(descriptor(&data, 256).is_none());
    }

    #[test]
    fn reads_indexed_entry_past_old_scan_limit_and_detects_changed_bytes() {
        let offset = 32 * BLOCK_SIZE as u64;
        let mut bytes = entry(257, offset, &[2; 80]).unwrap();
        let header: [u8; 20] = bytes[..20].try_into().unwrap();
        let read = |at, dest: &mut [u8]| {
            assert_eq!(at, offset);
            assert_eq!(dest.len(), 100);
            dest.copy_from_slice(&bytes[..dest.len()]);
            Some(())
        };
        assert_eq!(indexed_descriptor(&header, read), Some(alloc::vec![2; 80]));
        bytes[20] ^= 1;
        assert!(indexed_descriptor(&header, |_, dest| {
            dest.copy_from_slice(&bytes[..dest.len()]);
            Some(())
        }).is_none());
    }

    #[test]
    fn security_index_descends_by_key_and_rejects_bad_child() {
        let mut record = new_record(1024).unwrap();
        let first = u16::from_le_bytes(record[20..22].try_into().unwrap()) as usize;
        let root = &mut record[first..first + 88];
        root[..4].copy_from_slice(&0x90u32.to_le_bytes());
        root[4..8].copy_from_slice(&88u32.to_le_bytes());
        root[9] = 4;
        root[10] = 24;
        root[16..20].copy_from_slice(&56u32.to_le_bytes());
        root[20] = 32;
        root[24..32].copy_from_slice(SII);
        root[36] = 0x10;
        root[40..44].copy_from_slice(&512u32.to_le_bytes());
        root[48] = 16;
        root[52] = 40;
        root[56] = 40;
        root[60] = 1;
        root[72] = 24;
        root[76] = 3;
        root[80] = 8;
        record[first + 88..first + 92].copy_from_slice(&u32::MAX.to_le_bytes());

        let mut page = alloc::vec![0; 512];
        page[..4].copy_from_slice(b"INDX");
        page[16] = 8;
        page[24] = 16;
        page[28] = 72;
        page[32..36].copy_from_slice(&488u32.to_le_bytes());
        let bytes = entry(257, 128, &[2; 80]).unwrap();
        page[40] = 20;
        page[42] = 20;
        page[48] = 40;
        page[50] = 4;
        page[56..60].copy_from_slice(&257u32.to_le_bytes());
        page[60..80].copy_from_slice(&bytes[..20]);
        page[88] = 16;
        page[92] = 2;

        let read_page = |vcn, size| {
            assert_eq!((vcn, size), (8, 512));
            Some(page.clone())
        };
        assert_eq!(index_entry(&record, 257, read_page).unwrap(), bytes[..20]);
        assert!(index_entry(&record, 256, read_page).is_none());
        assert!(index_entry(&record, 258, read_page).is_none());
        page[16] = 9;
        assert!(index_entry(&record, 257, |_, _| Some(page.clone())).is_none());
        page[16] = 8;
        page[48] = 32;
        assert!(index_entry(&record, 257, |_, _| Some(page.clone())).is_none());
    }

    #[test]
    fn selects_named_data_after_an_unnamed_attribute() {
        let mut record = new_record(1024).unwrap();
        let start = u16::from_le_bytes(record[20..22].try_into().unwrap()) as usize;
        record[start..start + 4].copy_from_slice(&0x80u32.to_le_bytes());
        record[start + 4..start + 8].copy_from_slice(&24u32.to_le_bytes());
        record[start + 24..start + 28].copy_from_slice(&u32::MAX.to_le_bytes());
        let mut record = upsert(&record, 256, &[1; 80]).unwrap();
        let (offset, attr) = attribute(&record).unwrap();
        assert_eq!(offset, start + 24);
        let len = attr.len();
        record[offset + 8] = 1;
        assert_eq!(attribute(&record).unwrap().1.len(), len);
        assert!(stream(&record).is_none());
        assert!(upsert(&record, 257, &[2; 80]).is_none());
    }
}
