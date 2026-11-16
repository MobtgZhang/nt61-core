//! Attribute queries read persistent NTFS metadata rather than inventing times.
use super::*;
use crate::mm::user_copy::{
    copy_to_user, probe_user_write, read_user_object, read_user_unicode_string,
};

fn query(
    attributes: *mut super::super::types::ObjectAttributes,
    output: u64,
    full: bool,
) -> NTSTATUS {
    (|| {
        let length = if full { 56 } else { 40 };
        probe_user_write(output, length)?;
        let attributes =
            read_user_object::<super::super::types::ObjectAttributes>(attributes as u64)?;
        if attributes.length as usize
            != core::mem::size_of::<super::super::types::ObjectAttributes>()
        {
            return Err(STATUS_INVALID_PARAMETER);
        }
        // Relative FILE_OBJECT roots require an I/O-manager name capture path.
        if !attributes.root_directory.is_null() {
            return Err(super::super::status::STATUS_NOT_SUPPORTED);
        }
        let mut path = read_user_unicode_string(attributes.object_name as u64, 65534)?;
        if path.starts_with(&[92, 63, 63, 92]) {
            path.drain(..4);
        }
        if !check_file_access(&path, 0x80) {
            return Err(STATUS_ACCESS_DENIED);
        }
        let fs = crate::fs::ntfs::get_mounted_fs().ok_or(STATUS_DEVICE_NOT_READY)?;
        let file = crate::fs::ntfs::open_file(fs, &path, None)
            .ok_or(super::super::status::STATUS_OBJECT_NAME_NOT_FOUND)?;
        let record = crate::fs::ntfs::read_mft_record(&fs.ntfs_data, file.mft_record)
            .ok_or(super::super::status::STATUS_UNSUCCESSFUL)?;
        let info = crate::fs::ntfs::parse_attribute(&record, 0x10)
            .filter(|bytes| bytes.len() >= 36)
            .ok_or(super::super::status::STATUS_UNSUCCESSFUL)?;
        let mut bytes = [0u8; 56];
        bytes[..8].copy_from_slice(&info[..8]);
        bytes[8..16].copy_from_slice(&info[24..32]); // last access
        bytes[16..24].copy_from_slice(&info[8..16]); // last write
        bytes[24..32].copy_from_slice(&info[16..24]); // MFT change
        if full {
            let allocation = crate::fs::ntfs::find_attribute_in_record(&record, 0x80)
                .and_then(|offset| {
                    let header = record.get(offset..)?;
                    if *header.get(8)? != 0 {
                        Some(u64::from_le_bytes(header.get(40..48)?.try_into().ok()?))
                    } else {
                        // Resident allocation is the eight-byte aligned value
                        // capacity within the MFT attribute, not file contents.
                        let total = u32::from_le_bytes(header.get(4..8)?.try_into().ok()?);
                        let value_offset = u16::from_le_bytes(header.get(20..22)?.try_into().ok()?);
                        Some(u64::from(total.checked_sub(u32::from(value_offset))?))
                    }
                })
                .unwrap_or(0);
            bytes[32..40].copy_from_slice(&allocation.to_le_bytes());
            bytes[40..48].copy_from_slice(&file.file_size.to_le_bytes());
            bytes[48..52].copy_from_slice(&info[32..36]);
        } else {
            bytes[32..36].copy_from_slice(&info[32..36]);
        }
        copy_to_user(output, &bytes[..length])?;
        Ok(STATUS_SUCCESS)
    })()
    .unwrap_or_else(|status| status)
}

pub extern "C" fn nt_query_attributes_file(
    attributes: *mut super::super::types::ObjectAttributes,
    output: PVOID,
) -> NTSTATUS {
    query(attributes, output as u64, false)
}

pub extern "C" fn nt_query_full_attributes_file(
    attributes: *mut super::super::types::ObjectAttributes,
    output: PVOID,
) -> NTSTATUS {
    query(attributes, output as u64, true)
}
