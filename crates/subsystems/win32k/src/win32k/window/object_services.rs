//! Objects in the installed WinSta0/Default session. Open calls capture NT64
//! object attributes; handles carry caller ownership and mapped access rights.
use super::*;
use crate::mm::user_copy::{copy_from_user, copy_to_user};

fn failure(error: u32) -> u64 {
    super::super::super::input_services::set_last_error(error);
    0
}

pub const DEFAULT_WINDOW_STATION: u64 = 0x000e_0001;
struct StationHandle {
    handle: u64,
    process: u64,
    _access: u32,
}
struct StationState {
    handles: Vec<StationHandle>,
    assignments: Vec<(u64, u64)>,
}
static STATIONS: Spinlock<StationState> = Spinlock::new(StationState {
    handles: Vec::new(),
    assignments: Vec::new(),
});

#[repr(C)]
#[derive(Clone, Copy)]
struct Attributes {
    length: u32,
    padding: u32,
    root: u64,
    name: u64,
    flags: u32,
    padding2: u32,
    security: u64,
    qos: u64,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct Name {
    length: u16,
    maximum: u16,
    padding: u32,
    buffer: u64,
}

fn station_owned(state: &StationState, handle: u64, process: u64) -> bool {
    handle == DEFAULT_WINDOW_STATION
        || state
            .handles
            .iter()
            .any(|h| h.handle == handle && h.process == process)
}
fn matches_name(address: u64, expected: &str, root_station: bool) -> bool {
    let Ok(attributes) = read_user_object::<Attributes>(address) else {
        return false;
    };
    // Inheritable handles and cross-session namespaces need process/session
    // support. Reject those requests instead of discarding their attributes.
    if attributes.length != 48
        || attributes.flags & !0x40 != 0
        || attributes.security != 0
        || attributes.qos != 0
    {
        return false;
    }
    if attributes.root != 0
        && (!root_station || !station_owned(&STATIONS.lock(), attributes.root, caller_identity().0))
    {
        return false;
    }
    let Ok(name) = read_user_object::<Name>(attributes.name) else {
        return false;
    };
    if name.length == 0 || name.length > 512 || name.length % 2 != 0 || name.maximum < name.length {
        return false;
    }
    let mut bytes = [0u8; 512];
    if copy_from_user(&mut bytes[..name.length as usize], name.buffer).is_err() {
        return false;
    }
    let expected = expected.as_bytes();
    name.length as usize == expected.len() * 2
        && expected.iter().enumerate().all(|(i, ascii)| {
            let unit = u16::from_le_bytes([bytes[i * 2], bytes[i * 2 + 1]]);
            if attributes.flags & 0x40 != 0 {
                unit < 128 && (unit as u8).eq_ignore_ascii_case(ascii)
            } else {
                unit == u16::from(*ascii)
            }
        })
}

pub extern "C" fn nt_user_open_desktop(frame: &SyscallFrame) -> u64 {
    if !matches_name(frame.args[0], "Default", true) {
        return failure(87);
    }
    let mut request = SyscallFrame::default();
    request.args[0] = frame.args[1];
    request.args[2] = frame.args[2];
    nt_user_open_input_desktop(&request)
}
pub extern "C" fn nt_user_open_window_station(frame: &SyscallFrame) -> u64 {
    if !matches_name(frame.args[0], "WinSta0", false) {
        return failure(87);
    }
    let mut access = frame.args[1] as u32;
    for (generic, specific) in [
        (0x80000000, 0x20303),
        (0x40000000, 0x2001c),
        (0x20000000, 0x20060),
        (0x10000000, 0xf037f),
    ] {
        if access & generic != 0 {
            access = (access & !generic) | specific;
        }
    }
    if access & !0xf037f != 0 {
        return failure(5);
    }
    let mut state = STATIONS.lock();
    if state.handles.try_reserve(1).is_err() {
        return 0;
    }
    let Ok(handle) = NEXT_DESKTOP.try_update(
        core::sync::atomic::Ordering::Relaxed,
        core::sync::atomic::Ordering::Relaxed,
        |handle| handle.checked_add(1),
    ) else {
        return 0;
    };
    state.handles.push(StationHandle {
        handle,
        process: caller_identity().0,
        _access: access,
    });
    handle
}
pub extern "C" fn nt_user_get_process_window_station(_: &SyscallFrame) -> u64 {
    STATIONS
        .lock()
        .assignments
        .iter()
        .find(|a| a.0 == caller_identity().0)
        .map_or(DEFAULT_WINDOW_STATION, |a| a.1)
}
pub extern "C" fn nt_user_set_process_window_station(frame: &SyscallFrame) -> u64 {
    let process = caller_identity().0;
    let mut state = STATIONS.lock();
    if !station_owned(&state, frame.args[0], process) {
        return failure(6);
    }
    if let Some(assignment) = state.assignments.iter_mut().find(|a| a.0 == process) {
        assignment.1 = frame.args[0];
    } else {
        if state.assignments.try_reserve(1).is_err() {
            return 0;
        }
        state.assignments.push((process, frame.args[0]));
    }
    1
}
pub extern "C" fn nt_user_close_window_station(frame: &SyscallFrame) -> u64 {
    let mut state = STATIONS.lock();
    if state.assignments.iter().any(|a| a.1 == frame.args[0]) {
        return failure(170); // ERROR_BUSY
    }
    let Some(index) = state
        .handles
        .iter()
        .position(|h| h.handle == frame.args[0] && h.process == caller_identity().0)
    else {
        return failure(6);
    };
    state.handles.remove(index);
    1
}
pub(super) fn release_process(process: u64) {
    let mut state = STATIONS.lock();
    state.assignments.retain(|a| a.0 != process);
    state.handles.retain(|h| h.process != process);
}

pub extern "C" fn nt_user_get_object_information(frame: &SyscallFrame) -> u64 {
    let process = caller_identity().0;
    let station = station_owned(&STATIONS.lock(), frame.args[0], process);
    let desktop = frame.args[0] == DEFAULT_DESKTOP
        || DESKTOPS
            .lock()
            .handles
            .iter()
            .any(|h| h.handle == frame.args[0] && h.process == process);
    if !station && !desktop {
        return failure(6);
    }
    let mut bytes = [0u8; 32];
    let size = match frame.args[1] as u32 {
        1 => {
            // USEROBJECTFLAGS: fInherit, fReserved, dwFlags.
            bytes[8..12].copy_from_slice(&u32::from(station).to_le_bytes());
            12
        }
        2 | 3 => {
            let text = match (frame.args[1], station) {
                (2, true) => "WinSta0",
                (2, false) => "Default",
                (_, true) => "WindowStation",
                (_, false) => "Desktop",
            };
            for (pair, unit) in bytes.chunks_exact_mut(2).zip(text.encode_utf16()) {
                pair.copy_from_slice(&unit.to_le_bytes());
            }
            (text.len() + 1) * 2
        }
        6 if desktop => {
            bytes[..4].copy_from_slice(&1u32.to_le_bytes()); // UOI_IO
            4
        }
        _ => return failure(87),
    };
    if frame.args[4] != 0 && write_user_object(frame.args[4], &(size as u32)).is_err() {
        return failure(998);
    }
    if frame.args[3] < size as u64 {
        return failure(122);
    }
    match copy_to_user(frame.args[2], &bytes[..size]) {
        Ok(()) => 1,
        Err(_) => failure(998),
    }
}
