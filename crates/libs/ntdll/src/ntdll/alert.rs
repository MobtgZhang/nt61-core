//! User-mode thread alerts. An alert is a coalescing interrupt of an alertable
//! wait, separate from a user APC and requiring no user callback execution.
use super::alert_state::UserAlertState;
use super::file;
use super::status::*;
use super::types::{HANDLE, NTSTATUS};

/// The operation must finish without scheduling or retaining the reference.
pub(crate) fn with_current_alert<T>(operation: impl FnOnce(Option<&UserAlertState>) -> T) -> T {
    let thread = crate::ps::thread::get_current_ethread();
    if thread.is_null() {
        return operation(None);
    }
    // SAFETY: the current ETHREAD remains allocated for this nonblocking call;
    // only its atomic alert extension is shared with other CPUs.
    operation(Some(unsafe { &(*thread).user_alert }))
}

pub(crate) fn take_current_alert() -> bool {
    with_current_alert(|state| state.is_some_and(file::take_user_alert))
}

/// # Safety
/// The native ABI supplies a scalar handle; no user pointers are dereferenced.
pub unsafe extern "C" fn nt_alert_thread(handle: HANDLE) -> NTSTATUS {
    if handle as isize == -2 {
        let (_, tid) = super::sync::current_client_id();
        return with_current_alert(|state| match state {
            Some(state) => file::alert_user_thread(tid, state),
            None => STATUS_INVALID_HANDLE,
        });
    }
    let tid = match file::thread_alert_target(handle) {
        Ok(tid) => tid,
        Err(status) => return status,
    };
    // Retain the registry lock through the alert transaction. Process teardown
    // removes its registry entry before freeing the main ETHREAD allocation.
    crate::ps::thread::with_thread_user_alert(tid, |state| file::alert_user_thread(tid, state))
        .unwrap_or(STATUS_THREAD_IS_TERMINATING)
}

/// Test and clear this thread's pending user alert. APC delivery needs a
/// user-return trampoline; no user callback is ever executed in kernel mode.
///
/// # Safety
/// This syscall has no user pointer arguments.
pub unsafe extern "C" fn nt_test_alert() -> NTSTATUS {
    if take_current_alert() {
        STATUS_ALERTED
    } else {
        STATUS_SUCCESS
    }
}
