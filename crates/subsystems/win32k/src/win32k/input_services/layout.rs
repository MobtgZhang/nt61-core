//! Translation for the installed US keyboard layout. Scan-code mapping is
//! independent of live NumLock/Shift state; character conversion uses the
//! caller's captured 256-byte state. Other layouts and IMEs need layout data.

use crate::arch::common::syscall_frame::SyscallFrame;
use crate::mm::user_copy::{read_user_object, write_user_object};

pub const US_LAYOUT: u64 = 0x0409_0409;

fn supported(layout: u64) -> bool {
    matches!(layout, 0 | 0x0409 | US_LAYOUT)
}

fn scan_key(code: u32) -> Option<u8> {
    match code {
        0xe11d => Some(0x13), // Pause
        0xe046 => Some(0x03), // Ctrl+Break
        0x54 => Some(0x2c),   // SysRq / Print Screen
        0..=0x7f => super::keyboard::scan_to_key(code as u8, false, &[0; 256]),
        0xe000..=0xe07f => super::keyboard::scan_to_key(code as u8, true, &[0; 256]),
        _ => None,
    }
}

/// MapVirtualKey modes 0 through 4 for the installed layout. Mapping a scan
/// code to a key never depends on whether a user currently holds Shift.
pub fn map_virtual_key(code: u32, mode: u32) -> u32 {
    match mode {
        0 | 4 => {
            let key = match code {
                0x10 => 0xa0, // generic modifier selects its left-hand key
                0x11 => 0xa2,
                0x12 => 0xa4,
                0x60..=0x69 => [0x2d, 0x23, 0x28, 0x22, 0x25, 0x0c, 0x27, 0x24, 0x26, 0x21]
                    [(code - 0x60) as usize],
                0x6e => 0x2e,
                0..=255 => code as u8,
                _ => return 0,
            };
            let scan = (1..=0x7f)
                .chain(0xe001..=0xe07f)
                .chain(core::iter::once(0xe11d))
                .find(|&scan| scan_key(scan) == Some(key))
                .unwrap_or(0);
            if mode == 0 {
                scan & 0xff
            } else {
                scan
            }
        }
        1 | 3 => {
            let Some(key) = scan_key(code) else { return 0 };
            u32::from(if mode == 1 {
                match key {
                    0xa0 | 0xa1 => 0x10,
                    0xa2 | 0xa3 => 0x11,
                    0xa4 | 0xa5 => 0x12,
                    _ => key,
                }
            } else {
                key
            })
        }
        2 if (0x41..=0x5a).contains(&code) => code,
        2 => unshifted(code).map_or(0, u32::from),
        _ => 0,
    }
}

fn unshifted(key: u32) -> Option<u16> {
    Some(match key {
        0x08 | 0x09 | 0x0d | 0x1b | 0x20 | 0x30..=0x39 => key as u16,
        0x41..=0x5a => (key + 0x20) as u16,
        0x60..=0x69 => (key - 0x60 + 0x30) as u16,
        0x6a => b'*' as u16,
        0x6b => b'+' as u16,
        0x6d => b'-' as u16,
        0x6e => b'.' as u16,
        0x6f => b'/' as u16,
        0xba => b';' as u16,
        0xbb => b'=' as u16,
        0xbc => b',' as u16,
        0xbd => b'-' as u16,
        0xbe => b'.' as u16,
        0xbf => b'/' as u16,
        0xc0 => b'`' as u16,
        0xdb => b'[' as u16,
        0xdc | 0xe2 => b'\\' as u16,
        0xdd => b']' as u16,
        0xde => b'\'' as u16,
        _ => return None,
    })
}

fn shifted(key: u32) -> Option<u16> {
    Some(match key {
        0x30..=0x39 => u16::from(b")!@#$%^&*("[(key - 0x30) as usize]),
        0x41..=0x5a => key as u16,
        0xba => b':' as u16,
        0xbb => b'+' as u16,
        0xbc => b'<' as u16,
        0xbd => b'_' as u16,
        0xbe => b'>' as u16,
        0xbf => b'?' as u16,
        0xc0 => b'~' as u16,
        0xdb => b'{' as u16,
        0xdc | 0xe2 => b'|' as u16,
        0xdd => b'}' as u16,
        0xde => b'"' as u16,
        _ => return unshifted(key),
    })
}

