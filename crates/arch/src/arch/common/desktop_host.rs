//! Private Ring-3 desktop shell ABI (NT 6.1 Explorer).
//!
//! This is not a Win32 USER/GDI ABI. It is a small, versioned
//! framebuffer bridge used by the first `explorer.exe` while the
//! full win32k object/message stack is being implemented.

use crate::hal::common::framebuffer_impl::{self, FramebufferInfo};

/// One-shot serial diagnostics for the desktop bridge: proves the
/// shell's private syscalls reach the kernel and reports the live
/// framebuffer geometry. Kept permanent (one line each) so desktop
/// bring-up can be verified from the serial log alone.
fn bridge_diag() {
    use core::sync::atomic::{AtomicBool, Ordering};
    static LOGGED: AtomicBool = AtomicBool::new(false);
    if LOGGED.swap(true, Ordering::AcqRel) {
        return;
    }
    let info = framebuffer_impl::info();
    crate::hal::serial::write_string_always("[DESKHOST] fb addr=");
    crate::hal::serial::write_hex_u64_always(info.address);
    crate::hal::serial::write_string_always(" w=");
    crate::hal::serial::write_hex_u64_always(info.width as u64);
    crate::hal::serial::write_string_always(" h=");
    crate::hal::serial::write_hex_u64_always(info.height as u64);
    crate::hal::serial::write_string_always(" pitch=");
    crate::hal::serial::write_hex_u64_always(info.pitch as u64);
    crate::hal::serial::write_string_always(" bpp=");
    crate::hal::serial::write_hex_u64_always(info.bpp as u64);
    crate::hal::serial::write_string_always("\r\n");
    crate::drivers::input::virtio_input::set_mouse_screen_bounds(info.width, info.height);
}

pub const SYS_DESKTOP_INFO: u32 = 0x0300;
pub const SYS_DESKTOP_FILL_RECT: u32 = 0x0301;
pub const SYS_DESKTOP_DRAW_TEXT: u32 = 0x0302;
pub const SYS_DESKTOP_DRAW_RGBA: u32 = 0x0303;
pub const SYS_DESKTOP_POLL_KEY: u32 = 0x0304;
pub const SYS_DESKTOP_POLL_MOUSE: u32 = 0x0305;

