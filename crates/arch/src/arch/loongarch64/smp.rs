//! LoongArch 64 SMP — bring up APs via mail_send
//!
//! Implements the `extern "Rust"` functions required by `arch::common::smp`:
//! - `__smp_cpu_count()` - returns CPU count
//! - `__smp_boot_secondary()` - signals APs via mail_send
//! - `__smp_get_current_cpu_id()` - reads cpuid CSR
//!
//! Enabled core IDs come from ACPI MADT. Firmware mailbox/IPI startup
//! transfers each AP through a private-stack trampoline. The reported
//! count includes only CPUs that completed scheduler initialization.

use core::arch::asm;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use crate::arch::loongarch64::cpuinfo_core;
use crate::arch::loongarch64::paging;
use crate::arch::loongarch64::soc;

static ONLINE: AtomicU32 = AtomicU32::new(1);
static mut TOPOLOGY: crate::arch::common::smp::ProcessorTopology = crate::arch::common::smp::ProcessorTopology::empty();
static SECONDARY_ROOT_PFN: AtomicU64 = AtomicU64::new(0);

const MAX_AP_STACKS: usize = 256;
const AP_STACK_SIZE: usize = 64 * 1024;

#[no_mangle]
pub static AP_STACK_TOPS: [AtomicU64; MAX_AP_STACKS] = [const { AtomicU64::new(0) }; MAX_AP_STACKS];

core::arch::global_asm!(
    ".global __smp_secondary_entry",
    "__smp_secondary_entry:",
    "    csrrd $t0, 0x20",
    "    la.local $t1, AP_STACK_TOPS",
    "    slli.d $t2, $t0, 3",
    "    add.d $t1, $t1, $t2",
    "    ld.d $sp, $t1, 0",
    "    b secondary_entry",
);

pub fn cpu_count() -> u32 {
    ONLINE.load(Ordering::Acquire)
}

/// Firmware hardware core corresponding to a logical scheduler CPU.
pub fn hardware_cpu_id(cpu: u32) -> Option<u64> {
    unsafe { TOPOLOGY.ids[..TOPOLOGY.count].get(cpu as usize).copied() }
}

/// Number of SMT threads per physical core for the running SoC.
/// Returns `1` for LA364/LA464, `2` for LA664.
pub fn smt_threads() -> u8 {
    if soc::is_smt_capable() {
        2
    } else {
        1
    }
}

/// Total logical CPUs considering SMT:
///   `physical_count * smt_threads()`
pub fn logical_cpu_count(physical_count: u32) -> u32 {
    physical_count.saturating_mul(smt_threads() as u32)
}

