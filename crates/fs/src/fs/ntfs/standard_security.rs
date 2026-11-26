//! File-record association with the volume's security descriptor store.

const LEGACY_LENGTH: usize = 48;
pub(super) const VALUE_LENGTH: usize = 72;
const ID_OFFSET: usize = 52;

pub(super) fn new_value(id: u32) -> [u8; VALUE_LENGTH] {
    let mut value = [0; VALUE_LENGTH];
    value[ID_OFFSET..ID_OFFSET + 4].copy_from_slice(&id.to_le_bytes());
    value
}

struct StandardInformation {
    attribute: usize,
    attribute_end: usize,
    value: usize,
    value_length: usize,
    used: usize,
    has_attribute_list: bool,
    inline_security: Option<(usize, usize)>,
}

fn inline_value(attr: &[u8]) -> Option<&[u8]> {
    if attr[8..10] != [0; 2] || attr[12..14] != [0; 2] {
        return None;
    }
    let length = u32::from_le_bytes(attr[16..20].try_into().ok()?) as usize;
    let offset = usize::from(u16::from_le_bytes(attr[20..22].try_into().ok()?));
    if length < 20 || offset < 24 {
        return None;
    }
    attr.get(offset..offset.checked_add(length)?)
}

fn locate(record: &[u8]) -> Option<StandardInformation> {
    if record.get(..4)? != b"FILE" || record.get(22)? & 1 == 0 || record.get(32..40)? != [0; 8] {
        return None;
    }
    let used = u32::from_le_bytes(record.get(24..28)?.try_into().ok()?) as usize;
    let used_record = record.get(..used)?;
    let mut offset = usize::from(u16::from_le_bytes(record.get(20..22)?.try_into().ok()?));
    let fixup_offset = usize::from(u16::from_le_bytes(record.get(4..6)?.try_into().ok()?));
    let fixup_count = usize::from(u16::from_le_bytes(record.get(6..8)?.try_into().ok()?));
    if offset < 48.max(fixup_offset + 2 * fixup_count) || offset % 8 != 0 {
        return None;
    }

    let mut found = None;
    let mut has_attribute_list = false;
    let mut inline_security = None;
    loop {
        let kind = u32::from_le_bytes(used_record.get(offset..offset + 4)?.try_into().ok()?);
        if kind == u32::MAX {
            let mut info: StandardInformation = found?;
            info.has_attribute_list = has_attribute_list;
            info.inline_security = inline_security;
            return Some(info);
        }
        let length =
            u32::from_le_bytes(used_record.get(offset + 4..offset + 8)?.try_into().ok()?) as usize;
        if length < 24 || length % 8 != 0 {
            return None;
        }
        let end = offset.checked_add(length)?;
        let attr = used_record.get(offset..end)?;
        if kind == 0x10 {
            if found.is_some() || attr[8..10] != [0; 2] || attr[12..14] != [0; 2] {
                return None;
            }
            let value_length = u32::from_le_bytes(attr[16..20].try_into().ok()?) as usize;
            let value_offset = usize::from(u16::from_le_bytes(attr[20..22].try_into().ok()?));
            if !matches!(value_length, LEGACY_LENGTH | VALUE_LENGTH)
                || value_offset < 24
                || value_offset + value_length > length
            {
                return None;
            }
            found = Some(StandardInformation {
                attribute: offset,
                attribute_end: end,
                value: offset + value_offset,
                value_length,
                used,
                has_attribute_list: false,
                inline_security: None,
            });
        } else if kind == 0x20 {
            has_attribute_list = true;
        } else if kind == 0x50 {
            if found.is_none() || inline_security.is_some() {
                return None;
            }
            inline_value(attr)?;
            inline_security = Some((offset, end));
        }
        offset = end;
    }
}

pub(super) fn inline_descriptor(record: &[u8]) -> Option<&[u8]> {
    let info = locate(record)?;
    if info.value_length != LEGACY_LENGTH || info.has_attribute_list {
        return None;
    }
    let (start, end) = info.inline_security?;
    inline_value(&record[start..end])
}

pub(super) fn security_id(record: &[u8]) -> Option<u32> {
    let info = locate(record)?;
    if info.value_length != VALUE_LENGTH || info.inline_security.is_some() {
        return None;
    }
    let offset = info.value + ID_OFFSET;
    let id = u32::from_le_bytes(record[offset..offset + 4].try_into().ok()?);
    (id != 0).then_some(id)
}

