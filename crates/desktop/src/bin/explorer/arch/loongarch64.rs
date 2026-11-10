pub const SYS_EXIT_PROCESS: u32 = 0x0201;
pub const SYS_DESKTOP_INFO: u32 = 0x0300;
pub const SYS_DESKTOP_FILL_RECT: u32 = 0x0301;
#[allow(dead_code)]
#[allow(dead_code)]
pub const SYS_DESKTOP_DRAW_TEXT: u32 = 0x0302;
pub const SYS_DESKTOP_DRAW_RGBA: u32 = 0x0303;
pub const SYS_DESKTOP_POLL_KEY: u32 = 0x0304;
pub const SYS_DESKTOP_POLL_MOUSE: u32 = 0x0305;
pub const SYS_GET_KEY: u32 = 0x0204;

/// # Safety
///
/// Callers must provide valid Ring-3 pointers for pointer arguments.
#[unsafe(naked)]
unsafe extern "C" fn syscall5_raw(
    number: u32,
    arg0: usize,
    arg1: usize,
    arg2: usize,
    arg3: usize,
    arg4: usize,
) -> u64 {
    core::arch::naked_asm!(
        "move $t0, $a0",
        "move $a7, $a0",
        "move $a0, $a1",
        "move $a1, $a2",
        "move $a2, $a3",
        "move $a3, $a4",
        "move $a4, $a5",
        "move $a5, $zero",
        "move $a6, $zero",
        "syscall 0",
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

#[inline(always)]
#[allow(dead_code)]
pub fn fill_rect(x: u32, y: u32, width: u32, height: u32, color: u32) -> u64 {
    syscall5(
        SYS_DESKTOP_FILL_RECT,
        x as usize,
        y as usize,
        width as usize,
        height as usize,
        color as usize,
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
    )
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
