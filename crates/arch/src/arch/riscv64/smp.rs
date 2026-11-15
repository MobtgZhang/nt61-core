//! RISC-V 64 SMP — SBI HSM HART_START.
//!
//! Implements the `extern "Rust"` functions required by
//! `arch::common::smp`:
//!
//! Enabled hardware hart IDs come from ACPI MADT. SBI HSM starts each AP
//! through a stack trampoline; logical IDs are independent of hart IDs.
//! The exported CPU count includes only harts that reached the scheduler.

use core::arch::global_asm;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use crate::arch::riscv64::paging;
use crate::arch::riscv64::percpu_impl;

static ONLINE: AtomicU32 = AtomicU32::new(1);
static mut TOPOLOGY: crate::arch::common::smp::ProcessorTopology = crate::arch::common::smp::ProcessorTopology::empty();
static SECONDARY_ROOT_PFN: AtomicU64 = AtomicU64::new(0);

extern "C" {
    fn __smp_secondary_entry();
}

const AP_STACK_SIZE: usize = 64 * 1024;

/// Return the cached CPU count.
pub fn cpu_count() -> u32 {
    ONLINE.load(Ordering::Acquire)
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// SBI HSM hart_start (re-exported here for callers that already
/// imported from this module).
pub unsafe fn sbi_hsm_hart_start(hart_id: u64, start_addr: u64, opaque: u64) -> i64 {
    crate::arch::riscv64::sbi::hsm_hart_start(hart_id, start_addr, opaque).error
}

/// Read the running hart's hardware ID from the published topology.
pub fn current_hart_id() -> u32 {
    let logical = crate::arch::riscv64::percpu_impl::__percpu_get_current_cpu_id() as usize;
    unsafe { if logical < TOPOLOGY.count { TOPOLOGY.ids[logical] as u32 } else { 0 } }
}

// Trampoline entry point for secondary harts.  The firmware jumps here
// after a successful `HART_START`; the Rust entry installs per-hart
// state before entering the scheduler.
global_asm!(
    ".align 4",
    ".global __smp_secondary_entry",
    "__smp_secondary_entry:",
    // Secondary harts start with an undefined stack pointer. Install
    // the trampoline stack before any Rust prologue can touch memory.
    "  mv sp, a1",
    "  tail __smp_secondary_entry_rust",
);

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
#[no_mangle]
pub unsafe extern "C" fn __smp_secondary_entry_rust(
    hardware_hart: u64,
    _stack_top: u64,
) -> ! {
    // Firmware defines only a0/a1 at HSM entry. Install tp before any
    // logging, since UART locking reads the current per-CPU base.
    let hart = unsafe { TOPOLOGY.ids[..TOPOLOGY.count].iter().position(|&id| id == hardware_hart).unwrap_or(0) as u32 };
    let base = crate::arch::riscv64::percpu_impl::per_cpu_base_for(hart);
    if base == 0 {
        loop {
            core::arch::asm!("wfi", options(nostack));
        }
    }
    crate::arch::riscv64::csr::write_tp(base);
    crate::boot_debug_println!("[AP] entry");

    // Install the same address space the BSP is already using.
    let root_pfn = SECONDARY_ROOT_PFN.load(Ordering::Acquire);
    if root_pfn != 0 {
        unsafe { paging::load_page_root(root_pfn) };
    }

    crate::arch::riscv64::idt::init();
    crate::arch::riscv64::fpu::enable_fpu();
    crate::ke::scheduler::init_smp_this_cpu(hart);
    ONLINE.fetch_add(1, Ordering::Release);
    crate::hal::serial::write_string_always(&alloc::format!("[SMP] online cpu={} hart={}\r\n", hart, hardware_hart));
    crate::ke::scheduler::idle_loop()
}

// ---------------------------------------------------------------------------
// extern "Rust" implementations required by arch::common::smp
// ---------------------------------------------------------------------------

/// Returns the number of available CPUs.
#[no_mangle]
pub extern "Rust" fn __smp_cpu_count() -> u32 {
    cpu_count()
}

/// Boot secondary harts using the root page-table PFN shared with the BSP.
///
/// # Safety
/// The root page table and physical trampoline must remain valid while
/// secondary harts install their private stack and per-CPU state.
#[no_mangle]
pub unsafe extern "Rust" fn __smp_boot_secondary(satp_root: u64) {
    SECONDARY_ROOT_PFN.store(satp_root, Ordering::Release);
    crate::boot_debug_println!("[SMP] boot_secondary entered");
    let mut topology = crate::arch::common::smp::firmware_processors(24);
    // HSM reports the only started hart before we release APs. There is no
    // SBI BASE get-hart-id call; BASE only defines FIDs 0 through 6.
    let boot = topology.ids[..topology.count].iter().copied().find(|&id| {
        let state = crate::arch::riscv64::sbi::hsm_hart_status(id);
        state.error == 0 && state.value == 0
    }).unwrap_or(0);
    topology.bootstrap_first(boot);
    TOPOLOGY = topology;
    let max = topology.count as u32;
    crate::hal::serial::write_string_always(&alloc::format!("[SMP] enumerated={} boot_hart={}\r\n", max, boot));
    crate::ke::scheduler::init_smp(max);

    for hart in 1..max as usize {
        let base = percpu_impl::per_cpu_base_for(hart as u32);
        let stack =
            crate::mm::pool::allocate(crate::mm::pool::PoolType::NonPaged, AP_STACK_SIZE) as u64;
        if base == 0 || stack == 0 {
            return;
        }
        let stack_top = stack + AP_STACK_SIZE as u64 - 16;
        unsafe {
            (base as *mut u64).add(1).write_volatile(stack_top);
        }
    }

    for hart in 1..max {
        let trampoline = __smp_secondary_entry as *const () as u64;
        let stack_top = unsafe {
            (percpu_impl::per_cpu_base_for(hart) as *const u64)
                .add(1)
                .read_volatile()
        };
        crate::hal::serial::write_string_always(&alloc::format!(
            "[SMP] hart={} trampoline={:#x} stack={:#x}\r\n",
            hart,
            trampoline,
            stack_top
        ));
        let r = sbi_hsm_hart_start(topology.ids[hart as usize], trampoline, stack_top);
        crate::hal::serial::write_string_always(&alloc::format!("[SMP] HART_START cpu={} status={}\r\n", hart, r));
    }
    for _ in 0..10_000_000 {
        if ONLINE.load(Ordering::Acquire) == max { break; }
        core::hint::spin_loop();
    }
}

/// Returns the current compact logical CPU ID from the per-hart base.
#[no_mangle]
pub extern "Rust" fn __smp_get_current_cpu_id() -> u32 {
    crate::arch::riscv64::percpu_impl::__percpu_get_current_cpu_id()
}
