//! Native event pairs contain independent low/high synchronization events.
//! Signals coalesce and release one waiter. The dispatcher owns both halves,
//! including named opens, handle aliases, and references held by queued waits.

use super::file::{DispatcherState, HandleKind};
use super::types::{ObjectAttributes, HANDLE, NTSTATUS};

const LOW: u64 = 1;
const HIGH: u64 = 2;

/// # Safety
/// User output and object attributes are captured by the common object layer.
pub unsafe extern "C" fn nt_create_event_pair(
    output: *mut HANDLE,
    access: u32,
    attributes: *mut ObjectAttributes,
) -> NTSTATUS {
    super::object::create_native_sync(
        output, access, attributes, HandleKind::EventPair, 0, DispatcherState::None,
    )
}

/// # Safety
/// User output and object attributes are captured by the common object layer.
pub unsafe extern "C" fn nt_open_event_pair(
    output: *mut HANDLE,
    access: u32,
    attributes: *mut ObjectAttributes,
) -> NTSTATUS {
    super::object::open_native_sync(output, access, attributes, HandleKind::EventPair)
}

fn operate(handle: HANDLE, signal: u64, wait: u64) -> NTSTATUS {
    let (pid, tid) = super::sync::current_client_id();
    super::sync::complete_wait(super::file::operate_event_pair(handle, signal, wait, pid, tid))
}

pub extern "C" fn nt_set_low_event_pair(handle: HANDLE) -> NTSTATUS {
    operate(handle, LOW, 0)
}

pub extern "C" fn nt_set_high_event_pair(handle: HANDLE) -> NTSTATUS {
    operate(handle, HIGH, 0)
}

pub extern "C" fn nt_wait_low_event_pair(handle: HANDLE) -> NTSTATUS {
    operate(handle, 0, LOW)
}

pub extern "C" fn nt_wait_high_event_pair(handle: HANDLE) -> NTSTATUS {
    operate(handle, 0, HIGH)
}

pub extern "C" fn nt_set_low_wait_high_event_pair(handle: HANDLE) -> NTSTATUS {
    operate(handle, LOW, HIGH)
}

pub extern "C" fn nt_set_high_wait_low_event_pair(handle: HANDLE) -> NTSTATUS {
    operate(handle, HIGH, LOW)
}
