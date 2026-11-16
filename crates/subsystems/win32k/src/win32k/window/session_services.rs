//! USER operations for the current single desktop and framebuffer session.
//! Additional desktops, DWM redirection and cross-session access are not exposed.
use super::{input_context::caller_identity, WINDOW_MANAGER};
use crate::arch::common::syscall_frame::SyscallFrame;
use crate::ke::sync::Spinlock;
use crate::mm::user_copy::{read_user_object, write_user_object};
use alloc::vec::Vec;

pub const DEFAULT_DESKTOP: u64 = 0x000f_0001;
struct DesktopHandle {
    handle: u64,
    process: u64,
    access: u32,
}
struct DesktopState {
    handles: Vec<DesktopHandle>,
    assignments: Vec<(u64, u64)>,
}
static DESKTOPS: Spinlock<DesktopState> = Spinlock::new(DesktopState {
    handles: Vec::new(),
    assignments: Vec::new(),
});
static NEXT_DESKTOP: core::sync::atomic::AtomicU64 =
    core::sync::atomic::AtomicU64::new(0x7200_0000);

fn thread_exists(thread: u64) -> bool {
    thread != 0 && crate::ps::thread::find_thread_by_tid(thread).is_some()
}
fn desktop_access(handle: u64, access: u32) -> bool {
    handle == DEFAULT_DESKTOP
        || DESKTOPS.lock().handles.iter().any(|entry| {
            entry.handle == handle
                && entry.process == caller_identity().0
                && entry.access & access == access
        })
}
pub extern "C" fn nt_user_get_thread_desktop(frame: &SyscallFrame) -> u64 {
    let thread = frame.args[0] as u32 as u64;
    if !thread_exists(thread) {
        return 0;
    }
    DESKTOPS
        .lock()
        .assignments
        .iter()
        .find(|entry| entry.0 == thread)
        .map_or(DEFAULT_DESKTOP, |entry| entry.1)
}
pub extern "C" fn nt_user_set_thread_desktop(frame: &SyscallFrame) -> u64 {
    let (process, thread) = caller_identity();
    let mut state = DESKTOPS.lock();
    let handle = frame.args[0];
    // All handles refer to the same underlying desktop. Existing windows/hooks
    // therefore stay attached, but the assigned handle must remain open.
    if handle != DEFAULT_DESKTOP
        && !state
            .handles
            .iter()
            .any(|entry| entry.handle == handle && entry.process == process)
    {
        return 0;
    }
    if let Some(assignment) = state.assignments.iter_mut().find(|entry| entry.0 == thread) {
        assignment.1 = handle;
    } else {
        if state.assignments.try_reserve(1).is_err() {
            return 0;
        }
        state.assignments.push((thread, handle));
    }
    1
}

