//! USER input state shared by native calls and class-driver events. Hardware
//! producers do not allocate. Blocking is released when its owning thread exits.
use super::{
    objects::Rect,
    window::{input_context::caller_identity, WINDOW_MANAGER},
};
use crate::arch::common::syscall_frame::SyscallFrame;
use crate::ke::sync::Spinlock;
use crate::mm::user_copy::{copy_to_user, read_user_object, write_user_object};

#[path = "input_services/history.rs"]
pub mod history;
#[path = "input_services/layout.rs"]
pub mod layout;
#[path = "input_services/keyboard.rs"]
pub mod keyboard;

struct InputState {
    clip: Option<Rect>,
    cursor: (i32, i32),
    blocker: Option<(u64, u64)>,
    history: history::MoveHistory,
    keyboard: keyboard::KeyboardState,
}
static INPUT: Spinlock<InputState> = Spinlock::new(InputState {
    clip: None,
    cursor: (0, 0),
    blocker: None,
    history: history::MoveHistory::new(),
    keyboard: keyboard::KeyboardState::new(),
});

pub fn screen_rect() -> Rect {
    let info = crate::hal::common::framebuffer::info();
    Rect::new(
        0,
        0,
        i32::try_from(info.width)
            .ok()
            .filter(|n| *n > 0)
            .unwrap_or(1024),
        i32::try_from(info.height)
            .ok()
            .filter(|n| *n > 0)
            .unwrap_or(768),
    )
}
fn clamp(point: (i32, i32), rect: Rect) -> (i32, i32) {
    (
        point
            .0
            .clamp(rect.left, rect.right.saturating_sub(1).max(rect.left)),
        point
            .1
            .clamp(rect.top, rect.bottom.saturating_sub(1).max(rect.top)),
    )
}
pub fn hardware_allowed() -> bool {
    INPUT.lock().blocker.is_none()
}
pub fn move_hardware_cursor(dx: i16, dy: i16) -> (i32, i32) {
    let mut input = INPUT.lock();
    if input.blocker.is_none() {
        input.cursor = clamp(
            (
                input.cursor.0.saturating_add(i32::from(dx)),
                input.cursor.1.saturating_add(i32::from(dy)),
            ),
            input.clip.unwrap_or_else(screen_rect),
        );
        let position = input.cursor;
        input.history.record(position, None, 0);
    }
    input.cursor
}
/// Update the shared cursor from a port driver's screen coordinates.
pub fn position_hardware_cursor(x: i32, y: i32) -> (i32, i32) {
    position_hardware_cursor_with_resolution(x, y, None)
}

/// Preserve an absolute device's normalized coordinates before pixel rounding.
pub fn position_hardware_cursor_with_resolution(
    x: i32,
    y: i32,
    high_resolution: Option<(u16, u16)>,
) -> (i32, i32) {
    let mut input = INPUT.lock();
    if input.blocker.is_none() {
        input.cursor = clamp((x, y), input.clip.unwrap_or_else(screen_rect));
        let position = input.cursor;
        // Clipping changes the effective pointer location, so a raw tablet
        // coordinate outside the clip no longer describes that location.
        let high_resolution = high_resolution.filter(|_| position == (x, y));
        input.history.record(position, high_resolution, 0);
    }
    input.cursor
}

