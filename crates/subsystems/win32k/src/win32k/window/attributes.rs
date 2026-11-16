//! Width-correct window attributes and property snapshots.
use super::{WindowStyle, MAX_WINDOWS, WINDOW_MANAGER};
use crate::arch::common::syscall_frame::SyscallFrame;
use crate::mm::user_copy::{copy_to_user, write_user_object};
use alloc::vec::Vec;

pub extern "C" fn nt_user_build_prop_list(frame: &SyscallFrame) -> u64 {
    let (bytes, count) = {
        let manager = WINDOW_MANAGER.lock();
        let Some(window) = manager.get_window(frame.args[0]) else {
            return 0xc0000008;
        };
        if frame.args[1] == 0 {
            (Vec::new(), (window.properties.len() * 16) as u32)
        } else {
            let length = frame.args[2] as u32 as usize;
            if length == 0 || length % 16 != 0 {
                return 0xc000000d;
            }
            let count = (length / 16).min(window.properties.len());
            let mut bytes = Vec::new();
            if bytes.try_reserve_exact(count * 16).is_err() {
                return 0xc000009a;
            }
            for &(atom, value) in &window.properties[..count] {
                bytes.extend_from_slice(&u64::from(atom).to_le_bytes());
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            (bytes, count as u32)
        }
    };
    if !bytes.is_empty() {
        if let Err(status) = copy_to_user(frame.args[1], &bytes) {
            return status as u32 as u64;
        }
    }
    if frame.args[3] != 0 {
        if let Err(status) = write_user_object(frame.args[3], &count) {
            return status as u32 as u64;
        }
    }
    0
}

fn set_window_attribute(frame: &SyscallFrame, width: usize) -> u64 {
    let mut manager = WINDOW_MANAGER.lock();
    let hwnd = frame.args[0];
    let index = frame.args[1] as i32;
    if index == -8 {
        // GWLP_HWNDPARENT: owner for top-level windows.
        let Some(window) = manager.get_window(hwnd) else {
            return 0;
        };
        if window.style & WindowStyle::CHILD != 0 {
            return 0;
        }
        let owner = frame.args[2];
        let mut current = (owner != 0).then_some(owner);
        for depth in 0..MAX_WINDOWS {
            match current {
                Some(handle) if handle == hwnd => return 0,
                Some(handle) => {
                    let Some(window) = manager.get_window(handle) else {
                        return 0;
                    };
                    current = window.owner;
                }
                None => break,
            }
            if depth + 1 == MAX_WINDOWS {
                return 0;
            }
        }
        let window = manager
            .get_window_mut(hwnd)
            .expect("guard retains validated window");
        let old = window.owner.unwrap_or(0);
        window.owner = (owner != 0).then_some(owner);
        return old;
    }
    let Some(window) = manager.get_window_mut(hwnd) else {
        return 0;
    };
    let value = if width == 8 {
        frame.args[2]
    } else if width == 4 {
        frame.args[2] as u32 as u64
    } else {
        frame.args[2] as u16 as u64
    };
    let old = match index {
        -4 if width != 2 => core::mem::replace(&mut window.wndproc, value),
        -6 => core::mem::replace(&mut window.instance, value),
        -12 => u64::from(core::mem::replace(&mut window.control_id, value as u32)),
        -16 if width != 2 => {
            let old = core::mem::replace(&mut window.style, value as u32);
            window.visible = window.style & WindowStyle::VISIBLE != 0;
            window.enabled = window.style & WindowStyle::DISABLED == 0;
            window.update_client_rect();
            old.into()
        }
        -20 if width != 2 => u64::from(core::mem::replace(&mut window.ex_style, value as u32)),
        -21 if width != 2 => core::mem::replace(&mut window.user_data, value),
        offset if offset >= 0 => {
            let start = offset as usize;
            let Some(end) = start.checked_add(width) else {
                return 0;
            };
            let Some(bytes) = window.extra.get_mut(start..end) else {
                return 0;
            };
            let mut previous = [0u8; 8];
            previous[..width].copy_from_slice(bytes);
            bytes.copy_from_slice(&value.to_le_bytes()[..width]);
            u64::from_le_bytes(previous)
        }
        _ => return 0,
    };
    window.dirty = true;
    if width == 4 {
        old as u32 as u64
    } else if width == 2 {
        old as u16 as u64
    } else {
        old
    }
}

pub extern "C" fn nt_user_set_window_long_ptr(frame: &SyscallFrame) -> u64 {
    set_window_attribute(frame, 8)
}
pub extern "C" fn nt_user_set_window_long(frame: &SyscallFrame) -> u64 {
    set_window_attribute(frame, 4)
}
pub extern "C" fn nt_user_set_window_word(frame: &SyscallFrame) -> u64 {
    set_window_attribute(frame, 2)
}

pub extern "C" fn nt_user_window_from_physical_point(frame: &SyscallFrame) -> u64 {
    let manager = WINDOW_MANAGER.lock();
    let (x, y) = (
        i64::from(frame.args[0] as i32),
        i64::from(frame.args[1] as i32),
    );
    // Sibling order is independent of storage slot reuse. Select a top-level
    // window before descending, so a child behind another root cannot win.
    let hit = |hwnd| {
        let window = manager.get_window(hwnd)?;
        if !window.visible || !window.enabled { return None; }
        let (left, top) = manager.client_screen_origin(window.parent)?;
        (x >= left + i64::from(window.rect.left)
            && x < left + i64::from(window.rect.right)
            && y >= top + i64::from(window.rect.top)
            && y < top + i64::from(window.rect.bottom)).then_some(window)
    };
    let mut selected = manager.z_order.iter().rev().copied().find(|hwnd| {
        manager.get_window(*hwnd).is_some_and(|window| window.parent.is_none())
            && hit(*hwnd).is_some()
    });
    for _ in 0..MAX_WINDOWS {
        let Some(hwnd) = selected else { return 0 };
        let Some(window) = manager.get_window(hwnd) else { return 0 };
        let Some(origin) = manager.client_screen_origin(Some(hwnd)) else { return 0 };
        let width = i64::from(window.client_rect.right) - i64::from(window.client_rect.left);
        let height = i64::from(window.client_rect.bottom) - i64::from(window.client_rect.top);
        if x < origin.0 || y < origin.1 || x >= origin.0 + width || y >= origin.1 + height {
            return hwnd;
        }
        match window.children.iter().rev().copied().find(|child| hit(*child).is_some()) {
            Some(child) => selected = Some(child),
            None => return hwnd,
        }
    }
    0
}
