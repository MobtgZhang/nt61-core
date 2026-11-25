//! Synchronize instruction fetch after mapping and relocating a boot image.

/// Make copied and relocated code visible to instruction fetch on this CPU.
///
/// # Safety
///
/// The supplied range must describe a live, mapped image.
#[cfg(target_arch = "x86_64")]
pub unsafe fn sync_image_code(_base: u64, _size: u64) {
    // x86 instruction and data caches are coherent for newly mapped code.
    // Prevent the compiler from moving image writes across the entry call.
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
}

/// Make copied and relocated code visible to instruction fetch on this CPU.
///
/// # Safety
///
/// `base..base + size` must be a live, mapped image with a checked extent.
#[cfg(target_arch = "aarch64")]
pub unsafe fn sync_image_code(base: u64, size: u64) {
    let end = base.checked_add(size).expect("validated image extent");
    let ctr: u64;
    // SAFETY: UEFI executes at an exception level permitting cache maintenance.
    unsafe {
        core::arch::asm!("mrs {ctr}, ctr_el0", ctr = out(reg) ctr, options(nostack));
    }
    let dline = 4u64 << ((ctr >> 16) & 0xf);
    let iline = 4u64 << (ctr & 0xf);
    let mut addr = base & !(dline - 1);
    while addr < end {
        // SAFETY: addr selects a cache line covering the validated image mapping.
        unsafe {
            core::arch::asm!("dc cvau, {addr}", addr = in(reg) addr, options(nostack));
        }
        addr += dline;
    }
    // SAFETY: finish data writeback before invalidating corresponding I-cache lines.
    unsafe {
        core::arch::asm!("dsb ish", options(nostack));
    }
    addr = base & !(iline - 1);
    while addr < end {
        // SAFETY: addr selects a cache line covering the validated image mapping.
        unsafe {
            core::arch::asm!("ic ivau, {addr}", addr = in(reg) addr, options(nostack));
        }
        addr += iline;
    }
    // SAFETY: complete maintenance before fetching the loaded entry.
    unsafe {
        core::arch::asm!("dsb ish", "isb", options(nostack));
    }
}

/// Make copied and relocated code visible to instruction fetch on this CPU.
///
/// # Safety
///
/// The supplied range must describe a live, mapped image.
#[cfg(target_arch = "riscv64")]
pub unsafe fn sync_image_code(_base: u64, _size: u64) {
    // SAFETY: the loader executes on the current hart in supervisor mode.
    // Boot payloads run on this hart; secondary harts synchronize at startup.
    unsafe {
        core::arch::asm!("fence rw, rw", "fence.i", options(nostack));
    }
}

/// Make copied and relocated code visible to instruction fetch on this CPU.
///
/// # Safety
///
/// `base..base + size` must be a live, mapped image with a checked extent.
#[cfg(target_arch = "loongarch64")]
pub unsafe fn sync_image_code(base: u64, size: u64) {
    let end = base.checked_add(size).expect("validated image extent");
    let dcfg: u64;
    let icfg: u64;
    // SAFETY: CPUCFG reads are available at the loader's privilege level.
    unsafe {
        core::arch::asm!(
            "cpucfg {dcfg}, {dindex}",
            "cpucfg {icfg}, {iindex}",
            dcfg = out(reg) dcfg,
            icfg = out(reg) icfg,
            dindex = in(reg) 0x12u64,
            iindex = in(reg) 0x11u64,
            options(nostack),
        );
    }
    let dline = 1u64 << ((dcfg >> 24) & 0x7f).min(12);
    let iline = 1u64 << ((icfg >> 24) & 0x7f).min(12);
    let mut addr = base & !(dline - 1);
    while addr < end {
        // SAFETY: hit writeback/invalidate of L1 D-cache at a mapped image VA.
        unsafe {
            core::arch::asm!("cacop 0x11, {addr}, 0", addr = in(reg) addr, options(nostack));
        }
        addr += dline;
    }
    // SAFETY: complete data writes before invalidating the instruction cache.
    unsafe {
        core::arch::asm!("dbar 0", options(nostack));
    }
    addr = base & !(iline - 1);
    while addr < end {
        // SAFETY: hit invalidate of L1 I-cache at a mapped image VA.
        unsafe {
            core::arch::asm!("cacop 0x10, {addr}, 0", addr = in(reg) addr, options(nostack));
        }
        addr += iline;
    }
    // SAFETY: complete instruction maintenance before jumping to loaded code.
    unsafe {
        core::arch::asm!("dbar 0", "ibar 0", options(nostack));
    }
}
