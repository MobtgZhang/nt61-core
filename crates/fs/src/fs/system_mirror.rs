//! Explicit writeback for the boot-time NTFS system-volume mirror.
//!
//! Ordinary mutations remain in the shared mirror. A persistence boundary
//! writes only dirty sectors to a uniquely identified backing volume. This
//! is synchronous writeback, not NTFS journal/crash recovery.
use crate::{drivers::storage::disk, ke::sync::Spinlock};
use alloc::{vec, vec::Vec};

const IO_ERROR: u32 = 0xc0000001;
const UNSUPPORTED: u32 = 0xc00000bb;
const SECTOR: usize = 512;

struct DirtySectors {
    bits: Vec<u64>,
    sectors: usize,
}

static DIRTY: Spinlock<DirtySectors> = Spinlock::new(DirtySectors {
    bits: Vec::new(),
    sectors: 0,
});

pub(super) fn reset(size: usize) {
    let sectors = size / SECTOR;
    *DIRTY.lock() = DirtySectors {
        bits: vec![0; sectors.div_ceil(64)],
        sectors,
    };
}

/// Caller holds SYS_RAMDISK, which serializes mutations against flush.
pub(super) fn mark(offset: usize, length: usize) {
    if length == 0 {
        return;
    }
    let mut dirty = DIRTY.lock();
    let end = offset
        .saturating_add(length)
        .div_ceil(SECTOR)
        .min(dirty.sectors);
    for sector in offset / SECTOR..end {
        dirty.bits[sector / 64] |= 1 << (sector % 64);
    }
}

/// Match the complete original NTFS boot sector, including volume serial,
/// geometry and its partition offset. Never assume C: means disk zero.
/// NTFS volumes beyond the BPB's 32-bit hidden-sector range are unsupported.
fn backing(boot: &[u8; SECTOR], size: usize) -> Result<(usize, u64), u32> {
    if &boot[3..11] != b"NTFS    "
        || boot[11..13] != [0, 2]
        || boot[510..] != [0x55, 0xaa]
        || !size.is_multiple_of(SECTOR)
    {
        return Err(UNSUPPORTED);
    }
    let first = u32::from_le_bytes([boot[28], boot[29], boot[30], boot[31]]) as u64;
    let end = first.checked_add((size / SECTOR) as u64).ok_or(IO_ERROR)?;
    let offset = first.checked_mul(SECTOR as u64).ok_or(IO_ERROR)?;
    let mut found = None;
    for index in 0..disk::disk_count() {
        if disk::get_disk(index).is_none_or(|info| end > info.sector_count) {
            continue;
        }
        let mut candidate = [0; SECTOR];
        if disk::disk_read(index, offset, SECTOR, &mut candidate) != SECTOR || candidate != *boot {
            continue;
        }
        if found.is_some() {
            return Err(UNSUPPORTED); // Cloned volumes are ambiguous.
        }
        found = Some((index, offset));
    }
    found.ok_or(UNSUPPORTED)
}

/// Flush all pending system-volume metadata and data together. Retain the
/// complete dirty set on a partial failure so the caller can retry. Holding
/// SYS_RAMDISK keeps writers from publishing newer bytes during this flush.
pub(super) fn flush() -> Result<(), u32> {
    let guard = super::SYS_RAMDISK.lock();
    let mirror = guard.as_ref().ok_or(UNSUPPORTED)?;
    let mut dirty = DIRTY.lock();
    if mirror.size < SECTOR || mirror.block_size != SECTOR as u32 {
        return Err(UNSUPPORTED);
    }
    let mut buffer = [0; SECTOR];
    // SAFETY: the mounted mirror is mapped for its recorded size; its lock
    // excludes sector writers until all copies and I/O are complete.
    unsafe {
        for (index, byte) in buffer.iter_mut().enumerate() {
            *byte = core::ptr::read_volatile(mirror.base.add(index));
        }
    }
    let (device, offset) = backing(&buffer, mirror.size)?;
    if !disk::flush(device) {
        return Err(UNSUPPORTED);
    }
    for (word_index, word) in dirty.bits.iter().enumerate() {
        let mut bits = *word;
        while bits != 0 {
            let sector = word_index * 64 + bits.trailing_zeros() as usize;
            let at = sector * SECTOR;
            // SAFETY: mark only sets bits within the mounted mirror; reset
            // initializes the bitset when this same mirror is mounted.
            unsafe {
                for (index, byte) in buffer.iter_mut().enumerate() {
                    *byte = core::ptr::read_volatile(mirror.base.add(at + index));
                }
            }
            let address = offset.checked_add(at as u64).ok_or(IO_ERROR)?;
            if disk::disk_write(device, address, SECTOR, &buffer) != SECTOR {
                return Err(IO_ERROR);
            }
            bits &= bits - 1;
        }
    }
    if !disk::flush(device) {
        return Err(IO_ERROR);
    }
    dirty.bits.fill(0);
    Ok(())
}
