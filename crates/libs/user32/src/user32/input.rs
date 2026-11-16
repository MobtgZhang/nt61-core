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

/// System metrics. The bootstrap reports a deterministic
/// 1024x768 screen.
pub mod sm {
    pub const SM_CXSCREEN: i32 = 0;
    pub const SM_CYSCREEN: i32 = 1;
    pub const SM_CXVSCROLL: i32 = 2;
    pub const SM_CYHSCROLL: i32 = 3;
    pub const SM_CYCAPTION: i32 = 4;
    pub const SM_CXBORDER: i32 = 5;
    pub const SM_CYBORDER: i32 = 6;
    pub const SM_CXDLGFRAME: i32 = 7;
    pub const SM_CYDLGFRAME: i32 = 8;
    pub const SM_CYVTHUMB: i32 = 9;
    pub const SM_CXHTHUMB: i32 = 10;
    pub const SM_CXICON: i32 = 11;
    pub const SM_CYICON: i32 = 12;
    pub const SM_CXCURSOR: i32 = 13;
    pub const SM_CYCURSOR: i32 = 14;
    pub const SM_CYMENU: i32 = 15;
    pub const SM_CXFULLSCREEN: i32 = 16;
    pub const SM_CYFULLSCREEN: i32 = 17;
    pub const SM_CYKANJIWINDOW: i32 = 18;
    pub const SM_MOUSEPRESENT: i32 = 19;
    pub const SM_CYVSCROLL: i32 = 20;
    pub const SM_CXHSCROLL: i32 = 21;
    pub const SM_DEBUG: i32 = 22;
    pub const SM_SWAPBUTTON: i32 = 23;
    pub const SM_RESERVED1: i32 = 24;
    pub const SM_RESERVED2: i32 = 25;
    pub const SM_RESERVED3: i32 = 26;
    pub const SM_RESERVED4: i32 = 27;
    pub const SM_CXMIN: i32 = 28;
    pub const SM_CYMIN: i32 = 29;
    pub const SM_CXSIZE: i32 = 30;
    pub const SM_CYSIZE: i32 = 31;
    pub const SM_CXFRAME: i32 = 32;
    pub const SM_CYFRAME: i32 = 33;
    pub const SM_CXMINTRACK: i32 = 34;
    pub const SM_CYMINTRACK: i32 = 35;
    pub const SM_CXDOUBLECLK: i32 = 36;
    pub const SM_CYDOUBLECLK: i32 = 37;
    pub const SM_CXICONSPACING: i32 = 38;
    pub const SM_CYICONSPACING: i32 = 39;
    pub const SM_MENUDROPALIGNMENT: i32 = 40;
    pub const SM_PENWINDOWS: i32 = 41;
    pub const SM_DBCSENABLED: i32 = 42;
    pub const SM_CMOUSEBUTTONS: i32 = 43;
    pub const SM_CXFIXEDFRAME: i32 = 7;
    pub const SM_CYFIXEDFRAME: i32 = 8;
    pub const SM_CXSIZEFRAME: i32 = 32;
    pub const SM_CYSIZEFRAME: i32 = 33;
    pub const SM_SECURE: i32 = 44;
    pub const SM_CXEDGE: i32 = 45;
    pub const SM_CYEDGE: i32 = 46;
    pub const SM_CXMINSPACING: i32 = 47;
    pub const SM_CYMINSPACING: i32 = 48;
    pub const SM_CXSMICON: i32 = 49;
    pub const SM_CYSMICON: i32 = 50;
    pub const SM_CYSMCAPTION: i32 = 51;
    pub const SM_CXSMSIZE: i32 = 52;
    pub const SM_CYSMSIZE: i32 = 53;
    pub const SM_CXMENUSIZE: i32 = 54;
    pub const SM_CYMENUSIZE: i32 = 55;
    pub const SM_ARRANGE: i32 = 56;
    pub const SM_CXMINIMIZED: i32 = 57;
    pub const SM_CYMINIMIZED: i32 = 58;
    pub const SM_CXMAXTRACK: i32 = 59;
    pub const SM_CYMAXTRACK: i32 = 60;
    pub const SM_CXMAXIMIZED: i32 = 61;
    pub const SM_CYMAXIMIZED: i32 = 62;
    pub const SM_NETWORK: i32 = 63;
    pub const SM_CLEANBOOT: i32 = 67;
    pub const SM_CXDRAG: i32 = 68;
    pub const SM_CYDRAG: i32 = 69;
    pub const SM_SHOWSOUNDS: i32 = 70;
    pub const SM_CXMENUCHECK: i32 = 71;
    pub const SM_CYMENUCHECK: i32 = 72;
    pub const SM_SLOWMACHINE: i32 = 73;
    pub const SM_MIDEASTENABLED: i32 = 74;
    pub const SM_MOUSEWHEELPRESENT: i32 = 75;
    pub const SM_XVIRTUALSCREEN: i32 = 76;
    pub const SM_YVIRTUALSCREEN: i32 = 77;
    pub const SM_CXVIRTUALSCREEN: i32 = 78;
    pub const SM_CYVIRTUALSCREEN: i32 = 79;
    pub const SM_CMONITORS: i32 = 80;
    pub const SM_SAMEDISPLAYFORMAT: i32 = 81;
    pub const SM_IMMENABLED: i32 = 82;
    pub const SM_CXFOCUSBORDER: i32 = 83;
    pub const SM_CYFOCUSBORDER: i32 = 84;
    pub const SM_TABLETPC: i32 = 86;
    pub const SM_MEDIACENTER: i32 = 87;
    pub const SM_STARTER: i32 = 88;
    pub const SM_SERVERR2: i32 = 89;
    pub const SM_MOUSEHORIZONTALWHEELPRESENT: i32 = 91;
    pub const SM_CXPADDEDBORDER: i32 = 92;
    pub const SM_DIGITIZER: i32 = 94;
    pub const SM_MAXIMUMTOUCHES: i32 = 95;
    pub const SM_CMETRICS: i32 = 97;
}

