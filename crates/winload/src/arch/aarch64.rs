//! AArch64 loader-to-kernel handoff using the project's private `BootInfo`
//! ABI: X0 is the pointer and SP is 16-byte aligned. Firmware translation
//! remains active until the loaded kernel establishes its own page tables.

/// No legacy callback slot is needed for the loaded native kernel entry.
pub fn install_kernel_handoff() -> u64 {
    0
}

/// Make copied and relocated code visible to instruction fetch on this CPU.
///
/// # Safety
///
/// `base..base + size` must be a live, mapped image with a checked extent.
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

/// Enter the loaded PE kernel after ExitBootServices.
///
/// # Safety
///
/// The entry must be executable mapped kernel code, `bi_ptr` must point to a
/// live `BootInfo` describing that mapping, and the writable stack must cover
/// the aligned stack top and the entry's stack usage.
#[inline(never)]
pub unsafe fn launch_kernel(stack_top: u64, bi_ptr: u64, ntoskrnl_entry: u64) -> ! {
    // SAFETY: the caller guarantees a validated, mapped image and BootInfo.
    unsafe {
        let bi = &*(bi_ptr as *const nt61_boot_types::boot_types::BootInfo);
        sync_image_code(bi.ntoskrnl_image_base, bi.ntoskrnl_image_size);
    }
    // SAFETY: fixed registers preserve all operands while switching the stack.
    // Mask exceptions before replacing SP; keep the firmware MMU state intact.
    unsafe {
        core::arch::asm!(
            "msr daifset, #0xf",
            "mov sp, x9",
            "mov x29, xzr",
            "blr x16",
            "brk #0",
            in("x9") stack_top & !15,
            in("x0") bi_ptr,
            in("x16") ntoskrnl_entry,
            options(noreturn),
        );
    }
}

/// Write an architecture-local early diagnostic marker.
pub fn write_debug_marker(_bytes: &[u8]) {}

const PL011_DATA_REGISTER: u64 = 0x0900_0000;

pub fn write_com1_marker(bytes: &[u8]) {
    for byte in bytes {
        unsafe {
            core::ptr::write_volatile(PL011_DATA_REGISTER as *mut u8, *byte);
        }
    }
}
