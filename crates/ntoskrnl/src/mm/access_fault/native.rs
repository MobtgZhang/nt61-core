//! Native user faults: committed anonymous pages and native access bits.
use super::native_policy::{self, Architecture, Backend, Install, Permissions, Region};
use super::{AccessFlags, FaultStatus};
#[cfg(target_arch = "aarch64")]
use crate::arch::aarch64::paging;
#[cfg(target_arch = "loongarch64")]
use crate::arch::loongarch64::paging;
#[cfg(target_arch = "riscv64")]
use crate::arch::riscv64::paging;
use crate::ke::sync::Spinlock;
use crate::mm::vad::{VadFlags, VadType};

// Serialize native fault installations, including intermediate table creation.
// Kernel/recursive faults are rejected before this lock; independent CPUs wait.
static FAULT_LOCK: Spinlock<()> = Spinlock::new(());

struct Native;
impl Backend for Native {
    fn leaf(&mut self, va: u64) -> u64 {
        paging::fault_leaf(va)
    }
    fn update_access(&mut self, va: u64, expected: u64, updated: u64) -> bool {
        paging::fault_update_leaf(va, expected, updated)
    }
    fn allocate_zeroed(&mut self) -> Option<u64> {
        let pfn = crate::mm::pfn::allocate_pfn()?;
        let pa = pfn << 12;
        if !crate::arch::identity_map_region(pa, 4096) {
            crate::mm::pfn::free_pfn(pfn);
            return None;
        }
        // SAFETY: the newly allocated page is private and identity mapped.
        unsafe {
            core::ptr::write_bytes(pa as *mut u8, 0, 4096);
        }
        Some(pa)
    }
    fn free(&mut self, pa: u64) {
        crate::mm::pfn::free_pfn(pa >> 12);
    }
    fn install(&mut self, va: u64, pa: u64, p: Permissions) -> Install {
        use crate::mm::pte::{PTE_NX, PTE_RW, PTE_US};
        let flags = PTE_US | if p.write { PTE_RW } else { 0 } | if p.execute { 0 } else { PTE_NX };
        match paging::fault_install_page(va, pa, flags) {
            Some(true) => Install::Installed,
            Some(false) => Install::Occupied,
            None => Install::OutOfMemory,
        }
    }
}

pub fn handle(va: u64, access: AccessFlags) -> FaultStatus {
    #[cfg(target_arch = "aarch64")]
    let architecture = Architecture::Arm64;
    #[cfg(target_arch = "riscv64")]
    let architecture = Architecture::Riscv64;
    #[cfg(target_arch = "loongarch64")]
    let architecture = Architecture::LoongArch64;
    if !native_policy::user_address(architecture, va)
        || !access.user
        || !crate::mm::vas::user_mapping_available(va, 1)
    {
        return FaultStatus::AccessViolation;
    }
    let process = crate::arch::common::percpu::get_current_process();
    if process.is_null() {
        return FaultStatus::AccessViolation;
    }
    let _lock = FAULT_LOCK.lock();
    // SAFETY: the current thread keeps its owning process alive throughout the
    // trap. The VAD tree lock keeps its metadata valid until leaf installation.
    let process = unsafe { &*process };
    if process.pml4_phys == 0 || process.pml4_phys != paging::fault_root() {
        return FaultStatus::AccessViolation;
    }
    process
        .vad_root
        .with_entry(va, |vad| {
            let region = Region {
                committed: vad.flags.contains(VadFlags::SEC_COMMIT),
                guard: vad.flags.is_guard(),
                anonymous: matches!(
                    vad.vad_type,
                    VadType::VadNone | VadType::VadRoot | VadType::VadWriteWatch
                ),
                protection: vad.protection.bits(),
            };
            native_policy::resolve(architecture, va, access, Some(region), &mut Native)
        })
        .unwrap_or(FaultStatus::AccessViolation)
}