/// Convert one US key using the supplied synchronous keyboard state. `Some(0)`
/// is a valid Ctrl+2 character and must remain distinct from no translation.
pub fn character(key: u32, state: &[u8; 256]) -> Option<u16> {
    let shift = state[0x10] & 0x80 != 0;
    let control = state[0x11] & 0x80 != 0;
    if control {
        if state[0x12] & 0x80 != 0 {
            return None;
        }
        return match key {
            0x41..=0x5a => Some((key - 0x40) as u16),
            0x08 => Some(0x7f),
            0x0d if !shift => Some(0x0a),
            0x1b | 0x20 => Some(key as u16),
            0x32 => Some(0),
            0x36 if shift => Some(0x1e),
            0xbd if shift => Some(0x1f),
            0xdb => Some(0x1b),
            0xdc | 0xe2 => Some(0x1c),
            0xdd => Some(0x1d),
            _ => None,
        };
    }
    if (0x41..=0x5a).contains(&key) {
        let uppercase = shift ^ (state[0x14] & 1 != 0);
        return Some((key + if uppercase { 0 } else { 0x20 }) as u16);
    }
    if shift {
        shifted(key)
    } else {
        unshifted(key)
    }
}

pub extern "C" fn nt_user_map_virtual_key_ex(frame: &SyscallFrame) -> u64 {
    // Native ABI has keyboardId in slot 2 and HKL in slot 3.
    if !supported(frame.args[3]) {
        return 0;
    }
    u64::from(map_virtual_key(frame.args[0] as u32, frame.args[1] as u32))
}

pub extern "C" fn nt_user_vk_key_scan_ex(frame: &SyscallFrame) -> u64 {
    if frame.args[2] as u32 != 0 && !supported(frame.args[1]) {
        return u64::from(u32::MAX);
    }
    let value = frame.args[0] as u16;
    // Prefer main keyboard keys over numpad aliases and Ctrl combinations.
    for modifiers in [0u16, 1, 2, 3] {
        let mut state = [0; 256];
        state[0x10] = if modifiers & 1 != 0 { 0x80 } else { 0 };
        state[0x11] = if modifiers & 2 != 0 { 0x80 } else { 0 };
        for key in 1..=255 {
            if (0x60..=0x6f).contains(&key) || key == 0xe2 {
                continue;
            }
            if character(key, &state) == Some(value) {
                return u64::from((modifiers << 8) | key as u16);
            }
        }
    }
    u64::from(u32::MAX)
}

pub extern "C" fn nt_user_get_keyboard_layout_list(frame: &SyscallFrame) -> u64 {
    let count = frame.args[0] as i32;
    if count < 0 {
        super::set_last_error(87);
        return 0;
    }
    if count == 0 || frame.args[1] == 0 {
        return 1;
    }
    if write_user_object(frame.args[1], &US_LAYOUT).is_err() {
        super::set_last_error(998);
        return 0;
    }
    1
}

pub extern "C" fn nt_user_to_unicode_ex(frame: &SyscallFrame) -> u64 {
    if frame.args[4] as i32 <= 0 || !supported(frame.args[6]) {
        return 0;
    }
    let Ok(state) = read_user_object::<[u8; 256]>(frame.args[2]) else {
        super::set_last_error(998);
        return 0;
    };
    // Key-break conversion is opt-in. Win7 has no Win10 no-state-change flag.
    if frame.args[1] as u32 & 0x8000 != 0 && frame.args[5] as u32 & 2 == 0 {
        return 0;
    }
    let Some(value) = character(frame.args[0] as u32, &state) else {
        return 0;
    };
    if write_user_object(frame.args[3], &value).is_err() {
        super::set_last_error(998);
        return 0;
    }
    1
}

/// Translate a captured MSG and enqueue a character on the calling thread.
pub extern "C" fn nt_user_translate_message(frame: &SyscallFrame) -> u64 {
    let Ok(message) = read_user_object::<super::super::message::Msg>(frame.args[0]) else {
        super::set_last_error(998);
        return 0;
    };
    if !matches!(message.message, 0x100 | 0x101 | 0x104 | 0x105) {
        return 0;
    }
    if matches!(message.message, 0x100 | 0x104) {
        super::super::message::with_current_queue(|queue| {
            if let Some(translated) = queue.translate_keyboard_message(&message) {
                queue.post_message(translated);
            }
        });
    }
    // TranslateMessage reports a keyboard message even when it has no character.
    1
}