pub fn cursor_position() -> (i32, i32) {
    INPUT.lock().cursor
}
pub fn release_thread(thread: u64) {
    let mut input = INPUT.lock();
    if input.blocker.is_some_and(|owner| owner.1 == thread) {
        input.blocker = None;
    }
}
pub fn set_cursor(x: i32, y: i32) -> bool {
    let owner = caller_identity();
    let mut input = INPUT.lock();
    if input.blocker.is_some_and(|blocker| blocker != owner) {
        return false;
    }
    input.cursor = clamp((x, y), input.clip.unwrap_or_else(screen_rect));
    let position = input.cursor;
    input.history.record(position, None, 0);
    true
}
pub fn get_cursor(pointer: u64) -> bool {
    let point = INPUT.lock().cursor;
    write_user_object(pointer, &[point.0, point.1]).is_ok()
}
pub extern "C" fn nt_user_clip_cursor(frame: &SyscallFrame) -> u64 {
    let clip = if frame.args[0] == 0 {
        None
    } else {
        let Ok(rect) = read_user_object::<Rect>(frame.args[0]) else {
            return 0;
        };
        if rect.left > rect.right || rect.top > rect.bottom {
            return 0;
        }
        let Some(rect) = screen_rect().intersect(&rect) else {
            return 0;
        };
        Some(rect)
    };
    let mut input = INPUT.lock();
    if input
        .blocker
        .is_some_and(|owner| owner != caller_identity())
    {
        return 0;
    }
    input.clip = clip;
    input.cursor = clamp(input.cursor, clip.unwrap_or_else(screen_rect));
    1
}
pub extern "C" fn nt_user_get_clip_cursor(frame: &SyscallFrame) -> u64 {
    let rect = INPUT.lock().clip.unwrap_or_else(screen_rect);
    u64::from(write_user_object(frame.args[0], &rect).is_ok())
}
pub extern "C" fn nt_user_block_input(frame: &SyscallFrame) -> u64 {
    let owner = caller_identity();
    let mut input = INPUT.lock();
    if frame.args[0] != 0 {
        if input.blocker.is_some() {
            return 0;
        }
        input.blocker = Some(owner);
    } else {
        if input.blocker != Some(owner) {
            return 0;
        }
        input.blocker = None;
    }
    1
}
pub extern "C" fn nt_user_get_keyboard_layout_name(frame: &SyscallFrame) -> u64 {
    // The installed class layout is US English. A KLID is eight UTF-16 digits
    // followed by a NUL, independent of the caller's locale.
    u64::from(write_user_object(frame.args[0], &[48u16, 48, 48, 48, 48, 52, 48, 57, 0]).is_ok())
}
pub extern "C" fn nt_user_unload_keyboard_layout(_: &SyscallFrame) -> u64 {
    // The only installed layout is the system default and cannot be unloaded.
    0
}
pub extern "C" fn nt_user_get_key_name_text(frame: &SyscallFrame) -> u64 {
    let count = frame.args[2] as i32;
    if count <= 0 {
        return 0;
    }
    let scan = (frame.args[0] >> 16) as u8;
    let extended = frame.args[0] & (1 << 24) != 0;
    let name = match (scan, extended) {
        (1, _) => "Esc",
        (14, _) => "Backspace",
        (15, _) => "Tab",
        (28, false) => "Enter",
        (28, true) => "Num Enter",
        (29, false) => "Ctrl",
        (29, true) => "Right Ctrl",
        (42, _) | (54, _) => "Shift",
        (56, false) => "Alt",
        (56, true) => "Right Alt",
        (57, _) => "Space",
        (58, _) => "Caps Lock",
        (69, _) => "Num Lock",
        (70, _) => "Scroll Lock",
        (71, true) => "Home",
        (72, true) => "Up",
        (73, true) => "Page Up",
        (75, true) => "Left",
        (77, true) => "Right",
        (79, true) => "End",
        (80, true) => "Down",
        (81, true) => "Page Down",
        (82, true) => "Insert",
        (83, true) => "Delete",
        (59, _) => "F1",
        (60, _) => "F2",
        (61, _) => "F3",
        (62, _) => "F4",
        (63, _) => "F5",
        (64, _) => "F6",
        (65, _) => "F7",
        (66, _) => "F8",
        (67, _) => "F9",
        (68, _) => "F10",
        (87, _) => "F11",
        (88, _) => "F12",
        _ => "",
    };
    let mut output = [0u16; 32];
    let mut length = 0;
    if name.is_empty() {
        const SCANS: &[u8] = b"\x02\x03\x04\x05\x06\x07\x08\x09\x0a\x0b\x0c\x0d\x10\x11\x12\x13\x14\x15\x16\x17\x18\x19\x1a\x1b\x1e\x1f\x20\x21\x22\x23\x24\x25\x26\x27\x28\x29\x2b\x2c\x2d\x2e\x2f\x30\x31\x32\x33\x34\x35";
        const NAMES: &[u8] = b"1234567890-=QWERTYUIOP[]ASDFGHJKL;'`\\ZXCVBNM,./";
        if let Some(index) = SCANS.iter().position(|&value| value == scan) {
            output[0] = u16::from(NAMES[index]);
            length = 1;
        }
    } else {
        for (slot, value) in output.iter_mut().zip(name.encode_utf16()) {
            *slot = value;
            length += 1;
        }
    }
    length = length.min(count as usize - 1);
    output[length] = 0;
    // SAFETY: a UTF-16 array has no padding and the captured prefix is initialized.
    let bytes =
        unsafe { core::slice::from_raw_parts(output.as_ptr().cast::<u8>(), (length + 1) * 2) };
    if copy_to_user(frame.args[1], bytes).is_err() {
        return 0;
    }
    length as u64
}
pub extern "C" fn nt_user_get_thread_state(frame: &SyscallFrame) -> u64 {
    let (process, thread) = caller_identity();
    let mut manager = WINDOW_MANAGER.lock();
    let own = |hwnd| {
        manager
            .get_window(hwnd)
            .filter(|w| w.thread_id == thread && w.process_id == process)
            .map_or(0, |w| w.hwnd)
    };
    match frame.args[0] as u32 {
        0 => own(manager.get_focus()),
        1 => own(manager.get_active_window()),
        2 => own(manager.get_capture()),
        3 => 0, // No IME server window exists for the built-in US layout.
        4 => manager.default_input_context(process, thread).unwrap_or(0),
        5 => {
            drop(manager);
            u64::from(super::message::with_current_queue(|queue| {
                queue.peek_has_messages()
            }))
        }
        7 | 8 | 9 => 0,
        6 => {
            drop(manager);
            super::user_state::with_user_state(|state| state.current_cursor())
        }
        10 => u64::from(
            manager
                .get_window(manager.get_foreground_window())
                .is_some_and(|w| w.thread_id == thread),
        ),
        _ => 0,
    }
}
pub extern "C" fn nt_user_get_caret_pos(frame: &SyscallFrame) -> u64 {
    let (_, thread) = caller_identity();
    let point = super::user_state::with_user_state(|state| state.caret_position(thread));
    u64::from(write_user_object(frame.args[0], &point.unwrap_or([0, 0])).is_ok())
}

/// Publish a USER failure in the calling thread's NT64 TEB without directly
/// dereferencing a user address. An unmapped TEB must not fault the kernel.
pub(super) fn set_last_error(error: u32) {
    let thread = crate::ps::thread::get_current_ethread();
    // SAFETY: the scheduler pins the current ETHREAD during its system call.
    if let Some(thread) = unsafe { thread.as_ref() } {
        let teb = thread.kthread.teb as u64;
        if teb != 0 {
            if let Some(address) = teb.checked_add(0x68) {
                let _ = write_user_object(address, &error);
            }
        }
    }
}
