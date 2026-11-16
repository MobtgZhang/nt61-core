//! Process-local keyed rendezvous used by user-mode locks. A release without
//! a matching waiter blocks instead of accumulating an event signal.

use super::file::{DispatcherState, HandleKind};
use super::types::{ObjectAttributes, HANDLE, NTSTATUS};
use crate::mm::user_copy::read_user_object;

/// # Safety
/// Object attributes and the output handle are captured by the object layer.
pub unsafe extern "C" fn nt_create_keyed_event(
    output: *mut HANDLE,
    access: u32,
    attributes: *mut ObjectAttributes,
    flags: u32,
) -> NTSTATUS {
    if flags != 0 {
        return super::status::STATUS_INVALID_PARAMETER;
    }
    super::object::create_native_sync(
        output, access, attributes, HandleKind::KeyedEvent, 0, DispatcherState::None,
    )
}

/// # Safety
/// Object attributes and the output handle are captured by the object layer.
pub unsafe extern "C" fn nt_open_keyed_event(
    output: *mut HANDLE,
    access: u32,
    attributes: *mut ObjectAttributes,
) -> NTSTATUS {
    super::object::open_native_sync(output, access, attributes, HandleKind::KeyedEvent)
}

fn rendezvous(handle: HANDLE, key: usize, alertable: u8, timeout: *const i64, release: bool) -> NTSTATUS {
    if key & 1 != 0 {
        return 0xc000_00ef_u32 as i32; // STATUS_INVALID_PARAMETER_1
    }
    let timeout = if timeout.is_null() {
        None
    } else {
        match read_user_object::<i64>(timeout as u64) {
            Ok(value) => Some(value),
            Err(status) => return status,
        }
    };
    let (pid, tid) = super::sync::current_client_id();
    let start = super::alert::with_current_alert(|state| {
        super::file::begin_keyed_wait(
            handle, key as u64, release, pid, tid, timeout,
            if alertable != 0 { state } else { None },
        )
    });
    super::sync::complete_wait(start)
}

/// # Safety
/// The optional timeout is copied through the user memory access layer.
pub unsafe extern "C" fn nt_wait_for_keyed_event(
    handle: HANDLE,
    key: usize,
    alertable: u8,
    timeout: *const i64,
) -> NTSTATUS {
    rendezvous(handle, key, alertable, timeout, false)
}

/// # Safety
/// The optional timeout is copied through the user memory access layer.
pub unsafe extern "C" fn nt_release_keyed_event(
    handle: HANDLE,
    key: usize,
    alertable: u8,
    timeout: *const i64,
) -> NTSTATUS {
    rendezvous(handle, key, alertable, timeout, true)
}
