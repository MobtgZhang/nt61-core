//! Bounded resident `$Secure:$SDS` updates for the bootstrap volume.
//! The full `$SII`/`$SDH` indexes and nonresident stream growth are separate work.
extern crate alloc;
use alloc::vec::Vec;

const NAME: &[u8] = b"$SDS";

pub fn stream(record: &[u8]) -> Option<(usize, usize, &[u8])> {
    let mut at = u16::from_le_bytes(record.get(0x14..0x16)?.try_into().ok()?) as usize;
    while at + 24 <= record.len() {
        let kind = u32::from_le_bytes(record[at..at + 4].try_into().ok()?);
        if kind == u32::MAX { return None; }
        let length = u32::from_le_bytes(record[at + 4..at + 8].try_into().ok()?) as usize;
        if length < 24 || length > record.len() - at { return None; }
        let attr = &record[at..at + length];
        let name_offset = u16::from_le_bytes(attr[10..12].try_into().ok()?) as usize;
        if kind == 0x80 && attr[9] as usize == NAME.len()
            && name_offset + NAME.len() * 2 <= length
            && NAME.iter().enumerate().all(|(i, b)| attr[name_offset + i * 2] == *b && attr[name_offset + i * 2 + 1] == 0) {
            if attr[8] != 0 { return None; }
            let size = u32::from_le_bytes(attr[16..20].try_into().ok()?) as usize;
            let start = u16::from_le_bytes(attr[20..22].try_into().ok()?) as usize;
            return Some((at, length, attr.get(start..start.checked_add(size)?)?));
        }
        at += length;
    }
    None
}

pub fn hash(descriptor: &[u8]) -> u32 {
    descriptor.chunks(4).fold(0u32, |sum, bytes| {
        let mut word = [0; 4];
        word[..bytes.len()].copy_from_slice(bytes);
        sum.rotate_left(3).wrapping_add(u32::from_le_bytes(word))
    })
}

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
    let mut at = 0usize;
    while at + 20 <= data.len() {
        let length = u32::from_le_bytes(data[at + 16..at + 20].try_into().ok()?) as usize;
        if length < 20 || length > data.len() - at { return None; }
        if u32::from_le_bytes(data[at + 4..at + 8].try_into().ok()?) == id {
            let bytes = &data[at + 20..at + length];
            return (hash(bytes) == u32::from_le_bytes(data[at..at + 4].try_into().ok()?)).then_some(bytes);
        }
        at = at.checked_add(length.checked_add(15)? & !15)?;
    }
    None
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
        if let Some(old) = descriptor(data, id) {
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
}
