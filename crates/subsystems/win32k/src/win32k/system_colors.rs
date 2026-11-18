//! Shared system colors and stable system-owned brushes.
//!
//! This is the truecolor backend foundation for NtUserSetSysColors. Registration
//! still requires desktop access validation and user shared-info publication.
use alloc::vec::Vec;
use crate::{arch::common::syscall_frame::SyscallFrame, ke::sync::Spinlock, mm::user_copy};
use super::objects::{self, GdiBrush};

pub const COLOR_COUNT: usize = 31;
// Default Basic/96-DPI colors measured in the pinned original SP1 guest.
// Keep requested/realized mutable state for the existing color/brush consumers.
const DEFAULTS: [u32; COLOR_COUNT] = nt61_win32_metrics::BASIC_96_COLORS;

struct SystemColors {
    requested: [u32; COLOR_COUNT],
    realized: [u32; COLOR_COUNT],
    brushes: [u64; COLOR_COUNT],
}
static COLORS: Spinlock<SystemColors> = Spinlock::new(SystemColors {
    requested: DEFAULTS, realized: DEFAULTS, brushes: [0; COLOR_COUNT],
});

/// GetSysColor's native consumer. Invalid indices return zero.
pub fn get_sys_color(index: i32) -> u32 {
    COLORS.lock().realized.get(index as usize).copied().unwrap_or(0)
}

/// GetSysColorBrush's native consumer. The handle survives color changes and
/// cannot be deleted or revoked when the creating process exits.
pub fn get_sys_color_brush(index: i32) -> u64 {
    if !(0..COLOR_COUNT as i32).contains(&index) { return 0; }
    let mut state = COLORS.lock();
    let index = index as usize;
    if state.brushes[index] == 0 {
        let handle = objects::gdi_create_solid_brush(state.realized[index]);
        if handle != 0 {
            objects::with_handle_table(|table| {
                if let Some(entry) = table.get_mut(handle) {
                    entry.flags |= objects::HANDLE_FLAG_STOCK;
                    entry.process_id = 0;
                }
            });
            state.brushes[index] = handle;
        }
    }
    state.brushes[index]
}

/// Resolve a class background: COLOR_* + 1 or a real solid brush handle.
/// The returned value is COLORREF, so painting can use the same mutable colors
/// without allocating a brush just to clear a window.
pub fn background_color(brush: u64) -> Option<u32> {
    if (1..=COLOR_COUNT as u64).contains(&brush) {
        return Some(get_sys_color(brush as i32 - 1));
    }
    objects::with_handle_table(|table| {
        let entry = table.get(brush)?;
        if entry.get_type()? != objects::GdiObjectType::Brush || entry.obj_ptr == 0 { return None; }
        let brush = unsafe { &*(entry.obj_ptr as *const GdiBrush) };
        (brush.style == objects::BrushStyle::Solid as u32).then_some(brush.color)
    })
}

fn set_one(state: &mut SystemColors, index: u32, color: u32) {
    let index = index as usize;
    if index >= COLOR_COUNT { return; }
    state.requested[index] = color;
    // GreGetNearestColor strips the reserved high byte on an unpaletted
    // truecolor display. Requested and realized values remain distinct.
    let realized = color & 0x00ff_ffff;
    state.realized[index] = realized;
    let brush = state.brushes[index];
    if brush != 0 {
        objects::with_handle_table(|table| {
            if let Some(entry) = table.get(brush) {
                // SAFETY: the system brush is immortal, initialized as a
                // GdiBrush, and serialized by the object table guard.
                unsafe { (*(entry.obj_ptr as *mut GdiBrush)).color = realized; }
            }
        });
    }
}

/// Trusted desktop update, after authorization and complete input capture.
/// Flag 4 applies the four palette-sensitive indices first, matching Win7's
/// SetSysColor order even though the current display needs no palette remap.
pub fn set_system_colors(indices: &[u32], colors: &[u32], flags: u32) -> bool {
    if indices.len() != colors.len() { return false; }
    // PALETTEINDEX needs the display/DC palette resolver, which is not part of
    // this foundation. Do not silently substitute an RGB value for an index.
    if indices.iter().zip(colors).any(|(&index, &color)| index < COLOR_COUNT as u32 && color & 0x0100_0000 != 0) {
        return false;
    }
    {
        let mut state = COLORS.lock();
        if flags & 4 != 0 {
            for (&index, &color) in indices.iter().zip(colors) {
                if matches!(index, 1 | 15 | 16 | 20) { set_one(&mut state, index, color); }
            }
        }
        for (&index, &color) in indices.iter().zip(colors) {
            if flags & 4 == 0 || !matches!(index, 1 | 15 | 16 | 20) {
                set_one(&mut state, index, color);
            }
        }
    }
    if flags & 1 != 0 { super::window::WINDOW_MANAGER.lock().system_colors_changed(); }
    true
}

fn capture_array(address: u64, length: usize) -> Option<Vec<u8>> {
    user_copy::probe_user_read(address, length).ok()?;
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(length).ok()?;
    bytes.resize(length, 0);
    user_copy::copy_from_user(&mut bytes, address).ok()?;
    Some(bytes)
}

/// Unregistered adapter until the desktop WRITEOBJECTS check and shared client
/// mapping are implemented. Its trusted counterpart above exercises repaint.
pub extern "C" fn nt_user_set_sys_colors(frame: &SyscallFrame) -> u64 {
    let count = frame.args[0] as u32;
    let flags = frame.args[3] as u32;
    if count > 0x3fff_ffff || flags & 1 != 0 { return 0; }
    if count == 0 { return 1; }
    if (frame.args[1] | frame.args[2]) & 3 != 0 { return 0; }
    let length = count as usize * 4;
    // Capture both arrays completely before the first mutation, even for
    // invalid indices. A failure in the second array must leave all state alone.
    let Some(indices) = capture_array(frame.args[1], length) else { return 0; };
    let Some(colors) = capture_array(frame.args[2], length) else { return 0; };
    for (index, color) in indices.chunks_exact(4).zip(colors.chunks_exact(4)) {
        let index = u32::from_le_bytes([index[0], index[1], index[2], index[3]]);
        if index < COLOR_COUNT as u32 && color[3] & 1 != 0 { return 0; }
    }
    let mut state = COLORS.lock();
    for priority in 0..=u32::from(flags & 4 != 0) {
        for (index, color) in indices.chunks_exact(4).zip(colors.chunks_exact(4)) {
            let index = u32::from_le_bytes([index[0], index[1], index[2], index[3]]);
            let color = u32::from_le_bytes([color[0], color[1], color[2], color[3]]);
            let special = matches!(index, 1 | 15 | 16 | 20);
            if flags & 4 == 0 || special == (priority == 0) { set_one(&mut state, index, color); }
        }
    }
    1
}
