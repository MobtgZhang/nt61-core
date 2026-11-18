//! user32 — input / system metrics

extern crate alloc;

use super::types::BOOL;
use crate::arch::common::syscall_frame::SyscallFrame;
use crate::libs::win32k::input_services;

pub use input_services::history::MouseMovePoint;

fn input_call(handler: extern "C" fn(&SyscallFrame) -> u64, arguments: &[u64]) -> u64 {
    let mut frame = SyscallFrame::default();
    frame.args[..arguments.len()].copy_from_slice(arguments);
    handler(&frame)
}

/// Windows 7 system metric indices.
pub use nt61_win32_metrics::sm;

/// Query the active display through the same Rust implementation as win32k.
pub extern "C" fn get_system_metrics(index: i32) -> i32 {
    input_call(
        crate::libs::win32k::system_metrics::nt_user_get_system_metrics,
        &[index as u32 as u64],
    ) as i32
}

/// Query the current native system COLORREF, including mutable palette changes.
pub extern "C" fn get_sys_color(index: i32) -> u32 {
    crate::libs::win32k::system_colors::get_sys_color(index)
}

/// Query instantaneous hardware state; bit 0 consumes the session press latch.
pub extern "C" fn get_async_key_state(vk: i32) -> i16 {
    input_services::keyboard::async_key_state(vk)
}

/// Query the calling thread's state at its last removed input message.
pub extern "C" fn get_key_state(vk: i32) -> i16 {
    input_call(
        input_services::keyboard::nt_user_get_key_state,
        &[vk as u64],
    ) as i16
}

/// Copy the calling thread's 256 virtual-key states.
///
/// # Safety
/// `buffer` must designate 256 writable bytes in the calling process.
pub unsafe extern "C" fn get_keyboard_state(buffer: *mut u8) -> BOOL {
    input_call(
        input_services::keyboard::nt_user_get_keyboard_state,
        &[buffer as u64],
    ) as BOOL
}

/// Replace only the calling thread's state, leaving hardware and other queues
/// unchanged. This does not change keyboard LEDs.
///
/// # Safety
/// `buffer` must designate 256 readable bytes in the calling process.
pub unsafe extern "C" fn set_keyboard_state(buffer: *const u8) -> BOOL {
    input_call(
        input_services::keyboard::nt_user_set_keyboard_state,
        &[buffer as u64],
    ) as BOOL
}

/// Read the shared cursor position in screen coordinates.
///
/// # Safety
/// `point` must designate two writable i32 coordinates in the calling process.
pub unsafe extern "C" fn get_cursor_pos(point: *mut i32) -> BOOL {
    BOOL::from(input_services::get_cursor(point as u64))
}

/// Move the cursor, respecting its clip rectangle and input-blocking owner.
pub extern "C" fn set_cursor_pos(x: i32, y: i32) -> BOOL {
    BOOL::from(input_services::set_cursor(x, y))
}

/// Query up to 64 retained pointer positions, newest first.
///
/// # Safety
/// `input` must point to a readable MouseMovePoint; `output` must accommodate
/// `count` writable entries. The buffers may overlap.
pub unsafe extern "C" fn get_mouse_move_points_ex(
    size: u32,
    input: *const MouseMovePoint,
    output: *mut MouseMovePoint,
    count: i32,
    resolution: u32,
) -> i32 {
    input_call(
        input_services::history::nt_user_get_mouse_move_points_ex,
        &[
            u64::from(size),
            input as u64,
            output as u64,
            count as u64,
            u64::from(resolution),
        ],
    ) as i32
}
/// `get_double_click_time` — return 500 ms.
pub extern "C" fn get_double_click_time() -> u32 {
    500
}
/// `get_message_extra_info` — return 0.
pub extern "C" fn get_message_extra_info() -> usize {
    0
}
/// Convert between a virtual key, scan code, or character in the active layout.
pub extern "C" fn map_virtual_key_w(code: u32, mode: u32) -> u32 {
    map_virtual_key_ex_w(code, mode, 0)
}

/// Convert using an installed layout. The native call has a separate keyboard
/// ID slot before the HKL; the public API does not expose that slot.
pub extern "C" fn map_virtual_key_ex_w(code: u32, mode: u32, layout: u64) -> u32 {
    input_call(
        input_services::layout::nt_user_map_virtual_key_ex,
        &[u64::from(code), u64::from(mode), 0, layout],
    ) as u32
}

/// Find the virtual key and Shift/Ctrl modifiers that produce one UTF-16 unit.
pub extern "C" fn vk_key_scan_ex_w(character: u16, layout: u64) -> i16 {
    input_call(
        input_services::layout::nt_user_vk_key_scan_ex,
        &[u64::from(character), layout, 1],
    ) as i16
}

/// Enumerate the installed keyboard layouts.
///
/// # Safety
/// `layouts` must hold `count` writable HKL slots when both are nonzero.
pub unsafe extern "C" fn get_keyboard_layout_list(count: i32, layouts: *mut u64) -> i32 {
    input_call(
        input_services::layout::nt_user_get_keyboard_layout_list,
        &[count as u64, layouts as u64],
    ) as i32
}

/// Convert a key using the supplied 256-byte keyboard state.
///
/// # Safety
/// `state` must hold 256 readable bytes and `output` must hold `count` writable
/// UTF-16 units. Output is not guaranteed to be NUL-terminated.
pub unsafe extern "C" fn to_unicode_ex(
    key: u32,
    scan: u32,
    state: *const u8,
    output: *mut u16,
    count: i32,
    flags: u32,
    layout: u64,
) -> i32 {
    input_call(
        input_services::layout::nt_user_to_unicode_ex,
        &[
            u64::from(key),
            u64::from(scan),
            state as u64,
            output as u64,
            count as u64,
            u64::from(flags),
            layout,
        ],
    ) as i32
}
/// Return the sole installed US layout handle.
pub extern "C" fn get_keyboard_layout(_id: u32) -> *mut core::ffi::c_void {
    input_services::layout::US_LAYOUT as *mut core::ffi::c_void
}
