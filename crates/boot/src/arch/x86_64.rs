/// Write raw diagnostic bytes to COM1 before boot services are used.
#[inline(never)]
pub fn write_com1_marker(bytes: &[u8]) {
    for byte in bytes {
        unsafe {
            core::arch::asm!(
                "out dx, al",
                in("dx") 0x3f8_u16,
                in("al") *byte as i8,
                options(nostack),
            );
        }
    }
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// Load `efiapi` argument registers and jump to the target image
/// with a private stack. The target is expected not to return.
///
/// SAFETY: `stack_top` must be a mapped, writable, 16-byte-aligned
/// stack and `entry_vma` must point to a valid UEFI entry point.
pub unsafe fn launch_efi_main(
    image_handle: u64,
    system_table: u64,
    stack_top: u64,
    entry_vma: u64,
) -> ! {
    core::arch::asm!(
        "mov rsp, r10",
        "sub rsp, 32",
        "call rax",
        "cli",
        "2: hlt",
        "jmp 2b",
        in("rcx") image_handle,
        in("rdx") system_table,
        in("r10") stack_top,
        in("rax") entry_vma,
        options(noreturn),
    )
}
