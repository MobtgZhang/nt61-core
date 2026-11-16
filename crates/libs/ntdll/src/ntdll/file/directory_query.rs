//! Bounded directory-information encoding, independent of user pointers.
//!
//! These are wire offsets, not `size_of` values of structs containing a
//! one-element flexible array. Only gaps between records are eight-byte aligned.
//! `RawDirEntry` has no short name or EA size; those fields remain zero.
//! The directory type bit is retained for legacy producers without attributes;
//! file attribute masks otherwise come directly from the filesystem.
//! The NTFS enumeration FileIndex is undefined here and remains zero. The number
//! of complete records is a source position count, never an NT FileIndex token.
use alloc::vec::Vec;
use crate::fs::ntfs::RawDirEntry;
use crate::libs::ntdll::status::{
    STATUS_BUFFER_OVERFLOW, STATUS_INFO_LENGTH_MISMATCH, STATUS_INSUFFICIENT_RESOURCES,
    STATUS_INVALID_INFO_CLASS, STATUS_INVALID_PARAMETER, STATUS_SUCCESS,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposition {
    /// Every requested record was encoded, or `single` stopped after one.
    Complete,
    /// A following record did not fit. All emitted records are complete.
    MoreEntries,
    /// Only the first record's header and a prefix of its name fit.
    /// The caller must decide whether its query phase permits partial names.
    PartialName,
    /// Even the first record's fixed fields did not fit; nothing was emitted.
    HeaderTooSmall,
}

#[derive(Debug)]
pub struct DirectoryBuffer {
    pub bytes: Vec<u8>,
    /// Complete source records encoded. Does not include a partial first name.
    pub entries_written: usize,
    pub disposition: Disposition,
}

impl DirectoryBuffer {
    /// Empty input has SUCCESS here. The enumeration owner decides between
    /// NO_SUCH_FILE and NO_MORE_FILES using its captured search/cursor state.
    pub fn status(&self) -> i32 {
        match self.disposition {
            Disposition::Complete | Disposition::MoreEntries => STATUS_SUCCESS,
            Disposition::PartialName => STATUS_BUFFER_OVERFLOW,
            Disposition::HeaderTooSmall => STATUS_INFO_LENGTH_MISMATCH,
        }
    }
}

pub fn name_offset(class: u32) -> Result<usize, i32> {
    match class {
        1 => Ok(0x40),  // FILE_DIRECTORY_INFORMATION
        2 => Ok(0x44),  // FILE_FULL_DIR_INFORMATION
        3 => Ok(0x5e),  // FILE_BOTH_DIR_INFORMATION
        12 => Ok(0x0c), // FILE_NAMES_INFORMATION
        37 => Ok(0x68), // FILE_ID_BOTH_DIR_INFORMATION
        38 => Ok(0x50), // FILE_ID_FULL_DIR_INFORMATION
        _ => Err(STATUS_INVALID_INFO_CLASS),
    }
}

fn put_u32(out: &mut [u8], offset: usize, value: u32) {
    out[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn put_u64(out: &mut [u8], offset: usize, value: u64) {
    out[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn header(out: &mut [u8], raw: &RawDirEntry, class: u32) {
    let name_bytes = u32::from(raw.name_len) * 2;
    if class == 12 {
        put_u32(out, 8, name_bytes);
        return;
    }
    put_u64(out, 8, raw.creation_time as u64);
    put_u64(out, 0x10, raw.last_access_time as u64);
    put_u64(out, 0x18, raw.last_write_time as u64);
    put_u64(out, 0x20, raw.change_time as u64);
    put_u64(out, 0x28, raw.size);
    put_u64(out, 0x30, raw.alloc_size);
    put_u32(out, 0x38, raw.file_attributes | if raw.is_dir { 0x10 } else { 0 });
    put_u32(out, 0x3c, name_bytes);
    if class == 37 {
        put_u64(out, 0x60, raw.mft_ref);
    } else if class == 38 {
        put_u64(out, 0x48, raw.mft_ref);
    }
}

fn append_zeroes(out: &mut Vec<u8>, new_len: usize) -> Result<(), i32> {
    out.try_reserve(new_len - out.len())
        .map_err(|_| STATUS_INSUFFICIENT_RESOURCES)?;
    out.resize(new_len, 0);
    Ok(())
}

/// Encode a bounded owned buffer without forming references over flexible arrays.
///
/// Input order is preserved. Name lengths are checked against the actual source
/// array before reading. A partial name retains its complete FileNameLength and
/// has NextEntryOffset zero. Callers handling subsequent queries may discard a
/// PartialName result without advancing their cursor; this module does not
/// invent first/subsequent-query or filesystem-specific index semantics.
pub fn serialize(
    entries: &[RawDirEntry],
    class: u32,
    capacity: usize,
    single: bool,
) -> Result<DirectoryBuffer, i32> {
    let prefix = name_offset(class)?;
    let mut result = DirectoryBuffer {
        bytes: Vec::new(),
        entries_written: 0,
        disposition: Disposition::Complete,
    };
    let mut previous_start = None;
    for raw in entries {
        if usize::from(raw.name_len) > raw.name.len() {
            return Err(STATUS_INVALID_PARAMETER);
        }
        let name_bytes = usize::from(raw.name_len) * 2;
        // Every record is at most 616 bytes, so per-record offsets fit DWORD.
        let start = result.bytes.len().checked_add(7)
            .map(|length| length & !7).ok_or(STATUS_INVALID_PARAMETER)?;
        let end = start.checked_add(prefix + name_bytes)
            .ok_or(STATUS_INVALID_PARAMETER)?;
        if end > capacity {
            if !result.bytes.is_empty() {
                result.disposition = Disposition::MoreEntries;
                break;
            }
            if capacity < prefix {
                result.disposition = Disposition::HeaderTooSmall;
                break;
            }
            append_zeroes(&mut result.bytes, capacity)?;
            header(&mut result.bytes, raw, class);
            for (index, byte) in result.bytes[prefix..].iter_mut().enumerate() {
                *byte = raw.name[index / 2].to_le_bytes()[index % 2];
            }
            result.disposition = Disposition::PartialName;
            break;
        }
        append_zeroes(&mut result.bytes, end)?;
        if let Some(previous) = previous_start {
            put_u32(&mut result.bytes, previous, (start - previous) as u32);
        }
        header(&mut result.bytes[start..], raw, class);
        for (source, target) in raw.name[..usize::from(raw.name_len)]
            .iter().zip(result.bytes[start + prefix..end].chunks_exact_mut(2))
        {
            target.copy_from_slice(&source.to_le_bytes());
        }
        previous_start = Some(start);
        result.entries_written += 1;
        if single {
            break;
        }
    }
    Ok(result)
}
