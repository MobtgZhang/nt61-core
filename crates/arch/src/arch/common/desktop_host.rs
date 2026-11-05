//! Private Ring-3 desktop shell ABI (NT 6.1 Explorer).
//!
//! This is not a Win32 USER/GDI ABI. It is a small, versioned
//! framebuffer bridge used by the first `explorer.exe` while the
//! full win32k object/message stack is being implemented.

use crate::hal::common::framebuffer_impl::{self, FramebufferInfo};
use alloc::vec;

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
}

pub const SYS_DESKTOP_INFO: u32 = 0x0300;
pub const SYS_DESKTOP_FILL_RECT: u32 = 0x0301;
pub const SYS_DESKTOP_DRAW_TEXT: u32 = 0x0302;
pub const SYS_DESKTOP_DRAW_RGBA: u32 = 0x0303;
pub const SYS_DESKTOP_POLL_KEY: u32 = 0x0304;

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
    match crate::ntoskrnl::mm::user_copy::copy_to_user(buffer as u64, slice) {
        Ok(()) => bytes as u64,
        Err(_) => STATUS_INVALID_PARAMETER,
    }
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
    STATUS_SUCCESS
}

/// Blit an RGBA buffer into the framebuffer. `stride_bytes` may be
/// larger than `width * 4` for aligned bitmaps.
///
/// # Safety
///
/// `pixels` must point to readable user bytes covering
/// `stride_bytes * height`.
pub unsafe fn draw_rgba(
    pixels: *const u8,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    stride_bytes: usize,
) -> u64 {
    if pixels.is_null() || width == 0 || height == 0 || stride_bytes < width as usize * 4 {
        return STATUS_INVALID_PARAMETER;
    }
    let required = stride_bytes.checked_mul(height as usize);
    let Some(byte_count) = required else {
        return STATUS_INVALID_PARAMETER;
    };
    let mut bytes = vec![0u8; byte_count];
    if crate::ntoskrnl::mm::user_copy::copy_from_user(&mut bytes, pixels as u64).is_err() {
        return STATUS_INVALID_PARAMETER;
    }
    let Some((destination_x, destination_y, clip_width, clip_height)) =
        clipped_rect(x, y, width, height)
    else {
        return STATUS_SUCCESS;
    };
    for row in 0..clip_height {
        let source_row = &bytes[(row as usize) * stride_bytes..];
        for column in 0..clip_width {
            let offset = (column as usize) * 4;
            let alpha = source_row[offset + 3];
            if alpha == 0 {
                continue;
            }
            let source_red = source_row[offset];
            let source_green = source_row[offset + 1];
            let source_blue = source_row[offset + 2];
            // The framebuffer stores 0x00RRGGBB (set_pixel writes the u32
            // verbatim) while color32 packs its arguments as BGR. Swap the
            // channels at this boundary so RGBA bitmaps keep true colors.
            let color = if alpha == 255 {
                framebuffer_impl::color32(source_blue, source_green, source_red)
            } else if let Some(background) =
                framebuffer_impl::get_pixel(destination_x + column, destination_y + row)
            {
                let background_red = ((background >> 16) & 0xFF) as u32;
                let background_green = ((background >> 8) & 0xFF) as u32;
                let background_blue = (background & 0xFF) as u32;
                let alpha = alpha as u32;
                let inverse = 255 - alpha;
                framebuffer_impl::color32(
                    ((source_blue as u32 * alpha + background_blue * inverse) / 255) as u8,
                    ((source_green as u32 * alpha + background_green * inverse) / 255) as u8,
                    ((source_red as u32 * alpha + background_red * inverse) / 255) as u8,
                )
            } else {
                framebuffer_impl::color32(source_blue, source_green, source_red)
            };
            framebuffer_impl::set_pixel(destination_x + column, destination_y + row, color);
        }
    }
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
    // TEMPORARY diagnostic: confirm the desktop input path receives
    // keystrokes (remove once the logon handoff is verified).
    crate::hal::serial::write_string_always("[DESKHOST] key=0x");
    crate::hal::serial::write_hex_u64_always(byte as u64);
    crate::hal::serial::write_string_always("\r\n");
    match crate::ntoskrnl::mm::user_copy::copy_to_user(key as u64, &bytes) {
        Ok(()) => 1,
        Err(_) => STATUS_INVALID_PARAMETER,
    }
}