pub fn release_thread(thread: u64) {
    DESKTOPS
        .lock()
        .assignments
        .retain(|entry| entry.0 != thread);
}
pub fn release_process(process: u64) {
    object_services::release_process(process);
    let mut state = DESKTOPS.lock();
    // Remove assignments before their process-owned handles disappear.
    let DesktopState {
        handles,
        assignments,
    } = &mut *state;
    assignments.retain(|entry| {
        !handles
            .iter()
            .any(|handle| handle.handle == entry.1 && handle.process == process)
    });
    handles.retain(|entry| entry.process != process);
}
pub extern "C" fn nt_user_check_desktop_by_thread_id(frame: &SyscallFrame) -> u64 {
    u64::from(thread_exists(frame.args[0] as u32 as u64))
}
pub extern "C" fn nt_user_check_window_thread_desktop(frame: &SyscallFrame) -> u64 {
    u64::from(
        thread_exists(frame.args[1] as u32 as u64)
            && WINDOW_MANAGER.lock().get_window(frame.args[0]).is_some(),
    )
}
pub extern "C" fn nt_user_open_input_desktop(frame: &SyscallFrame) -> u64 {
    // Only the current input desktop is installed. Unknown access rights and
    // DF_ALLOWOTHERACCOUNTHOOK are rejected rather than silently granted.
    let mut access = frame.args[2] as u32;
    // Desktop generic mappings from winnt.h. Synchronize is not a desktop
    // right. Handle inheritance needs process handle-table support.
    for (generic, specific) in [
        (0x8000_0000, 0x0002_0041),
        (0x4000_0000, 0x0002_00be),
        (0x2000_0000, 0x0002_0100),
        (0x1000_0000, 0x000f_01ff),
    ] {
        if access & generic != 0 {
            access = (access & !generic) | specific;
        }
    }
    if frame.args[0] != 0 || frame.args[1] != 0 || access & !0x000f_01ff != 0 {
        return 0;
    }
    let mut state = DESKTOPS.lock();
    let handles = &mut state.handles;
    if handles.try_reserve(1).is_err() {
        return 0;
    }
    let Ok(previous) = NEXT_DESKTOP.try_update(
        core::sync::atomic::Ordering::Relaxed,
        core::sync::atomic::Ordering::Relaxed,
        |handle| handle.checked_add(1),
    ) else {
        return 0;
    };
    handles.push(DesktopHandle {
        handle: previous,
        process: caller_identity().0,
        access,
    });
    previous
}
pub extern "C" fn nt_user_close_desktop(frame: &SyscallFrame) -> u64 {
    let process = caller_identity().0;
    let mut state = DESKTOPS.lock();
    if state
        .assignments
        .iter()
        .any(|entry| entry.1 == frame.args[0])
    {
        return 0;
    }
    let handles = &mut state.handles;
    let Some(index) = handles
        .iter()
        .position(|entry| entry.handle == frame.args[0] && entry.process == process)
    else {
        return 0;
    };
    handles.remove(index);
    1
}
pub extern "C" fn nt_user_switch_desktop(frame: &SyscallFrame) -> u64 {
    // The second native argument is reserved by the public Win7 wrapper.
    u64::from(frame.args[1] == 0 && desktop_access(frame.args[0], 0x100))
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CompositionData {
    attribute: u32,
    padding: u32,
    data: u64,
    size: usize,
}
pub extern "C" fn nt_user_get_window_composition_attribute(frame: &SyscallFrame) -> u64 {
    let Ok(request) = read_user_object::<CompositionData>(frame.args[1]) else {
        return 0;
    };
    if request.size != 4 {
        return 0;
    }
    let manager = WINDOW_MANAGER.lock();
    let Some(window) = manager.get_window(frame.args[0]) else {
        return 0;
    };
    let value = match request.attribute {
        1 => 0, // WCA_NCRENDERING_ENABLED: no DWM redirection surface.
        2 => window.native.nc_rendering_policy,
        _ => return 0,
    };
    u64::from(write_user_object(request.data, &value).is_ok())
}
pub extern "C" fn nt_user_set_window_composition_attribute(frame: &SyscallFrame) -> u64 {
    let Ok(request) = read_user_object::<CompositionData>(frame.args[1]) else {
        return 0;
    };
    if request.attribute != 2 || request.size != 4 {
        return 0;
    }
    let Ok(policy) = read_user_object::<u32>(request.data) else {
        return 0;
    };
    if policy > 2 {
        return 0;
    }
    let mut manager = WINDOW_MANAGER.lock();
    let Some(window) = manager.get_window_mut(frame.args[0]) else {
        return 0;
    };
    if window.process_id != caller_identity().0 {
        return 0;
    }
    // Store the application's requested policy independently of whether the
    // current renderer supports composition. The enabled query remains false.
    window.native.nc_rendering_policy = policy;
    window.dirty = true;
    1
}

pub extern "C" fn nt_user_calculate_popup_window_position(frame: &SyscallFrame) -> u64 {
    let Ok(anchor) = read_user_object::<[i32; 2]>(frame.args[0]) else {
        return 0;
    };
    let Ok(size) = read_user_object::<[i32; 2]>(frame.args[1]) else {
        return 0;
    };
    let flags = frame.args[2] as u32;
    if size[0] < 0
        || size[1] < 0
        || flags & !0x1007c != 0
        || flags & 0xc == 0xc
        || flags & 0x30 == 0x30
    {
        return 0;
    }
    let exclude = if frame.args[3] == 0 {
        None
    } else {
        let Ok(rect) = read_user_object::<super::super::objects::Rect>(frame.args[3]) else {
            return 0;
        };
        if rect.left > rect.right || rect.top > rect.bottom {
            return 0;
        }
        Some(rect)
    };
    let screen = super::super::input_services::screen_rect();
    let (width, height) = (i64::from(size[0]), i64::from(size[1]));
    let (ax, ay) = (i64::from(anchor[0]), i64::from(anchor[1]));
    let mut x = ax
        - if flags & 8 != 0 {
            width
        } else if flags & 4 != 0 {
            width / 2
        } else {
            0
        };
    let mut y = ay
        - if flags & 0x20 != 0 {
            height
        } else if flags & 0x10 != 0 {
            height / 2
        } else {
            0
        };
    let fit = |position: i64, extent: i64, low: i32, high: i32| {
        position.clamp(
            i64::from(low),
            (i64::from(high) - extent).max(i64::from(low)),
        )
    };
    if flags & 0x40 != 0 {
        if x + width > i64::from(screen.right) && ax - width >= i64::from(screen.left) {
            x = ax - width;
        }
    } else if y + height > i64::from(screen.bottom) && ay - height >= i64::from(screen.top) {
        y = ay - height;
    }
    x = fit(x, width, screen.left, screen.right);
    y = fit(y, height, screen.top, screen.bottom);
    if let Some(exclude) = exclude {
        let overlaps = |cx: i64, cy: i64| {
            cx < i64::from(exclude.right)
                && cx + width > i64::from(exclude.left)
                && cy < i64::from(exclude.bottom)
                && cy + height > i64::from(exclude.top)
        };
        if overlaps(x, y) {
            let candidates = if flags & 0x40 != 0 {
                [
                    (i64::from(exclude.right), y),
                    (i64::from(exclude.left) - width, y),
                    (x, i64::from(exclude.bottom)),
                    (x, i64::from(exclude.top) - height),
                ]
            } else {
                [
                    (x, i64::from(exclude.bottom)),
                    (x, i64::from(exclude.top) - height),
                    (i64::from(exclude.right), y),
                    (i64::from(exclude.left) - width, y),
                ]
            };
            if let Some((cx, cy)) = candidates.into_iter().find(|&(cx, cy)| {
                !overlaps(cx, cy)
                    && cx >= i64::from(screen.left)
                    && cy >= i64::from(screen.top)
                    && cx + width <= i64::from(screen.right)
                    && cy + height <= i64::from(screen.bottom)
            }) {
                x = cx;
                y = cy;
            }
        }
    }
    let (Ok(right), Ok(bottom)) = (i32::try_from(x + width), i32::try_from(y + height)) else {
        return 0;
    };
    let rect = super::super::objects::Rect::new(x as i32, y as i32, right, bottom);
    u64::from(write_user_object(frame.args[4], &rect).is_ok())
}

#[path = "object_services.rs"]
mod object_services;
pub use object_services::*;
