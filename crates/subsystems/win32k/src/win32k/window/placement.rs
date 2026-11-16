//! Window placement uses the Win32 ABI (44 bytes), including saved restore
//! bounds. All caller memory is captured before acquiring the window lock.
use super::{Rect, WindowObject, WindowStyle, WINDOW_MANAGER};
use crate::arch::common::syscall_frame::SyscallFrame;
use crate::mm::user_copy::{probe_user_write, read_user_object, write_user_object};

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Placement {
    pub length: u32,
    pub flags: u32,
    pub show: u32,
    pub min_x: i32,
    pub min_y: i32,
    pub max_x: i32,
    pub max_y: i32,
    pub normal: Rect,
}

impl WindowObject {
    pub fn show_command(&self) -> u32 {
        if !self.visible {
            0
        } else if self.style & WindowStyle::MINIMIZE != 0 {
            2
        } else if self.style & WindowStyle::MAXIMIZE != 0 {
            3
        } else {
            1
        }
    }

    fn apply_placement(&mut self, placement: Placement) -> bool {
        if placement.flags & !7 != 0
            || placement.show > 11
            || placement.normal.left > placement.normal.right
            || placement.normal.top > placement.normal.bottom
        {
            return false;
        }
        self.normal_rect = placement.normal;
        self.minimize_point = (placement.min_x, placement.min_y);
        self.maximize_point = (placement.max_x, placement.max_y);
        self.placement_flags = placement.flags;
        let previous = self.show_command();
        self.style &= !(WindowStyle::MINIMIZE | WindowStyle::MAXIMIZE | WindowStyle::VISIBLE);
        self.visible = placement.show != 0;
        if self.visible {
            self.style |= WindowStyle::VISIBLE;
        }
        match placement.show {
            2 | 6 | 7 | 11 => self.style |= WindowStyle::MINIMIZE,
            3 => self.style |= WindowStyle::MAXIMIZE,
            5 | 8 if previous == 2 => self.style |= WindowStyle::MINIMIZE,
            5 | 8 if previous == 3 => self.style |= WindowStyle::MAXIMIZE,
            _ => {}
        }
        if self.style & (WindowStyle::MINIMIZE | WindowStyle::MAXIMIZE) == 0 {
            self.rect = self.normal_rect;
            self.update_client_rect();
        }
        self.dirty = true;
        true
    }

    fn placement(&self) -> Placement {
        Placement {
            length: core::mem::size_of::<Placement>() as u32,
            flags: self.placement_flags,
            show: self.show_command(),
            min_x: self.minimize_point.0,
            min_y: self.minimize_point.1,
            max_x: self.maximize_point.0,
            max_y: self.maximize_point.1,
            normal: if self.style & (WindowStyle::MINIMIZE | WindowStyle::MAXIMIZE) == 0 {
                self.rect
            } else {
                self.normal_rect
            },
        }
    }
}

pub extern "C" fn nt_user_get_window_placement(frame: &SyscallFrame) -> u64 {
    let Ok(length) = read_user_object::<u32>(frame.args[1]) else {
        return 0;
    };
    if length != 44 {
        return 0;
    }
    let placement = {
        let manager = WINDOW_MANAGER.lock();
        let Some(window) = manager.get_window(frame.args[0]) else {
            return 0;
        };
        window.placement()
    };
    u64::from(write_user_object(frame.args[1], &placement).is_ok())
}

pub extern "C" fn nt_user_set_window_placement(frame: &SyscallFrame) -> u64 {
    let Ok(placement) = read_user_object::<Placement>(frame.args[1]) else {
        return 0;
    };
    if placement.length != 44 {
        return 0;
    }
    let mut manager = WINDOW_MANAGER.lock();
    manager
        .get_window_mut(frame.args[0])
        .map_or(0, |window| u64::from(window.apply_placement(placement)))
}

pub extern "C" fn nt_user_get_internal_window_pos(frame: &SyscallFrame) -> u64 {
    if frame.args[1] != 0 && probe_user_write(frame.args[1], 16).is_err()
        || frame.args[2] != 0 && probe_user_write(frame.args[2], 8).is_err()
    {
        return 0;
    }
    let placement = {
        let manager = WINDOW_MANAGER.lock();
        let Some(window) = manager.get_window(frame.args[0]) else {
            return 0;
        };
        window.placement()
    };
    if frame.args[1] != 0 && write_user_object(frame.args[1], &placement.normal).is_err() {
        return 0;
    }
    let point = [placement.min_x, placement.min_y];
    if frame.args[2] != 0 && write_user_object(frame.args[2], &point).is_err() {
        return 0;
    }
    placement.show.into()
}

pub extern "C" fn nt_user_set_internal_window_pos(frame: &SyscallFrame) -> u64 {
    let rect = if frame.args[2] == 0 {
        None
    } else {
        let Ok(rect) = read_user_object::<Rect>(frame.args[2]) else {
            return 0;
        };
        Some(rect)
    };
    let point = if frame.args[3] == 0 {
        None
    } else {
        let Ok(point) = read_user_object::<[i32; 2]>(frame.args[3]) else {
            return 0;
        };
        Some(point)
    };
    let mut manager = WINDOW_MANAGER.lock();
    let Some(window) = manager.get_window_mut(frame.args[0]) else {
        return 0;
    };
    let mut placement = window.placement();
    placement.show = frame.args[1] as u32;
    if let Some(rect) = rect {
        placement.normal = rect;
    }
    if let Some([x, y]) = point {
        placement.min_x = x;
        placement.min_y = y;
    }
    // Native routine returns void. Invalid input leaves the object unchanged.
    window.apply_placement(placement);
    0
}
