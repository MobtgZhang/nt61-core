//! RISC-V64 loader-to-kernel handoff using the project's private `BootInfo`
//! ABI: A0 holds the pointer and SP is 16-byte aligned. The loaded kernel
//! replaces the firmware's SATP during its architecture initialization.

pub fn install_kernel_handoff() -> u64 {
    0
}

/// Enter the loaded PE kernel after ExitBootServices.
///
/// # Safety
///
/// The entry must be executable mapped kernel code, `bi_ptr` must point to a
/// live `BootInfo`, and the writable stack must cover the aligned stack top
/// and the entry's stack usage. The loader must execute in supervisor mode.
#[inline(never)]
pub unsafe fn launch_kernel(stack_top: u64, bi_ptr: u64, ntoskrnl_entry: u64) -> ! {
    // SAFETY: fixed operand registers survive clearing the firmware's TP/GP.
    // Mask SIE and SUM before switching stacks, and synchronize instruction
    // fetch with the loader's image copies and relocation writes.
    unsafe {
        core::arch::asm!(
            "csrc sstatus, t2",
            "fence rw, rw",
            "fence.i",
            "mv tp, zero",
            "mv gp, zero",
            "mv sp, t0",
            "mv s0, zero",
            "jalr ra, t1, 0",
            "unimp",
            in("t0") stack_top & !15,
            in("a0") bi_ptr,
            in("t1") ntoskrnl_entry,
            in("t2") (1u64 << 1) | (1u64 << 18),
            options(noreturn),
        );
    }
}

pub fn write_debug_marker(_bytes: &[u8]) {}

const UART_DATA_REGISTER: u64 = 0x1000_0000;

pub fn write_com1_marker(bytes: &[u8]) {
    for byte in bytes {
        unsafe {
            core::ptr::write_volatile(UART_DATA_REGISTER as *mut u8, *byte);
        }
    }
}
