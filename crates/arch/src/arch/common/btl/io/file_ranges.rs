use alloc::vec::Vec;
use core::ops::Range;

const INVALID_PARAMETER: u32 = 0xc000_000d;
const NO_MEMORY: u32 = 0xc000_0017;
const FILE_LOCK_CONFLICT: u32 = 0xc000_0054;
const LOCK_NOT_GRANTED: u32 = 0xc000_0055;
const RANGE_NOT_LOCKED: u32 = 0xc000_007e;
const MAX_LOCKS: usize = 4096;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct FileLockKey {
    pub record: u64,
    pub owner: u64,
}

pub struct FileRangeLocks(Vec<(FileLockKey, Range<u64>)>);

impl FileRangeLocks {
    pub const fn new() -> Self { Self(Vec::new()) }

    pub fn check(&self, key: FileLockKey, offset: u64, length: u64) -> Result<(), u32> {
        let end = offset.checked_add(length).ok_or(INVALID_PARAMETER)?;
        if length != 0 && self.0.iter().any(|(held, range)| {
            held.record == key.record && held.owner != key.owner
                && offset < range.end && range.start < end
        }) {
            return Err(FILE_LOCK_CONFLICT);
        }
        Ok(())
    }

    pub fn set(&mut self, key: FileLockKey, offset: u64, length: u64, lock: bool) -> Result<(), u32> {
        if length == 0 { return Err(INVALID_PARAMETER); }
        let end = offset.checked_add(length).ok_or(INVALID_PARAMETER)?;
        if lock {
            if self.0.iter().any(|(held, range)| {
                held.record == key.record && offset < range.end && range.start < end
            }) {
                return Err(LOCK_NOT_GRANTED);
            }
            if self.0.len() >= MAX_LOCKS { return Err(NO_MEMORY); }
            self.0.try_reserve(1).map_err(|_| NO_MEMORY)?;
            self.0.push((key, offset..end));
        } else {
            let index = self.0.iter().position(|(held, range)| {
                *held == key && range.start == offset && range.end == end
            }).ok_or(RANGE_NOT_LOCKED)?;
            self.0.swap_remove(index);
        }
        Ok(())
    }

    pub fn release(&mut self, key: FileLockKey) {
        self.0.retain(|(held, _)| *held != key);
    }
}
