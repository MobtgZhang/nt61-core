//! The volume's NTFS `$UpCase` table, indexed by UTF-16 code unit.
//!
//! Filename comparisons must use this on-disk mapping rather than the host's
//! Unicode version, locale, or mappings which expand one character to several.
use super::{get_file_by_record, read_file, NtfsFileSystem};
use crate::libs::ntdll::{status::STATUS_INSUFFICIENT_RESOURCES, types::NTSTATUS};
use alloc::vec::Vec;

pub const TABLE_BYTE_LEN: usize = 65_536 * 2;
const UPCASE_RECORD: u64 = 10;
const STATUS_FILE_CORRUPT_ERROR: NTSTATUS = 0xc000_0102u32 as NTSTATUS;

/// Read the complete, uncompressed NTFS 3.1 uppercase conversion table.
///
/// # Errors
/// Missing or malformed metadata and incomplete reads return
/// `STATUS_FILE_CORRUPT_ERROR`. Backend errors are preserved, and allocation
/// failure returns `STATUS_INSUFFICIENT_RESOURCES`. Callers must propagate the
/// failure rather than substitute ASCII or a different Unicode table.
pub fn read_table(fs: &NtfsFileSystem) -> Result<Vec<u8>, NTSTATUS> {
    let mut file = get_file_by_record(fs, UPCASE_RECORD).ok_or(STATUS_FILE_CORRUPT_ERROR)?;
    if file.is_directory || file.file_size != TABLE_BYTE_LEN as u64 {
        return Err(STATUS_FILE_CORRUPT_ERROR);
    }
    file.current_position = 0;
    let mut table = Vec::new();
    table
        .try_reserve_exact(TABLE_BYTE_LEN)
        .map_err(|_| STATUS_INSUFFICIENT_RESOURCES)?;
    table.resize(TABLE_BYTE_LEN, 0);

    let mut filled = 0;
    while filled < table.len() {
        let read = read_file(fs, &mut file, &mut table[filled..])?;
        if read == 0 || read > table.len() - filled {
            return Err(STATUS_FILE_CORRUPT_ERROR);
        }
        filled += read;
    }
    Ok(table)
}

/// Map one UTF-16 code unit using a table returned by [`read_table`].
///
/// The bytes are little endian on every architecture. An out-of-range lookup
/// in a malformed caller-supplied slice returns the original code unit; this
/// bounds check does not replace successful table loading before comparisons.
pub fn lookup(table: &[u8], unit: u16) -> u16 {
    let offset = usize::from(unit) * 2;
    match table.get(offset..offset + 2) {
        Some(pair) => u16::from_le_bytes([pair[0], pair[1]]),
        None => unit,
    }
}