pub(super) fn set_security_id(record: &mut [u8], id: u32) -> Option<()> {
    let info = locate(record)?;
    if id == 0 || info.has_attribute_list {
        return None;
    }
    let mut used = info.used;
    let new_end = info
        .attribute_end
        .max((info.value + VALUE_LENGTH).next_multiple_of(8));
    let growth = new_end - info.attribute_end;
    if let Some((start, end)) = info.inline_security {
        if info.value_length != LEGACY_LENGTH {
            return None;
        }
        used -= end - start;
        if used.checked_add(growth)? > record.len() {
            return None;
        }
        record.copy_within(end..info.used, start);
        record[used..info.used].fill(0);
    }
    if info.value_length == LEGACY_LENGTH {
        let value_end = info.value + VALUE_LENGTH;
        let new_used = used.checked_add(growth)?;
        if new_used > record.len() {
            return None;
        }
        record.copy_within(info.attribute_end..used, new_end);
        record[info.value + LEGACY_LENGTH..value_end].fill(0);
        record[info.attribute + 4..info.attribute + 8]
            .copy_from_slice(&((new_end - info.attribute) as u32).to_le_bytes());
        record[info.attribute + 16..info.attribute + 20]
            .copy_from_slice(&(VALUE_LENGTH as u32).to_le_bytes());
        record[24..28].copy_from_slice(&(new_used as u32).to_le_bytes());
    }
    let offset = info.value + ID_OFFSET;
    record[offset..offset + 4].copy_from_slice(&id.to_le_bytes());
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resident_record(value_length: usize, extra_kind: u32) -> [u8; 512] {
        let mut record = [0; 512];
        record[..4].copy_from_slice(b"FILE");
        record[4..6].copy_from_slice(&48u16.to_le_bytes());
        record[6..8].copy_from_slice(&2u16.to_le_bytes());
        record[20..22].copy_from_slice(&56u16.to_le_bytes());
        record[22] = 1;
        let standard = &mut record[56..80 + value_length];
        standard[..4].copy_from_slice(&0x10u32.to_le_bytes());
        standard[4..8].copy_from_slice(&((24 + value_length) as u32).to_le_bytes());
        standard[16..20].copy_from_slice(&(value_length as u32).to_le_bytes());
        standard[20..22].copy_from_slice(&24u16.to_le_bytes());
        standard[24..72].fill(0xa5);
        let extra = 80 + value_length;
        record[extra..extra + 4].copy_from_slice(&extra_kind.to_le_bytes());
        record[extra + 4..extra + 8].copy_from_slice(&32u32.to_le_bytes());
        record[extra + 24..extra + 32].copy_from_slice(b"contents");
        record[extra + 32..extra + 36].copy_from_slice(&u32::MAX.to_le_bytes());
        record[24..28].copy_from_slice(&((extra + 36) as u32).to_le_bytes());
        record
    }

    #[test]
    fn extends_legacy_value_and_preserves_other_attributes() {
        let mut record = resident_record(48, 0x80);
        let original = record;
        assert_eq!(security_id(&record), None);
        assert_eq!(set_security_id(&mut record, 257), Some(()));
        assert_eq!(security_id(&record), Some(257));
        assert_eq!(&record[80..128], &original[80..128]);
        assert_eq!(&record[152..188], &original[128..164]);
        assert_eq!(&record[24..28], &188u32.to_le_bytes());
        assert_eq!(&record[128..132], &[0; 4]);
        assert_eq!(&record[136..152], &[0; 16]);
    }

    #[test]
    fn replaces_only_the_security_id_in_an_extended_value() {
        let mut record = resident_record(72, 0x80);
        let original = record;
        assert_eq!(set_security_id(&mut record, 257), Some(()));
        assert_eq!(security_id(&record), Some(257));
        assert_eq!(&record[..132], &original[..132]);
        assert_eq!(&record[136..], &original[136..]);
    }

    #[test]
    fn unsupported_or_truncated_records_remain_unchanged() {
        for mut record in [resident_record(48, 0x20), resident_record(48, 0x50)] {
            let original = record;
            assert_eq!(set_security_id(&mut record, 257), None);
            assert_eq!(record, original);
        }
        let mut record = resident_record(48, 0x80);
        let original = record;
        assert_eq!(set_security_id(&mut record[..164], 257), None);
        assert_eq!(set_security_id(&mut record, 0), None);
        assert_eq!(record, original);
    }
}
