//! Per-window native USER state. Captures precede mutations; window lifetime
//! and ownership checks remain under the window manager lock.
use super::{input_context::caller_identity, WindowStyle, WindowStyleEx, WINDOW_MANAGER};
use crate::arch::common::syscall_frame::SyscallFrame;
use crate::mm::user_copy::{probe_user_write, read_user_object, write_user_object};
use alloc::vec::Vec;

#[derive(Clone, Debug, Default)]
pub struct NativeWindowState {
    pub nc_rendering_policy: u32,
    pub layered: Option<(u32, u8, u32)>,
    pub ime_open: bool,
    pub ime_conversion: u32,
    pub region: Option<super::super::region_geometry::Region>,
    pub touch_flags: Option<u32>,
    pub gestures: Vec<GestureConfig>,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct GestureConfig {
    pub id: u32,
    pub want: u32,
    pub block: u32,
}

pub extern "C" fn nt_user_set_layered_window_attributes(frame: &SyscallFrame) -> u64 {
    let flags = frame.args[3] as u32;
    if flags == 0 || flags & !3 != 0 {
        return 0;
    }
    let (process, _) = caller_identity();
    let mut manager = WINDOW_MANAGER.lock();
    let Some(window) = manager.get_window_mut(frame.args[0]) else {
        return 0;
    };
    // Layered child windows were introduced after NT 6.1.
    if window.process_id != process
        || window.ex_style & 0x80000 == 0
        || window.style & WindowStyle::CHILD != 0
    {
        return 0;
    }
    window.native.layered = Some((frame.args[1] as u32, frame.args[2] as u8, flags));
    window.dirty = true;
    1
}

pub extern "C" fn nt_user_get_layered_window_attributes(frame: &SyscallFrame) -> u64 {
    for (pointer, size) in [(frame.args[1], 4), (frame.args[2], 1), (frame.args[3], 4)] {
        if pointer != 0 && probe_user_write(pointer, size).is_err() {
            return 0;
        }
    }
    let data = {
        let manager = WINDOW_MANAGER.lock();
        let Some(window) = manager.get_window(frame.args[0]) else {
            return 0;
        };
        if window.ex_style & 0x80000 == 0 {
            return 0;
        }
        let Some(data) = window.native.layered else {
            return 0;
        };
        data
    };
    if frame.args[1] != 0 && write_user_object(frame.args[1], &data.0).is_err()
        || frame.args[2] != 0 && write_user_object(frame.args[2], &data.1).is_err()
        || frame.args[3] != 0 && write_user_object(frame.args[3], &data.2).is_err()
    {
        return 0;
    }
    1
}

pub extern "C" fn nt_user_get_window_display_affinity(frame: &SyscallFrame) -> u64 {
    if !WINDOW_MANAGER
        .lock()
        .get_window(frame.args[0])
        .is_some_and(|window| window.style & WindowStyle::CHILD == 0)
    {
        return 0;
    }
    u64::from(write_user_object(frame.args[1], &0u32).is_ok())
}

pub extern "C" fn nt_user_set_window_display_affinity(frame: &SyscallFrame) -> u64 {
    let (process, _) = caller_identity();
    let manager = WINDOW_MANAGER.lock();
    let Some(window) = manager.get_window(frame.args[0]) else {
        return 0;
    };
    // WDA_MONITOR requires DWM redirection and capture exclusion. The current
    // framebuffer backend cannot enforce it, so never report it as enabled.
    u64::from(
        window.process_id == process
            && window.style & WindowStyle::CHILD == 0
            && frame.args[1] == 0,
    )
}

pub extern "C" fn nt_user_get_top_level_window(frame: &SyscallFrame) -> u64 {
    WINDOW_MANAGER
        .lock()
        .ancestor(frame.args[0], 2)
        .unwrap_or(0)
}

pub extern "C" fn nt_user_is_top_level_window(frame: &SyscallFrame) -> u64 {
    WINDOW_MANAGER
        .lock()
        .get_window(frame.args[0])
        .map_or(0, |window| {
            u64::from(window.style & WindowStyle::CHILD == 0)
        })
}

pub extern "C" fn nt_user_get_window_minimize_rect(frame: &SyscallFrame) -> u64 {
    let rect = {
        let manager = WINDOW_MANAGER.lock();
        let Some(window) = manager.get_window(frame.args[0]) else {
            return 0;
        };
        // This backend keeps iconified bounds on the window; no icon-title
        // surrogate window is allocated.
        window.rect
    };
    u64::from(write_user_object(frame.args[1], &rect).is_ok())
}

pub extern "C" fn nt_user_internal_get_window_icon(frame: &SyscallFrame) -> u64 {
    let manager = WINDOW_MANAGER.lock();
    let Some(window) = manager.get_window(frame.args[0]) else {
        return 0;
    };
    let Some(class) = manager
        .classes
        .iter()
        .flatten()
        .find(|c| c.atom == window.class_atom)
    else {
        return 0;
    };
    match frame.args[1] as u32 {
        0 | 2 => {
            if class.small_icon != 0 {
                class.small_icon
            } else {
                class.h_icon
            }
        }
        1 => class.h_icon,
        _ => 0,
    }
}

pub extern "C" fn nt_user_set_child_window_no_activate(frame: &SyscallFrame) -> u64 {
    let (process, _) = caller_identity();
    let mut manager = WINDOW_MANAGER.lock();
    let Some(window) = manager.get_window_mut(frame.args[0]) else {
        return 0xc0000008;
    };
    if window.process_id != process {
        return 0xc0000022;
    }
    if window.style & WindowStyle::CHILD == 0 {
        return 0xc000000d;
    }
    window.ex_style |= WindowStyleEx::NO_ACTIVATE;
    0
}

pub extern "C" fn nt_user_set_process_dpi_aware(_: &SyscallFrame) -> u64 {
    // The single display runs at 96 DPI and has no virtualization. Record
    // awareness per process so later display backends can preserve the choice.
    let (process, _) = caller_identity();
    let mut manager = WINDOW_MANAGER.lock();
    if !manager.dpi_aware_processes.contains(&process) {
        if manager.dpi_aware_processes.try_reserve(1).is_err() {
            return 0;
        }
        manager.dpi_aware_processes.push(process);
    }
    1
}

pub extern "C" fn nt_user_set_shell_window_ex(frame: &SyscallFrame) -> u64 {
    let (process, _) = caller_identity();
    let mut manager = WINDOW_MANAGER.lock();
    if frame.args[0] == 0 || manager.shell_window != 0 {
        return 0;
    }
    let Some(window) = manager.get_window(frame.args[0]) else {
        return 0;
    };
    if window.process_id != process || window.style & WindowStyle::CHILD != 0 {
        return 0;
    }
    if frame.args[1] != 0
        && !manager
            .get_window(frame.args[1])
            .is_some_and(|w| w.process_id == process)
    {
        return 0;
    }
    manager.shell_window = frame.args[0];
    manager.shell_list_view = frame.args[1];
    1
}

pub extern "C" fn nt_user_notify_ime_status(frame: &SyscallFrame) -> u64 {
    let (process, _) = caller_identity();
    let mut manager = WINDOW_MANAGER.lock();
    let Some(window) = manager.get_window_mut(frame.args[0]) else {
        return 0;
    };
    if window.process_id != process {
        return 0;
    }
    window.native.ime_open = frame.args[1] != 0;
    window.native.ime_conversion = frame.args[2] as u32;
    0
}

pub extern "C" fn nt_user_get_window_rgn_ex(frame: &SyscallFrame) -> u64 {
    if frame.args[2] != 0 {
        return 0;
    }
    let geometry = {
        let manager = WINDOW_MANAGER.lock();
        let Some(region) = manager
            .get_window(frame.args[0])
            .and_then(|w| w.native.region.as_ref())
        else {
            return 0;
        };
        let Ok(region) = region.try_clone() else {
            return 0;
        };
        region
    };
    let complexity = geometry.complexity() as u64;
    if super::super::region::replace_geometry(frame.args[1], geometry) {
        complexity
    } else {
        0
    }
}

pub extern "C" fn nt_user_set_window_rgn_ex(frame: &SyscallFrame) -> u64 {
    set_window_region(frame)
}

pub(super) fn set_window_region(frame: &SyscallFrame) -> u64 {
    let geometry = if frame.args[1] == 0 {
        None
    } else {
        let Some(region) = super::super::region::snapshot(frame.args[1]) else {
            return 0;
        };
        Some(region)
    };
    let (process, _) = caller_identity();
    let mut manager = WINDOW_MANAGER.lock();
    let Some(window) = manager.get_window_mut(frame.args[0]) else {
        return 0;
    };
    if window.process_id != process {
        return 0;
    }
    // Do not change USER state when a selected, foreign or concurrently closed
    // region cannot transfer ownership. GDI never recycles these handle slots.
    if frame.args[1] != 0 && !super::super::region::gre_delete_region(frame.args[1]) {
        return 0;
    }
    window.native.region = geometry;
    window.dirty = frame.args[2] != 0 || window.dirty;
    1
}

pub extern "C" fn nt_user_is_touch_window(frame: &SyscallFrame) -> u64 {
    let flags = {
        let manager = WINDOW_MANAGER.lock();
        let Some(flags) = manager
            .get_window(frame.args[0])
            .and_then(|w| w.native.touch_flags)
        else {
            return 0;
        };
        flags
    };
    if frame.args[1] != 0 && write_user_object(frame.args[1], &flags).is_err() {
        return 0;
    }
    1
}

pub extern "C" fn nt_user_modify_window_touch_capability(frame: &SyscallFrame) -> u64 {
    let (process, _) = caller_identity();
    if frame.args[1] as u32 & !3 != 0 {
        return 0;
    }
    let mut manager = WINDOW_MANAGER.lock();
    let Some(window) = manager.get_window_mut(frame.args[0]) else {
        return 0;
    };
    if window.process_id != process {
        return 0;
    }
    window.native.touch_flags = match frame.args[2] as u32 {
        0 => None,
        1 => Some(frame.args[1] as u32),
        _ => return 0,
    };
    1
}

pub extern "C" fn nt_user_set_gesture_config(frame: &SyscallFrame) -> u64 {
    let count = frame.args[2] as u32 as usize;
    if frame.args[1] != 0 || frame.args[4] != 12 || count == 0 || count > 5 {
        return 0;
    }
    let mut captured = Vec::new();
    if captured.try_reserve_exact(count).is_err() {
        return 0;
    }
    for index in 0..count {
        let Ok(config) =
            read_user_object::<GestureConfig>(frame.args[3].saturating_add(index as u64 * 12))
        else {
            return 0;
        };
        let allowed = match config.id {
            0 => 1,
            3 | 5 | 6 | 7 => 1,
            4 => 0x1f,
            _ => return 0,
        };
        if (config.want | config.block) & !allowed != 0
            || config.want & config.block != 0
            || (config.id == 0 && count != 1)
            || captured.iter().any(|c: &GestureConfig| c.id == config.id)
        {
            return 0;
        }
        captured.push(config);
    }
    let (process, _) = caller_identity();
    let mut manager = WINDOW_MANAGER.lock();
    let Some(window) = manager.get_window_mut(frame.args[0]) else {
        return 0;
    };
    if window.process_id != process {
        return 0;
    }
    let mut next = Vec::new();
    if next.try_reserve_exact(6).is_err() {
        return 0;
    }
    next.extend_from_slice(&window.native.gestures);
    for config in captured {
        if let Some(old) = next.iter_mut().find(|old| old.id == config.id) {
            *old = config;
        } else {
            next.push(config);
        }
    }
    window.native.gestures = next;
    1
}

pub extern "C" fn nt_user_get_gesture_config(frame: &SyscallFrame) -> u64 {
    if frame.args[1] != 0 || frame.args[2] as u32 & !1 != 0 || frame.args[5] != 12 {
        return 0;
    }
    let Ok(count) = read_user_object::<u32>(frame.args[3]) else {
        return 0;
    };
    if count == 0 || count > 6 {
        return 0;
    }
    let mut requested = Vec::new();
    if requested.try_reserve_exact(count as usize).is_err() {
        return 0;
    }
    for index in 0..count {
        let Ok(config) =
            read_user_object::<GestureConfig>(frame.args[4].saturating_add(u64::from(index) * 12))
        else {
            return 0;
        };
        if !matches!(config.id, 0 | 3..=7) {
            return 0;
        }
        requested.push(config);
    }
    let manager = WINDOW_MANAGER.lock();
    let Some(window) = manager.get_window(frame.args[0]) else {
        return 0;
    };
    for config in &mut requested {
        *config = window
            .native
            .gestures
            .iter()
            .find(|entry| entry.id == config.id)
            .copied()
            .unwrap_or(GestureConfig {
                id: config.id,
                want: 1,
                block: 0,
            });
    }
    drop(manager);
    for (index, config) in requested.iter().enumerate() {
        if write_user_object(frame.args[4] + index as u64 * 12, config).is_err() {
            return 0;
        }
    }
    1
}
