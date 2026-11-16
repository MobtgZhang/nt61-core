use super::MmStatus;

/// Protect existing user 4 KiB leaves in the architecture's native format.
/// The caller must serialize mapping/unmapping and retain the root/tables.
/// The complete range is checked before writes. Concurrent leaf changes fail
/// closed; a changed prefix is possible. Large pages, COW and guards require
/// their own MM machinery and are rejected rather than partially emulated.
pub fn protect_user_range(root_phys: u64, va: u64, size: u64, new_protect: u32) -> MmStatus {
    use super::user_mapping_available;
    use super::user_page_protection::{protect_range, Architecture};
    use core::sync::atomic::{AtomicU64, Ordering};
    if size == 0 {
        return MmStatus::Ok;
    }
    if !user_mapping_available(va, size) {
        return MmStatus::InvalidAddress;
    }
    #[cfg(target_arch = "x86_64")]
    let architecture = Architecture::X64;
    #[cfg(target_arch = "aarch64")]
    let architecture = Architecture::Arm64;
    #[cfg(target_arch = "riscv64")]
    let architecture = Architecture::Riscv64;
    #[cfg(target_arch = "loongarch64")]
    let architecture = Architecture::Loong64;
    let success = protect_range(
        architecture,
        root_phys,
        va,
        size,
        new_protect,
        |table, index| {
            // SAFETY: caller retains native page-table storage; the walker
            // validates each intermediate descriptor before following it.
            let mapped = crate::mm::vm::direct_map_phys(table, 4096).ok()?;
            let value =
                unsafe { (&*((mapped as *const AtomicU64).add(index))).load(Ordering::Acquire) };
            crate::mm::vm::direct_unmap_va(mapped, 4096);
            Some(value)
        },
        |leaf, value| {
            let mapped = match crate::mm::vm::direct_map_phys(leaf.table, 4096) {
                Ok(mapped) => mapped,
                Err(_) => return false,
            };
            // SAFETY: a retained page-table page is mapped and the index is
            // below 512. Naturally aligned atomic replacement refuses a racing
            // mapping change instead of overwriting it or losing A/D state.
            let changed = unsafe {
                let entry = &*((mapped as *const AtomicU64).add(leaf.index));
                entry
                    .compare_exchange(leaf.value, value, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
            };
            crate::mm::vm::direct_unmap_va(mapped, 4096);
            if changed {
                // Downgrades cannot return while a remote CPU can still use
                // stale write/user permissions; no lazy timeout is allowed.
                crate::ke::tlb_shootdown::invalidate_va_all_cpus_complete(leaf.va);
            }
            changed
        },
    );
    if success {
        MmStatus::Ok
    } else {
        MmStatus::InvalidAddress
    }
}
