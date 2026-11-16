//! Native window text operates on counted LARGE_UNICODE_STRING input and
//! NUL-terminated UTF-16 output. No caller pointer is retained by a window.
use super::WINDOW_MANAGER;
use crate::arch::common::syscall_frame::SyscallFrame;
use crate::mm::user_copy::{copy_from_user, copy_to_user, read_user_object};
use alloc::vec::Vec;

#[repr(C)]
#[derive(Clone, Copy)]
struct LargeString {
    length: u32,
    maximum_and_ansi: u32,
    buffer: u64,
}

fn fail(error: u32) -> u64 {
    super::super::input_services::set_last_error(error);
    0
}

pub extern "C" fn nt_user_def_set_text(frame: &SyscallFrame) -> u64 {
    let mut title = Vec::new();
    if frame.args[1] != 0 {
        let header = match read_user_object::<LargeString>(frame.args[1]) {
            Ok(header) => header,
            Err(_) => return fail(998),
        };
        // This backend accepts Unicode. ANSI conversion requires the caller's
        // active code page and must not reinterpret an ANSI buffer as UTF-16.
        if header.maximum_and_ansi & 0x8000_0000 != 0 {
            return fail(50);
        }
        if header.length & 1 != 0 || header.length > header.maximum_and_ansi {
            return fail(87);
        }
        if header.length > 65534 {
            return fail(8);
        }
        let mut bytes = Vec::new();
        if bytes.try_reserve_exact(header.length as usize).is_err()
            || title.try_reserve_exact(header.length as usize / 2).is_err()
        {
            return fail(8);
        }
        bytes.resize(header.length as usize, 0);
        if !bytes.is_empty() && copy_from_user(&mut bytes, header.buffer).is_err() {
            return fail(998);
        }
        title.extend(
            bytes
                .chunks_exact(2)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]])),
        );
    }
    let mut manager = WINDOW_MANAGER.lock();
    let Some(window) = manager.get_window_mut(frame.args[0]) else {
        return fail(1400);
    };
    window.title = title;
    window.dirty = true;
    1
}

pub extern "C" fn nt_user_internal_get_window_text(frame: &SyscallFrame) -> u64 {
    let capacity = frame.args[2] as i32;
    if capacity <= 0 {
        return 0;
    }
    let mut bytes = Vec::new();
    let copied = {
        let manager = WINDOW_MANAGER.lock();
        let Some(window) = manager.get_window(frame.args[0]) else {
            return fail(1400);
        };
        let length = window
            .title
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(window.title.len());
        let count = length.min(capacity as usize - 1);
        if bytes.try_reserve_exact((count + 1) * 2).is_err() {
            return fail(8);
        }
        for unit in &window.title[..count] {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        bytes.extend_from_slice(&0u16.to_le_bytes());
        count
    };
    if copy_to_user(frame.args[1], &bytes).is_err() {
        return fail(998);
    }
    copied as u64
}
