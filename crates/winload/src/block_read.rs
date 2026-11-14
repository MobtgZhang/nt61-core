//! Heap-backed sector reads shared by the NTFS loader and its host tests.

extern crate alloc;

use alloc::vec::Vec;

const SECTOR_SIZE: usize = 512;
const TRANSFER_BYTES: usize = 64 * 1024;

pub(crate) fn read_sectors(
    start_lba: u64,
    count: u32,
    io_align: usize,
    mut read: impl FnMut(u64, &mut [u8]) -> Option<()>,
) -> Option<Vec<u8>> {
    if count == 0 {
        return Some(Vec::new());
    }
    start_lba.checked_add(u64::from(count) - 1)?;
    let alignment = io_align.max(1);
    if !alignment.is_power_of_two() {
        return None;
    }
    let byte_count = (count as usize).checked_mul(SECTOR_SIZE)?;
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(byte_count).ok()?;
    bytes.resize(byte_count, 0);

    // Firmware can require alignment greater than the pool allocator provides.
    // Reuse an aligned heap slice for bounded transfers; no large stack frame.
    let transfer_len = byte_count.min(TRANSFER_BYTES);
    let mut storage = Vec::new();
    let storage_len = transfer_len.checked_add(alignment - 1)?;
    storage.try_reserve_exact(storage_len).ok()?;
    storage.resize(storage_len, 0);
    let offset = (storage.as_ptr() as usize).wrapping_neg() & (alignment - 1);
    let transfer = &mut storage[offset..offset + transfer_len];
    for (index, chunk) in bytes.chunks_mut(TRANSFER_BYTES).enumerate() {
        let lba = start_lba + (index * (TRANSFER_BYTES / SECTOR_SIZE)) as u64;
        read(lba, &mut transfer[..chunk.len()])?;
        chunk.copy_from_slice(&transfer[..chunk.len()]);
    }
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batches_firmware_calls_and_preserves_sector_contents() {
        let mut calls = 0;
        let bytes = read_sectors(17, 257, 1, |lba, buf| {
            calls += 1;
            for (index, sector) in buf.chunks_exact_mut(SECTOR_SIZE).enumerate() {
                sector.fill((lba + index as u64) as u8);
            }
            Some(())
        })
        .unwrap();
        assert_eq!(calls, 3, "257 sectors should use three bounded transfers");
        for (index, sector) in bytes.chunks_exact(SECTOR_SIZE).enumerate() {
            assert!(sector.iter().all(|&byte| byte == (17 + index) as u8));
        }
    }

    #[test]
    fn honors_firmware_buffer_alignment() {
        for alignment in [1, 8, 512, 4096, 131072] {
            read_sectors(0, 257, alignment, |_, buf| {
                assert_eq!(buf.as_ptr() as usize % alignment, 0);
                Some(())
            })
            .unwrap();
        }
    }

    #[test]
    fn propagates_io_failure() {
        assert!(read_sectors(0, 257, 1, |lba, _| (lba < 128).then_some(())).is_none());
    }

    #[test]
    fn rejects_lba_overflow_before_reading() {
        assert!(read_sectors(u64::MAX, 2, 1, |_, _| panic!("invalid range read")).is_none());
    }

    #[test]
    fn empty_read_needs_no_firmware_call() {
        assert_eq!(
            read_sectors(0, 0, 0, |_, _| panic!("empty read")),
            Some(Vec::new())
        );
    }
}