pub const STATUS_SUCCESS: u64 = 0;
pub const STATUS_INVALID_PARAMETER: u64 = 0xC000_000D;
pub const STATUS_NO_DATA: u64 = 0xC000_0011;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct DesktopFramebufferInfo {
    pub address: u64,
    pub width: u32,
    pub height: u32,
    pub pitch: u32,
    pub bpp: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct DesktopMouseState {
    pub x: i32,
    pub y: i32,
    pub buttons: u32,
}

impl DesktopFramebufferInfo {
    fn from_hal(info: FramebufferInfo) -> Self {
        Self {
            address: info.address,
            width: info.width,
            height: info.height,
            pitch: info.pitch,
            bpp: info.bpp,
        }
    }
}

/// Copy the current framebuffer geometry to the caller.
///
/// # Safety
///
/// `buffer` must point to 32 writable bytes in user address space.
pub unsafe fn info(buffer: *mut DesktopFramebufferInfo) -> u64 {
    if buffer.is_null() {
        return STATUS_INVALID_PARAMETER;
    }
    bridge_diag();
    let info = DesktopFramebufferInfo::from_hal(framebuffer_impl::info());
    let bytes = core::mem::size_of::<DesktopFramebufferInfo>();
    let raw = core::ptr::from_ref(&info) as *const u8;
    let slice = unsafe { core::slice::from_raw_parts(raw, bytes) };
    let result = match crate::ntoskrnl::mm::user_copy::copy_to_user(buffer as u64, slice) {
        Ok(()) => bytes as u64,
        Err(status) => status as u64,
    };
    result
}

fn clipped_rect(x: u32, y: u32, width: u32, height: u32) -> Option<(u32, u32, u32, u32)> {
    let fb = framebuffer_impl::info();
    if x >= fb.width || y >= fb.height || width == 0 || height == 0 {
        return None;
    }
    Some((x, y, width.min(fb.width - x), height.min(fb.height - y)))
}

/// Fill a rectangle with an opaque 24-bit RGB color.
pub fn fill_rect(x: u32, y: u32, width: u32, height: u32, rgb: u32) -> u64 {
    let Some((x, y, width, height)) = clipped_rect(x, y, width, height) else {
        return STATUS_SUCCESS;
    };
    bridge_diag();
    for row in 0..height {
        for column in 0..width {
            framebuffer_impl::set_pixel(
                x + column,
                y + row,
                framebuffer_impl::color32(
                    (rgb & 0xFF) as u8,
                    ((rgb >> 8) & 0xFF) as u8,
                    ((rgb >> 16) & 0xFF) as u8,
                ),
            );
        }
    }
    crate::drivers::bootvid::present_desktop_frame();
    STATUS_SUCCESS
}

/// Draw up to 255 bytes of UTF-8/ASCII text at a pixel position.
///
/// # Safety
///
/// `text` must point to `length` readable user bytes.
pub unsafe fn draw_text(text: *const u8, length: usize, x: u32, y: u32, fg: u32, bg: u32) -> u64 {
    if text.is_null() || length == 0 || length > 255 {
        return STATUS_INVALID_PARAMETER;
    }
    let mut bytes = [0u8; 255];
    if crate::ntoskrnl::mm::user_copy::copy_from_user(&mut bytes, text as u64).is_err() {
        return STATUS_INVALID_PARAMETER;
    }
    for (index, character) in bytes.iter().take(length).enumerate() {
        framebuffer_impl::draw_char(x / 8 + index as u32, y / 16, *character, fg, bg);
    }
    crate::drivers::bootvid::present_desktop_frame();
    STATUS_SUCCESS
}

/// Blit an RGBA buffer into the framebuffer. `stride_bytes` may be
/// larger than `width * 4` for aligned bitmaps.
///
/// # Safety
///
/// `pixels` must point to readable user bytes covering
/// `(height - 1) * stride_bytes + width * 4`.
pub unsafe fn draw_rgba(
    pixels: *const u8,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    stride_bytes: usize,
) -> u64 {
    let Some(source_width) = (width as usize).checked_mul(4) else {
        return STATUS_INVALID_PARAMETER;
    };
    if pixels.is_null() || width == 0 || height == 0 || stride_bytes < source_width {
        return STATUS_INVALID_PARAMETER;
    }
    let Some((destination_x, destination_y, clip_width, clip_height)) =
        clipped_rect(x, y, width, height)
    else {
        return STATUS_SUCCESS;
    };
    // The final row only needs the visible pixels, not the remaining stride.
    let readable_end = (clip_height as usize - 1).checked_mul(stride_bytes)
        .and_then(|n| n.checked_add(clip_width as usize * 4))
        .and_then(|n| (pixels as usize).checked_add(n));
    if readable_end.is_none() {
        return STATUS_INVALID_PARAMETER;
    }
    let framebuffer = framebuffer_impl::info();
    let direct = framebuffer.bpp == 32 && framebuffer.address != 0;
    // Bounded stack use, independent of display width. No allocation on cursor blits.
    let mut storage = [0u8; 1024];
    for row in 0..clip_height as usize {
        let mut column = 0usize;
        while column < clip_width as usize {
            let count = (clip_width as usize - column).min(storage.len() / 4);
            let bytes = &mut storage[..count * 4];
            let source = (pixels as usize + row * stride_bytes + column * 4) as u64;
            if crate::ntoskrnl::mm::user_copy::copy_from_user(bytes, source).is_err() {
                // Earlier chunks may already be visible.
                framebuffer_impl::mark_damage(destination_x, destination_y, clip_width, clip_height);
                return STATUS_INVALID_PARAMETER;
            }
            for index in 0..count {
                let offset = index * 4;
                let alpha = bytes[offset + 3] as u32;
                if alpha == 0 { continue; }
                let dx = destination_x + (column + index) as u32;
                let dy = destination_y + row as u32;
                let target = (framebuffer.address as usize + dy as usize * framebuffer.pitch as usize
                    + dx as usize * 4) as *mut u32;
                let mut red = bytes[offset] as u32;
                let mut green = bytes[offset + 1] as u32;
                let mut blue = bytes[offset + 2] as u32;
                if alpha != 255 {
                    let background = if direct { Some(target.read_volatile()) }
                        else { framebuffer_impl::get_pixel(dx, dy) };
                    if let Some(bg) = background {
                        let inverse = 255 - alpha;
                        red = (red * alpha + ((bg >> 16) & 255) * inverse) / 255;
                        green = (green * alpha + ((bg >> 8) & 255) * inverse) / 255;
                        blue = (blue * alpha + (bg & 255) * inverse) / 255;
                    }
                }
                let color = (red << 16) | (green << 8) | blue;
                if direct { target.write_volatile(color); }
                else { framebuffer_impl::set_pixel(dx, dy, color); }
            }
            column += count;
        }
    }
    framebuffer_impl::mark_damage(destination_x, destination_y, clip_width, clip_height);
    // Complete the basic-display present before returning to the producer.
    // LogonUI and Explorer enter user mode directly during early boot; the
    // background mirror thread may not have been dispatched yet. RAM writes
    // alone cannot update a VirtIO scanout, unlike a firmware framebuffer.
    crate::drivers::bootvid::present_desktop_frame();
    STATUS_SUCCESS
}

/// Poll one keyboard byte. Returns 1 when a byte was copied to
/// `key`, 0 when the queue is empty, and STATUS_INVALID_PARAMETER
/// when `key` is null.
///
/// # Safety
///
/// `key` must point to one writable user byte.
pub unsafe fn poll_key(key: *mut u8) -> u64 {
    if key.is_null() {
        return STATUS_INVALID_PARAMETER;
    }
    // Drain the PS/2 (and unified USB-HID) hardware rings into the
    // kernel input buffer first - identical to the SYS_POLL_KEY arm.
    // Without this the 8042 controller is never read in desktop mode,
    // so ENTER on the LogonUI welcome screen is never delivered.
    crate::hal::keyboard_input::poll();
    let Some(byte) = crate::hal::keyboard_input::try_read_byte() else {
        return STATUS_NO_DATA;
    };
    let bytes = [byte];
    match crate::ntoskrnl::mm::user_copy::copy_to_user(key as u64, &bytes) {
        Ok(()) => 1,
        Err(_) => STATUS_INVALID_PARAMETER,
    }
}

/// Copy the next completed pointer report to the caller. `buttons` uses bit 0 for
/// left, bit 1 for right, and bit 2 for middle.
///
/// # Safety
///
/// `state` must point to writable user memory of at least 12 bytes.
pub unsafe fn poll_mouse(state_pointer: *mut DesktopMouseState) -> u64 {
    if state_pointer.is_null() {
        return STATUS_INVALID_PARAMETER;
    }
    crate::drivers::input::virtio_input::poll();
    let Some((x, y, buttons)) = crate::drivers::input::virtio_input::pop_mouse_report() else {
        return 0;
    };
    let state = DesktopMouseState {
        x: i32::from(x),
        y: i32::from(y),
        buttons: u32::from(buttons),
    };
    let bytes = core::mem::size_of::<DesktopMouseState>();
    let raw = core::ptr::from_ref(&state) as *const u8;
    let slice = core::slice::from_raw_parts(raw, bytes);
    match crate::ntoskrnl::mm::user_copy::copy_to_user(state_pointer as u64, slice) {
        Ok(()) => bytes as u64,
        Err(_) => STATUS_INVALID_PARAMETER,
    }
}


// Scalar shell queries avoid exposing kernel layouts or passing unvalidated pointers.
pub const SYS_DESKTOP_TIME: u32 = 0x0306;
pub const SYS_DESKTOP_TICKS: u32 = 0x0307;
pub const SYS_DESKTOP_AUDIO: u32 = 0x0308;
pub const SYS_DESKTOP_DEVICES: u32 = 0x0309;

pub fn shell_time() -> u64 {
    if crate::ntoskrnl::ke::time::get_wall_clock_base() == 0 { return 0; }
    crate::ntoskrnl::ke::time::get_system_time()
}
/// UI deadlines use the hardware clock, which advances even when timer IRQ
/// delivery is delayed during a foreground syscall or early scheduler bring-up.
pub fn shell_ticks() -> u64 { crate::hal::common::pit::get_system_time_ms() }
pub fn shell_devices() -> u64 {
    u64::from(crate::drivers::audio::playback::stats().backend != 0)
        | ((crate::drivers::net::nic_count() as u64) << 8)
}
pub fn shell_audio(volume: u64, muted: u64) -> u64 {
    if volume > 100 || muted > 1 { return STATUS_INVALID_PARAMETER; }
    if crate::drivers::audio::playback::stats().backend == 0 { return STATUS_NO_DATA; }
    crate::drivers::audio::playback::set_master_volume(volume as u8);
    crate::drivers::audio::playback::set_master_mute(muted != 0);
    STATUS_SUCCESS
}
