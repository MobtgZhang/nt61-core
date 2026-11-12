pub const SYS_EXIT_PROCESS: u32 = 0x0201;
pub const SYS_DESKTOP_INFO: u32 = 0x0300;
#[allow(dead_code)]
pub const SYS_DESKTOP_DRAW_TEXT: u32 = 0x0302;
pub const SYS_DESKTOP_DRAW_RGBA: u32 = 0x0303;
pub const SYS_DESKTOP_POLL_KEY: u32 = 0x0304;
pub const SYS_DESKTOP_POLL_MOUSE: u32 = 0x0305;
pub const SYS_GET_KEY: u32 = 0x0204;
#[allow(dead_code)]
pub const SYS_PUTCHAR: u32 = 0x0202;

/// # Safety
///
/// Callers must provide valid Ring-3 pointers for pointer arguments.
#[unsafe(naked)]
unsafe extern "sysv64" fn syscall5_raw(
    number: u32,
    arg0: usize,
    arg1: usize,
    arg2: usize,
    arg3: usize,
    arg4: usize,
) -> u64 {
    // Map the SysV register arguments the Rust caller passes
    // (number=rdi, arg0..arg4=rsi,rdx,rcx,r8,r9) onto the kernel's
    // Windows-x64-style syscall convention: the number in RAX and
    // arg0..arg3 in R10,RDX,R8,R9 with arg5-style extras on the user
    // stack. The kernel reads stack argument 0 at RSP+8, so push the
    // placeholder first and the extra argument second.
    core::arch::naked_asm!(
        "push r9",    // arg4 saved first; ends up at RSP+8 after the dummy push
        "push rax",   // dummy placeholder at RSP; RAX still holds garbage here
        "mov r9, r8", // arg3: SysV r8 → kernel r9
        "mov r8, rcx",
        "mov r10, rsi", // arg0: SysV rsi → kernel r10
        "mov rax, rdi", // syscall number
        "syscall",
        "add rsp, 16",
        "ret",
    );
}

#[inline(always)]
pub fn syscall5(
    number: u32,
    arg0: usize,
    arg1: usize,
    arg2: usize,
    arg3: usize,
    arg4: usize,
) -> u64 {
    unsafe { syscall5_raw(number, arg0, arg1, arg2, arg3, arg4) }
}

#[inline(always)]
pub fn desktop_info(info: &mut super::DesktopFramebufferInfo) -> u64 {
    syscall5(
        SYS_DESKTOP_INFO,
        core::ptr::from_mut(info) as usize,
        0,
        0,
        0,
        0,
    )
}

/// # Safety
///
/// The text pointer must reference `length` readable bytes in user space.
#[inline(always)]
#[allow(dead_code)]
pub unsafe fn draw_text(
    text: *const u8,
    length: usize,
    x: u32,
    y: u32,
    foreground: u32,
    background: u32,
) -> u64 {
    syscall5(
        SYS_DESKTOP_DRAW_TEXT,
        text as usize,
        length,
        x as usize,
        y as usize,
        (((foreground as u64) << 32) | background as u64) as usize,
    ) as u64
}

/// # Safety
///
/// The pixel pointer must reference a complete RGBA bitmap in user space.
#[inline(always)]
pub unsafe fn draw_rgba(
    pixels: *const u8,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    stride: usize,
) -> u64 {
    syscall5(
        SYS_DESKTOP_DRAW_RGBA,
        pixels as usize,
        (y as usize) | ((x as usize) << 32),
        (height as usize) | ((width as usize) << 32),
        stride,
        0,
    )
}

/// # Safety
///
/// `key` must point to writable memory in user space.
#[inline(always)]
pub unsafe fn poll_key(key: *mut u8) -> u64 {
    syscall5(SYS_DESKTOP_POLL_KEY, key as usize, 0, 0, 0, 0)
}

/// # Safety
///
///  must point to writable memory of at least 12 bytes.
#[inline(always)]
pub unsafe fn poll_mouse(state: *mut super::DesktopMouseState) -> u64 {
    syscall5(
        SYS_DESKTOP_POLL_MOUSE,
        core::ptr::from_mut(&mut *state) as usize,
        0,
        0,
        0,
        0,
    )
}

/// # Safety
///
/// `key` must point to writable memory in user space.
#[inline(always)]
pub unsafe fn wait_key(key: *mut u8) -> u64 {
    syscall5(SYS_GET_KEY, key as usize, 0, 0, 0, 0)
}

#[inline(always)]
pub fn exit_process(exit_code: u32) -> ! {
    syscall5(SYS_EXIT_PROCESS, exit_code as usize, 0, 0, 0, 0);
    loop {
        core::hint::spin_loop();
    }
}

/// Emit one diagnostic byte to the kernel serial console. Ring 3 has
/// no IOPL, so the syscall is the only channel.
#[inline(always)]
#[allow(dead_code)]
pub fn debug_putc(byte: u8) {
    syscall5(SYS_PUTCHAR, byte as usize, 0, 0, 0, 0);
}

#[inline(always)]
#[allow(dead_code)]
pub fn debug_str(s: &str) {
    for b in s.as_bytes() {
        debug_putc(*b);
    }
}
