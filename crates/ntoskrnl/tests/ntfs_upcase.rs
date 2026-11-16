//! Execute the production loader with a controlled NTFS I/O provider.
#![allow(dead_code, non_camel_case_types)]
extern crate alloc;
pub mod libs {
    pub mod ntdll {
        pub use ntoskrnl::ntdll::{status, types};
    }
}
use libs::ntdll::types::NTSTATUS;
use std::cell::RefCell;

struct NtfsHandle {
    current_position: u64,
    is_directory: bool,
    file_size: u64,
}
struct NtfsFileSystem {
    data: Vec<u8>,
    missing: bool,
    directory: bool,
    declared_size: u64,
    chunk_size: usize,
    fail_at: Option<usize>,
    oversized_read: bool,
    opened_records: RefCell<Vec<u64>>,
    read_positions: RefCell<Vec<u64>>,
}
impl NtfsFileSystem {
    fn table() -> Self {
        let mut data = Vec::with_capacity(131_072);
        for unit in 0..=u16::MAX {
            data.extend_from_slice(&unit.to_le_bytes());
        }
        Self {
            data,
            missing: false,
            directory: false,
            declared_size: 131_072,
            chunk_size: usize::MAX,
            fail_at: None,
            oversized_read: false,
            opened_records: RefCell::new(Vec::new()),
            read_positions: RefCell::new(Vec::new()),
        }
    }
}
fn get_file_by_record(fs: &NtfsFileSystem, record: u64) -> Option<NtfsHandle> {
    fs.opened_records.borrow_mut().push(record);
    (!fs.missing).then_some(NtfsHandle {
        current_position: 0,
        is_directory: fs.directory,
        file_size: fs.declared_size,
    })
}
const IO_ERROR: NTSTATUS = 0xc000_0185u32 as NTSTATUS;
const CORRUPT: NTSTATUS = 0xc000_0102u32 as NTSTATUS;
fn read_file(
    fs: &NtfsFileSystem,
    file: &mut NtfsHandle,
    buffer: &mut [u8],
) -> Result<usize, NTSTATUS> {
    fs.read_positions.borrow_mut().push(file.current_position);
    let at = file.current_position as usize;
    if fs.fail_at.is_some_and(|fail_at| at >= fail_at) {
        return Err(IO_ERROR);
    }
    if fs.oversized_read {
        return Ok(buffer.len() + 1);
    }
    let amount = buffer
        .len()
        .min(fs.chunk_size)
        .min(fs.data.len().saturating_sub(at));
    buffer[..amount].copy_from_slice(&fs.data[at..at + amount]);
    file.current_position += amount as u64;
    Ok(amount)
}
#[path = "../../fs/src/fs/ntfs/upcase.rs"]
mod upcase;

#[test]
fn actual_loader_opens_system_record_and_preserves_every_table_byte() {
    let fs = NtfsFileSystem::table();
    let table = upcase::read_table(&fs).unwrap();
    assert_eq!(table.len(), upcase::TABLE_BYTE_LEN);
    assert_eq!(table, fs.data);
    assert_eq!(*fs.opened_records.borrow(), [10]);
    assert_eq!(*fs.read_positions.borrow(), [0]);
    for unit in 0..=u16::MAX {
        assert_eq!(upcase::lookup(&table, unit), unit);
    }
}

#[test]
fn actual_loader_assembles_short_reads_without_skipping_or_repeating_bytes() {
    let mut fs = NtfsFileSystem::table();
    fs.chunk_size = 997;
    let table = upcase::read_table(&fs).unwrap();
    assert_eq!(table, fs.data);
    let positions = fs.read_positions.borrow();
    assert!(positions.len() > 100);
    for (index, position) in positions.iter().enumerate() {
        assert_eq!(*position as usize, index * 997);
    }
}

#[test]
fn missing_directory_and_incorrect_size_fail_before_reading_file_bytes() {
    for kind in 0..5 {
        let mut fs = NtfsFileSystem::table();
        match kind {
            0 => fs.missing = true,
            1 => fs.directory = true,
            2 => fs.declared_size = 0,
            3 => fs.declared_size -= 1,
            _ => fs.declared_size += 1,
        }
        assert_eq!(upcase::read_table(&fs), Err(CORRUPT));
        assert!(fs.read_positions.borrow().is_empty());
        assert_eq!(*fs.opened_records.borrow(), [10]);
    }
}

#[test]
fn incomplete_and_impossible_reads_reject_the_table() {
    for length in [0, 1, 65_535, 131_071] {
        let mut fs = NtfsFileSystem::table();
        fs.data.truncate(length);
        assert_eq!(upcase::read_table(&fs), Err(CORRUPT));
    }
    let mut fs = NtfsFileSystem::table();
    fs.oversized_read = true;
    assert_eq!(upcase::read_table(&fs), Err(CORRUPT));
}

#[test]
fn backend_failure_is_preserved_even_after_partial_progress() {
    for fail_at in [0, 4096] {
        let mut fs = NtfsFileSystem::table();
        fs.chunk_size = 4096;
        fs.fail_at = Some(fail_at);
        assert_eq!(upcase::read_table(&fs), Err(IO_ERROR));
        assert_eq!(
            fs.read_positions.borrow().last().copied(),
            Some(fail_at as u64)
        );
    }
}

#[test]
fn mappings_use_volume_bytes_including_non_ascii_surrogates_and_last_code_unit() {
    let mut fs = NtfsFileSystem::table();
    for (unit, mapped) in [
        (0x0069u16, 0x0130u16),
        (0x00df, 0x1234),
        (0xd800, 0xabcd),
        (0xffff, 0x4321),
    ] {
        let at = usize::from(unit) * 2;
        fs.data[at..at + 2].copy_from_slice(&mapped.to_le_bytes());
    }
    let table = upcase::read_table(&fs).unwrap();
    for (unit, mapped) in [
        (0x0069, 0x0130),
        (0x00df, 0x1234),
        (0xd800, 0xabcd),
        (0xffff, 0x4321),
    ] {
        assert_eq!(upcase::lookup(&table, unit), mapped);
    }
    assert_eq!(upcase::lookup(&[], 0xffff), 0xffff);
    assert_eq!(upcase::lookup(&[0x41], 0), 0);
}