/// `get_system_metrics` — return deterministic values.
pub extern "C" fn get_system_metrics(index: i32) -> i32 {
    match index {
        sm::SM_CXSCREEN => 1024,
        sm::SM_CYSCREEN => 768,
        sm::SM_CXVSCROLL => 16,
        sm::SM_CYHSCROLL => 16,
        sm::SM_CYCAPTION => 22,
        sm::SM_CXBORDER | sm::SM_CYFIXEDFRAME => 1,
        sm::SM_CYBORDER | sm::SM_CXFIXEDFRAME => 1,
        // SM_CXDLGFRAME/SM_CYDLGFRAME share values with the
        // *FIXEDFRAME constants above and are handled there.
        sm::SM_CXICON | sm::SM_CYICON => 32,
        sm::SM_CXCURSOR | sm::SM_CYCURSOR => 32,
        sm::SM_CYMENU => 18,
        sm::SM_CXFULLSCREEN => 1024,
        sm::SM_CYFULLSCREEN => 728,
        sm::SM_MOUSEPRESENT => 1,
        sm::SM_MOUSEWHEELPRESENT => 1,
        sm::SM_CMOUSEBUTTONS => 3,
        sm::SM_CXMAXIMIZED => 1024,
        sm::SM_CYMAXIMIZED => 728,
        sm::SM_CXMINIMIZED => 160,
        sm::SM_CYMINIMIZED => 28,
        sm::SM_CXMINTRACK => 112,
        sm::SM_CYMINTRACK => 27,
        sm::SM_CXDOUBLECLK => 4,
        sm::SM_CYDOUBLECLK => 4,
        sm::SM_CXICONSPACING => 75,
        sm::SM_CYICONSPACING => 75,
        sm::SM_CXEDGE => 3,
        sm::SM_CYEDGE => 3,
        sm::SM_CXSMICON => 16,
        sm::SM_CYSMICON => 16,
        sm::SM_CYSMCAPTION => 20,
        sm::SM_CXSMSIZE => 16,
        sm::SM_CYSMSIZE => 16,
        sm::SM_CXMENUSIZE => 18,
        sm::SM_CYMENUSIZE => 18,
        sm::SM_CXDRAG => 4,
        sm::SM_CYDRAG => 4,
        sm::SM_CXMAXTRACK => 1024,
        sm::SM_CYMAXTRACK => 768,
        sm::SM_CXFRAME => 4,
        sm::SM_CYFRAME => 4,
        sm::SM_CMETRICS => 97,
        sm::SM_CMONITORS => 1,
        sm::SM_SAMEDISPLAYFORMAT => 1,
        _ => 0,
    }
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