/// Returns true if `cpu_id` shares its physical core with another
/// logical CPU (i.e. it has an SMT sibling).
pub fn has_smt_sibling(_cpu_id: u32) -> bool {
    smt_threads() > 1
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// Send a `mail_send` to the target CPU with the supplied mailbox
/// data. Used to start the AP.
pub unsafe fn mail_send(cpu_id: u32, mailbox: u64) {
    // IOCSR mailbox send writes the low/high halves of mailbox 0 on
    // the selected remote core. QEMU/EDK2 APs wait for this entry + IPI.
    let low = ((mailbox & 0xffff_ffff) << 32) | ((cpu_id as u64) << 16) | (1 << 31);
    let high = (mailbox & 0xffff_ffff_0000_0000) | ((cpu_id as u64) << 16) | (1 << 31) | 4;
    asm!("iocsrwr.d {value}, {address}", value = in(reg) low, address = in(reg) 0x1048u64, options(nostack));
    asm!("iocsrwr.d {value}, {address}", value = in(reg) high, address = in(reg) 0x1048u64, options(nostack));
    asm!("dbar 0", options(nostack));
    let signal = (1u32 << 31) | (cpu_id << 16);
    asm!("iocsrwr.w {value}, {address}", value = in(reg) signal, address = in(reg) 0x1040u64, options(nostack));
}

// ---------------------------------------------------------------------------
// extern "Rust" implementations required by arch::common::smp
// ---------------------------------------------------------------------------

/// Returns the number of available CPUs.
#[no_mangle]
pub extern "Rust" fn __smp_cpu_count() -> u32 {
    cpu_count()
}

/// Boot secondary CPUs with the given PML4 PFN.
///
/// # Safety
/// The caller must ensure the page table is valid.
#[no_mangle]
pub unsafe extern "Rust" fn __smp_boot_secondary(_pml4_pfn: u64) {
    SECONDARY_ROOT_PFN.store(_pml4_pfn, Ordering::Release);
    // Detect the SoC / cpuinfo state (idempotent).
    let _ = soc::detect();
    let _ = crate::arch::loongarch64::cpuinfo::detect_all();

    cpuinfo_core::init_boot_cpu();

    let mut topology = crate::arch::common::smp::firmware_processors(17);
    let boot: u32;
    asm!("csrrd {}, 0x20", out(reg) boot, options(nostack));
    topology.bootstrap_first(boot as u64);
    TOPOLOGY = topology;
    let count = topology.count as u32;
    crate::hal::serial::write_string_always(&alloc::format!("[SMP] enumerated={}\r\n", count));
    crate::ke::scheduler::init_smp(count);

    // Pre-allocate the per-CPU state and a private bootstrap stack for
    // each AP. The assembly entry reads the stack top by CPU id.
    for cpu in 1..count as usize {
        if cpu >= MAX_AP_STACKS || topology.ids[cpu] >= MAX_AP_STACKS as u64 {
            return;
        }
        let base = unsafe { crate::arch::loongarch64::percpu_impl::allocate(cpu as u32) };
        let stack =
            crate::mm::pool::allocate(crate::mm::pool::PoolType::NonPaged, AP_STACK_SIZE) as u64;
        if base == 0 || stack == 0 {
            return;
        }
        let stack_top = stack + AP_STACK_SIZE as u64 - 16;
        unsafe {
            (base as *mut u64).add(1).write_volatile(stack_top);
            AP_STACK_TOPS[topology.ids[cpu] as usize].store(stack_top, Ordering::Release);
        }
    }

    extern "C" { fn __smp_secondary_entry(); }
    let entry = __smp_secondary_entry as *const () as u64;
    for cpu in 1..count {
        unsafe { mail_send(topology.ids[cpu as usize] as u32, entry) };
    }
    for _ in 0..10_000_000 {
        if ONLINE.load(Ordering::Acquire) == count { break; }
        core::hint::spin_loop();
    }
}

/// Returns the current CPU ID from the cpuid CSR (LoongArch-specific).
#[no_mangle]
pub extern "Rust" fn __smp_get_current_cpu_id() -> u32 {
    let cpuid: u32;
    unsafe {
        // csrrd reads the cpuid CSR (0x20) into a general-purpose register.
        core::arch::asm!("csrrd {0}, 0x20", out(reg) cpuid, options(nostack));
    }
    unsafe { TOPOLOGY.ids[..TOPOLOGY.count].iter().position(|&id| id == cpuid as u64).unwrap_or(0) as u32 }
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// Rust entry reached by the assembly trampoline after the per-CPU stack
/// has been selected.
#[no_mangle]
pub unsafe extern "C" fn secondary_entry() -> ! {
    let cpu = __smp_get_current_cpu_id();
    let base = crate::arch::loongarch64::percpu_impl::per_cpu_base_for(cpu);
    if base == 0 {
        loop {
            asm!("idle 0", options(nostack));
        }
    }

    crate::arch::loongarch64::percpu_impl::set_tp(base);

    crate::arch::loongarch64::fpu::enable(false, false);
    crate::arch::loongarch64::init_page_walk();
    let root_pfn = SECONDARY_ROOT_PFN.load(Ordering::Acquire);
    crate::arch::loongarch64::idt::init();
    if root_pfn != 0 {
        unsafe { paging::load_page_root(root_pfn) };
    }

    crate::ke::scheduler::init_smp_this_cpu(cpu);
    ONLINE.fetch_add(1, Ordering::Release);
    crate::hal::serial::write_string_always(&alloc::format!("[SMP] online cpu={}\r\n", cpu));
    crate::ke::scheduler::idle_loop()
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
unsafe fn _keep() {
    let _ = asm!("nop");
    let _ = smt_threads();
    let _ = logical_cpu_count(1);
    let _ = has_smt_sibling(0);
}
