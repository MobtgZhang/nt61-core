//! x86_64 loader-to-kernel handoff.
//!
//! The loaded project kernel uses the private `BootInfo` ABI and its
//! `extern "C"` ELF entry expects the pointer in RDI. RCX is populated as
//! well for the project's EFI-ABI compatibility entry. This is not the
//! Windows 7 `LOADER_PARAMETER_BLOCK` contract.

/// Publish the legacy project callback slot, retained for older images.
pub fn install_kernel_handoff() -> u64 {
    ntoskrnl::arch::x86_64::ntoskrnl_handoff::install_handoff_pointer()
}


/// # Safety
///
/// The entry must be executable mapped kernel code, `bi_ptr` must point to a
/// live `BootInfo`, and the writable stack must cover the aligned stack top
/// and the entry's stack usage. Boot services must already have exited.
pub unsafe fn launch_kernel(stack_top: u64, bi_ptr: u64, ntoskrnl_entry: u64) -> ! {
    // SAFETY: the caller supplies the validated image, boot information and stack.
    unsafe { jump_to_ntoskrnl_kisystemstartup(stack_top, bi_ptr, ntoskrnl_entry) }
}

pub fn write_debug_marker(bytes: &[u8]) {
    write_com1_marker(bytes);
}

/// Enter the loaded PE image at its validated absolute entry address.
///
/// # Safety
///
/// The caller must meet the mapping, stack and lifetime requirements of
/// `launch_kernel`. The entry must obey the project's private `BootInfo` ABI.
#[cfg(target_arch = "x86_64")]
#[inline(never)]
pub unsafe extern "C" fn jump_to_ntoskrnl_kisystemstartup(
    stack_top: u64,
    bi_ptr: u64,
    ntoskrnl_entry: u64,
) -> ! {
    // SAFETY: fixed input registers cannot be overwritten by the stack/argument
    // moves. Interrupts are masked before replacing the firmware stack. The
    // call enters with RSP % 16 == 8 and reserves Microsoft x64 shadow space.
    unsafe {
        core::arch::asm!(
            "cli",
            "cld",
            "mov rsp, rdx",
            "and rsp, -16",
            "sub rsp, 32",
            "mov rdi, rcx",
            "xor ebp, ebp",
            "call rax",
            "ud2",
            in("rdx") stack_top,
            in("rcx") bi_ptr,
            in("rax") ntoskrnl_entry,
            options(noreturn),
        );
    }
}
/// Write raw diagnostic bytes to COM1 before UEFI services are initialized.
#[inline(never)]
pub fn write_com1_marker(bytes: &[u8]) {
    if !cfg!(feature = "log_level_debug") {
        return;
    }

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
