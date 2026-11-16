//! Clipboard format queries capture one consistent format snapshot. They do
//! not need the clipboard to be open and never expose storage owned by it.

use super::formats;
use crate::arch::common::syscall_frame::SyscallFrame;
use crate::mm::user_copy::{copy_from_user, copy_to_user, probe_user_write, write_user_object};
use alloc::vec::Vec;

/// GetPriorityClipboardFormat returns zero for an empty clipboard, -1 when
/// none of the caller's formats is available, and otherwise the first match
/// in the caller's priority order (independent of insertion order).
pub extern "C" fn nt_user_get_priority_clipboard_format(frame: &SyscallFrame) -> u64 {
    let available = formats();
    if available.is_empty() {
        return 0;
    }
    let count = frame.args[1] as i32;
    if count < 0 {
        return u32::MAX as u64;
    }
    // Capture in bounded chunks, so an untrusted count cannot request a large
    // kernel allocation. Validate the full range before selecting any format.
    let Some(length) = (count as usize).checked_mul(4) else {
        return u32::MAX as u64;
    };
    if length != 0 && crate::mm::user_copy::probe_user_read(frame.args[0], length).is_err() {
        return u32::MAX as u64;
    }
    let mut selected = None;
    let mut offset = 0;
    while offset < length {
        let mut bytes = [0u8; 256];
        let size = (length - offset).min(bytes.len());
        let Some(address) = frame.args[0].checked_add(offset as u64) else {
            return u32::MAX as u64;
        };
        if copy_from_user(&mut bytes[..size], address).is_err() {
            return u32::MAX as u64;
        }
        for word in bytes[..size].chunks_exact(4) {
            let format = u32::from_le_bytes([word[0], word[1], word[2], word[3]]);
            if selected.is_none() && available.contains(&format) {
                selected = Some(format);
            }
        }
        offset += size;
    }
    u64::from(selected.unwrap_or(u32::MAX))
}

/// Report all formats currently available. On insufficient capacity the count
/// still reports the required number and the format array is left untouched.
pub extern "C" fn nt_user_get_updated_clipboard_formats(frame: &SyscallFrame) -> u64 {
    let available = formats();
    let count = available.len() as u32;
    if probe_user_write(frame.args[2], 4).is_err() {
        return 0;
    }
    if write_user_object(frame.args[2], &count).is_err() {
        return 0;
    }
    if (frame.args[1] as u32 as usize) < available.len() {
        return 0;
    }
    if available.is_empty() {
        return 1;
    }
    let mut bytes = Vec::new();
    if bytes.try_reserve_exact(available.len() * 4).is_err() {
        return 0;
    }
    for format in available {
        bytes.extend_from_slice(&format.to_le_bytes());
    }
    u64::from(copy_to_user(frame.args[0], &bytes).is_ok())
}

pub extern "C" fn nt_user_add_clipboard_format_listener(frame: &SyscallFrame) -> u64 {
    let manager = crate::libs::win32k::window::WINDOW_MANAGER.lock();
    if manager.get_window(frame.args[0]).is_none() { return 0; }
    let mut inner = super::CLIPBOARD.data.lock();
    if inner.listeners.contains(&frame.args[0]) || inner.listeners.try_reserve(1).is_err() { return 0; }
    inner.listeners.push(frame.args[0]);
    1
}

pub extern "C" fn nt_user_remove_clipboard_format_listener(frame: &SyscallFrame) -> u64 {
    let mut inner = super::CLIPBOARD.data.lock();
    let Some(index) = inner.listeners.iter().position(|hwnd| *hwnd == frame.args[0]) else { return 0 };
    inner.listeners.remove(index);
    1
}

pub extern "C" fn nt_user_get_clipboard_viewer(_: &SyscallFrame) -> u64 {
    super::CLIPBOARD.data.lock().viewer
}

pub extern "C" fn nt_user_set_clipboard_viewer(frame: &SyscallFrame) -> u64 {
    let manager = crate::libs::win32k::window::WINDOW_MANAGER.lock();
    if manager.get_window(frame.args[0]).is_none() { return 0; }
    let previous = {
        let mut inner = super::CLIPBOARD.data.lock();
        if inner.viewer == frame.args[0] { return 0; }
        core::mem::replace(&mut inner.viewer, frame.args[0])
    };
    drop(manager);
    crate::libs::win32k::message::post_message(frame.args[0], 0x0308, 0, 0);
    previous
}

pub extern "C" fn nt_user_change_clipboard_chain(frame: &SyscallFrame) -> u64 {
    let viewer = {
        let mut inner = super::CLIPBOARD.data.lock();
        if inner.viewer == frame.args[0] { inner.viewer = frame.args[1]; return 1; }
        inner.viewer
    };
    if viewer != 0 {
        crate::libs::win32k::message::post_message(viewer, 0x030d, frame.args[0], frame.args[1] as i64);
    }
    1
}

pub extern "C" fn nt_user_get_open_clipboard_window(_: &SyscallFrame) -> u64 { super::get_open_window() }
pub extern "C" fn nt_user_get_clipboard_owner(_: &SyscallFrame) -> u64 { super::get_owner() }
