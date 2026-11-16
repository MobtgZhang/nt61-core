//! Physical/logical coordinates for the current 96-DPI display backend.
//! These operations capture and copy points even when the transform is identity.
use super::WINDOW_MANAGER;
use crate::arch::common::syscall_frame::SyscallFrame;
use crate::mm::user_copy::{read_user_object, write_user_object};

fn convert_point(frame: &SyscallFrame) -> u64 {
    let Ok(point) = read_user_object::<[i32; 2]>(frame.args[1]) else {
        return 0;
    };
    let manager = WINDOW_MANAGER.lock();
    let Some(window) = manager.get_window(frame.args[0]) else { return 0 };
    let Some((origin_x, origin_y)) = manager.client_screen_origin(window.parent) else { return 0 };
    // DPI conversion accepts points on the window boundary. Unlike hit
    // testing, the right and bottom edges are included.
    let x = i64::from(point[0]);
    let y = i64::from(point[1]);
    if x < origin_x + i64::from(window.rect.left)
        || x > origin_x + i64::from(window.rect.right)
        || y < origin_y + i64::from(window.rect.top)
        || y > origin_y + i64::from(window.rect.bottom) {
        return 0;
    }
    drop(manager);
    u64::from(write_user_object(frame.args[1], &point).is_ok())
}

pub extern "C" fn nt_user_logical_to_physical_point(frame: &SyscallFrame) -> u64 {
    convert_point(frame)
}
pub extern "C" fn nt_user_physical_to_logical_point(frame: &SyscallFrame) -> u64 {
    convert_point(frame)
}

