//! Permanent supervisor aliases for the managed physical bank.
//!
//! Install these before the first process page root is copied. The mapping
//! never changes afterwards: global Rust allocations may outlive the process
//! which allocated them, and must not rely on that process's low identity map.
use core::sync::atomic::{AtomicU64, Ordering};

#[cfg(target_arch = "riscv64")]
const BASE: u64 = 0xffff_ffd0_0000_0000; // canonical Sv39 supervisor address
#[cfg(not(target_arch = "riscv64"))]
const BASE: u64 = 0xffff_a000_0000_0000;
const CAPACITY: u64 = 1024 * 1024 * 1024;
const PAGE: u64 = 4096;
static PHYSICAL_BASE: AtomicU64 = AtomicU64::new(0);
static MAPPED_BYTES: AtomicU64 = AtomicU64::new(0);

/// Called once during MM initialization, before process creation or SMP.
/// Partial failure leaves the page backend disabled. Installed aliases do not
/// own their target frames; only the page-table pages remain reserved by MM.
pub(super) fn init(physical: u64, bytes: u64) -> bool {
    if bytes == 0 || bytes > CAPACITY || physical % PAGE != 0 || bytes % PAGE != 0
        || physical.checked_add(bytes).is_none()
    {
        return false;
    }
    for offset in (0..bytes).step_by(PAGE as usize) {
        // Portable flags: present, writable, supervisor only, non-executable.
        if !crate::arch::map_page(BASE + offset, physical + offset, 0x8000_0000_0000_0003) {
            return false;
        }
    }
    PHYSICAL_BASE.store(physical, Ordering::Relaxed);
    MAPPED_BYTES.store(bytes, Ordering::Release);
    true
}

/// Return a stable kernel pointer only for a completely mapped extent.
pub(super) fn address(physical: u64, bytes: u64) -> Option<u64> {
    let mapped = MAPPED_BYTES.load(Ordering::Acquire);
    extent_address(PHYSICAL_BASE.load(Ordering::Relaxed), mapped, physical, bytes)
}

fn extent_address(bank: u64, mapped: u64, physical: u64, bytes: u64) -> Option<u64> {
    let offset = physical.checked_sub(bank)?;
    if mapped == 0 || bytes == 0 || offset.checked_add(bytes)? > mapped {
        return None;
    }
    BASE.checked_add(offset)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_only_complete_owned_bank_extents() {
        let bank = 0x1_2000_0000;
        assert_eq!(extent_address(bank, 0x20_0000, bank, 0x20_0000), Some(BASE));
        assert_eq!(extent_address(bank, 0x20_0000, bank + 4096, 4096), Some(BASE + 4096));
        for (mapped, physical, bytes) in [
            (0, bank, 4096), (4096, bank, 0), (4096, bank - 1, 1),
            (4096, bank, 4097), (4096, bank + 4096, 1),
            (4096, u64::MAX, 4096),
        ] {
            assert_eq!(extent_address(bank, mapped, physical, bytes), None);
        }
    }
}
