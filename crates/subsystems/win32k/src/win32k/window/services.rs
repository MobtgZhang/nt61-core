//! Native USER property and hierarchy services.
//!
//! All graph traversal and mutations retain the window manager guard. User
//! addresses are never stored in the graph. DPI virtualization, cross-process
//! integrity checks and callback-based hit testing remain separate work.

use super::{WindowManager, WindowStyleEx, MAX_WINDOWS, WINDOW_MANAGER};
use crate::arch::common::syscall_frame::SyscallFrame;

pub extern "C" fn nt_user_get_prop(frame: &SyscallFrame) -> u64 {
    let manager = WINDOW_MANAGER.lock();
    manager.get_window(frame.args[0])
        .and_then(|window| window.properties.iter()
            .find(|(atom, _)| *atom == frame.args[1] as u16))
        .map_or(0, |(_, value)| *value)
}

pub extern "C" fn nt_user_set_prop(frame: &SyscallFrame) -> u64 {
    let mut manager = WINDOW_MANAGER.lock();
    let Some(window) = manager.get_window_mut(frame.args[0]) else { return 0 };
    let atom = frame.args[1] as u16;
    if let Some((_, value)) = window.properties.iter_mut().find(|(key, _)| *key == atom) {
        *value = frame.args[2];
    } else {
        if window.properties.try_reserve(1).is_err() { return 0; }
        window.properties.push((atom, frame.args[2]));
    }
    1
}

pub extern "C" fn nt_user_remove_prop(frame: &SyscallFrame) -> u64 {
    let mut manager = WINDOW_MANAGER.lock();
    let Some(window) = manager.get_window_mut(frame.args[0]) else { return 0 };
    let Some(index) = window.properties.iter().position(|(atom, _)| *atom == frame.args[1] as u16)
        else { return 0 };
    window.properties.remove(index).1
}

impl WindowManager {
    /// Walk parents separately from owners. The bound also prevents a corrupt
    /// graph from turning a syscall into an unbounded kernel loop.
    pub fn ancestor(&self, hwnd: u64, flags: u32) -> Option<u64> {
        let window = self.get_window(hwnd)?;
        let desktop = self.desktop_window.load(core::sync::atomic::Ordering::Acquire);
        if hwnd == desktop { return None; }
        if flags == 1 { return window.parent.or((desktop != 0).then_some(desktop)); }
        if flags != 2 && flags != 3 { return None; }
        let mut current = hwnd;
        for _ in 0..MAX_WINDOWS {
            let window = self.get_window(current)?;
            let next = window.parent.filter(|parent| *parent != desktop)
                .or_else(|| if flags == 3 { window.owner } else { None });
            match next {
                Some(next) if next != current => current = next,
                None => return Some(current),
                _ => return None,
            }
        }
        None
    }

    /// CWP uses client coordinates, half-open bounds and immediate children.
    /// Children are inserted at the top of the sibling stack by CreateWindow.
    pub fn child_from_point(&self, parent: u64, x: i32, y: i32, flags: u32) -> Option<u64> {
        if flags & !7 != 0 { return None; }
        let window = self.get_window(parent)?;
        let width = i64::from(window.client_rect.right) - i64::from(window.client_rect.left);
        let height = i64::from(window.client_rect.bottom) - i64::from(window.client_rect.top);
        if x < 0 || y < 0 || i64::from(x) >= width || i64::from(y) >= height {
            return None;
        }
        for hwnd in window.children.iter().rev() {
            let Some(child) = self.get_window(*hwnd) else { continue };
            if flags & 1 != 0 && !child.visible
                || flags & 2 != 0 && !child.enabled
                || flags & 4 != 0 && child.ex_style & WindowStyleEx::TRANSPARENT != 0 {
                continue;
            }
            if child.rect.contains_point(x, y) { return Some(*hwnd); }
        }
        Some(parent)
    }

    /// Reparent atomically; SetParent does not toggle WS_CHILD/WS_POPUP.
    /// Window positions are converted so their screen location is preserved.
    pub fn reparent(&mut self, hwnd: u64, new_parent: Option<u64>) -> Option<u64> {
        let old_parent = self.get_window(hwnd)?.parent;
        let origin = self.client_screen_origin(old_parent)?;
        let next_origin = self.client_screen_origin(new_parent)?;
        let mut current = new_parent;
        for depth in 0..MAX_WINDOWS {
            match current {
                Some(handle) if handle == hwnd => return None,
                Some(handle) => current = self.get_window(handle)?.parent,
                None => break,
            }
            if depth + 1 == MAX_WINDOWS { return None; }
        }
        if old_parent == new_parent { return Some(old_parent.unwrap_or(0)); }
        let rect = self.get_window(hwnd)?.rect;
        let left = i32::try_from(i64::from(rect.left) + origin.0 - next_origin.0).ok()?;
        let top = i32::try_from(i64::from(rect.top) + origin.1 - next_origin.1).ok()?;
        let right = i32::try_from(i64::from(rect.right) + origin.0 - next_origin.0).ok()?;
        let bottom = i32::try_from(i64::from(rect.bottom) + origin.1 - next_origin.1).ok()?;
        if let Some(parent) = new_parent {
            self.get_window_mut(parent)?.children.try_reserve(1).ok()?;
        }
        if let Some(parent) = old_parent {
            self.get_window_mut(parent)?.children.retain(|child| *child != hwnd);
        }
        if let Some(parent) = new_parent { self.get_window_mut(parent)?.children.push(hwnd); }
        let window = self.get_window_mut(hwnd)?;
        window.parent = new_parent;
        window.rect = super::Rect { left, top, right, bottom };
        window.dirty = true;
        Some(old_parent.unwrap_or(0))
    }

    pub(super) fn client_screen_origin(&self, hwnd: Option<u64>) -> Option<(i64, i64)> {
        let (mut x, mut y) = (0i64, 0i64);
        let mut current = hwnd;
        for _ in 0..MAX_WINDOWS {
            let Some(handle) = current else { return Some((x, y)) };
            let window = self.get_window(handle)?;
            x += i64::from(window.rect.left) + i64::from(window.client_rect.left);
            y += i64::from(window.rect.top) + i64::from(window.client_rect.top);
            current = window.parent;
        }
        None
    }
}

pub extern "C" fn nt_user_get_ancestor(frame: &SyscallFrame) -> u64 {
    WINDOW_MANAGER.lock().ancestor(frame.args[0], frame.args[1] as u32).unwrap_or(0)
}

pub extern "C" fn nt_user_set_parent(frame: &SyscallFrame) -> u64 {
    WINDOW_MANAGER.lock().reparent(frame.args[0], (frame.args[1] != 0).then_some(frame.args[1]))
        .unwrap_or(0)
}

pub extern "C" fn nt_user_child_window_from_point_ex(frame: &SyscallFrame) -> u64 {
    WINDOW_MANAGER.lock().child_from_point(frame.args[0], frame.args[1] as i32,
        frame.args[2] as i32, frame.args[3] as u32).unwrap_or(0)
}

pub extern "C" fn nt_user_real_child_window_from_point(frame: &SyscallFrame) -> u64 {
    // This engine has no WM_NCHITTEST callback round trip yet. Explicit
    // WS_EX_TRANSPARENT/visibility handling matches the non-callback path.
    WINDOW_MANAGER.lock().child_from_point(frame.args[0], frame.args[1] as i32,
        frame.args[2] as i32, 5).unwrap_or(0)
}
