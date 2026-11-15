//! LoongArch64 loader-to-kernel handoff using the project's private
//! `BootInfo` ABI: A0 holds the pointer and SP is 16-byte aligned. Direct
//! addressing is restored before the loaded kernel initializes its MMU.

const CRMD_IE: u64 = 1 << 2;
const CRMD_DA: u64 = 1 << 3;
const CRMD_PG: u64 = 1 << 4;

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

/// Enter the loaded PE kernel after ExitBootServices.
///
/// # Safety
///
/// The entry must be executable mapped kernel code, `bi_ptr` must point to a
/// live `BootInfo` describing that image, and the writable stack must cover
/// the aligned stack top and entry's stack usage. All handoff addresses and
/// the trampoline must remain valid when direct addressing replaces paging.
#[inline(never)]
pub unsafe fn launch_kernel(stack_top: u64, bi_ptr: u64, ntoskrnl_entry: u64) -> ! {
    write_com1_marker(b"[TRAMPOLINE-LOONGARCH]\r\n");
    // SAFETY: the caller supplies the validated mapped image and BootInfo.
    unsafe {
        let bi = &*(bi_ptr as *const nt61_boot_types::boot_types::BootInfo);
        sync_image_code(bi.ntoskrnl_image_base, bi.ntoskrnl_image_size);
    }
    let crmd: u64;
    // SAFETY: CRMD is accessible at the loader's privilege level.
    unsafe {
        core::arch::asm!("csrrd {crmd}, 0x0", crmd = out(reg) crmd, options(nostack));
    }
    let direct_crmd = (crmd & !(CRMD_PG | CRMD_IE)) | CRMD_DA;
    // SAFETY: fixed argument registers survive CSRWR (which overwrites T0).
    // Disable interrupts in the same operation that restores direct addressing;
    // no Rust code or firmware-stack access executes after this transition.
    unsafe {
        core::arch::asm!(
            "csrwr $t0, 0x0",
            "ibar 0",
            "move $sp, $a1",
            "move $fp, $zero",
            "jirl $ra, $a2, 0",
            "break 0",
            in("$t0") direct_crmd,
            in("$a1") stack_top & !15,
            in("$a0") bi_ptr,
            in("$a2") ntoskrnl_entry,
            options(noreturn),
        );
    }
}

pub fn write_debug_marker(_bytes: &[u8]) {}

const UART0_BASE: u64 = 0x1FE0_01E0;
const UART_THR: u64 = 0;
const UART_LSR: u64 = 5;
const UART_LSR_THR_EMPTY: u8 = 0x20;

pub fn write_com1_marker(bytes: &[u8]) {
    for byte in bytes {
        unsafe {
            while core::ptr::read_volatile((UART0_BASE + UART_LSR) as *const u8)
                & UART_LSR_THR_EMPTY
                == 0
            {
                core::hint::spin_loop();
            }
            core::ptr::write_volatile((UART0_BASE + UART_THR) as *mut u8, *byte);
        }
    }
}
