//! Native APC entry points. User callback delivery requires a return-to-user
//! trampoline and context restoration. Unsupported delivery never invokes a
//! user routine at kernel privilege or reports a discarded APC as queued.
pub use super::alert::{nt_alert_thread, nt_test_alert};
use super::file::{lookup_handle, HandleKind};
use super::status::*;
use super::types::{HANDLE, NTSTATUS, PVOID};
use crate::mm::user_copy::probe_user_write;

fn validate_apc_thread(handle: HANDLE) -> Result<(), NTSTATUS> {
    if handle as isize == -2 {
        return if crate::ps::thread::get_current_ethread().is_null() {
            Err(STATUS_INVALID_HANDLE)
        } else {
            Ok(())
        };
    }
    match lookup_handle(handle) {
        Some(entry) if entry.kind == HandleKind::Thread => Ok(()),
        Some(_) => Err(STATUS_OBJECT_TYPE_MISMATCH),
        None => Err(STATUS_INVALID_HANDLE),
    }
}

/// # Safety
/// All pointers are scalar user addresses; none are executed or dereferenced.
pub unsafe extern "C" fn nt_queue_apc_thread(
    thread: HANDLE,
    routine: PVOID,
    _argument1: PVOID,
    _argument2: PVOID,
    _argument3: PVOID,
) -> NTSTATUS {
    if let Err(status) = validate_apc_thread(thread) {
        return status;
    }
    let _ = routine;
    STATUS_NOT_IMPLEMENTED
}

/// Alert-and-resume requires the scheduler's suspend counter and blocking state.
/// No alert, output or scheduling side effect is committed while unavailable.
///
/// # Safety
/// The optional output pointer belongs to the caller and is probed only.
pub unsafe extern "C" fn nt_alert_resume_thread(
    thread: HANDLE,
    previous_suspend_count: *mut u32,
) -> NTSTATUS {
    if !previous_suspend_count.is_null() {
        if let Err(status) = probe_user_write(previous_suspend_count as u64, 4) {
            return status;
        }
    }
    if let Err(status) = validate_apc_thread(thread) {
        return status;
    }
    STATUS_NOT_IMPLEMENTED
}
