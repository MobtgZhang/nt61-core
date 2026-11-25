//! AArch64 UEFI boot hand-off.

/// QEMU `virt` machine PL011 serial port. Firmware on this platform
/// exposes the debug UART at the standard 0x0900_0000 MMIO address.
const PL011_DATA_REGISTER: u64 = 0x0900_0000;

/// Write raw diagnostic bytes to the firmware debug UART.
#[inline(never)]
pub fn write_com1_marker(bytes: &[u8]) {
    for byte in bytes {
        unsafe {
            core::ptr::write_volatile(PL011_DATA_REGISTER as *mut u8, *byte);
        }
    }
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// Load `efiapi` argument registers and branch to the target image
/// with a private stack. The target is expected not to return.
///
/// SAFETY: `stack_top` must be mapped, writable, and 16-byte aligned;
/// `entry_vma` must point to a valid UEFI entry point.
pub unsafe fn launch_efi_main(
    image_handle: u64,
    system_table: u64,
    stack_top: u64,
    entry_vma: u64,
) -> ! {
    core::arch::asm!(
        "mov sp, x2",
        "blr x3",
        "brk #0",
        in("x0") image_handle,
        in("x1") system_table,
        in("x2") stack_top,
        in("x3") entry_vma,
        options(noreturn),
    )
}
