//! Native timer objects backed by the shared handle/object store.
//! Clock polling publishes expiration independently of queries. APC callbacks
//! and wake-from-suspend remain unavailable and are rejected before mutation.
use super::file::{
    alloc_dispatcher_handle, with_sync_access, DispatcherState, HandleKind, TimerState,
};
use super::status::*;
use super::types::{LargeInteger, HANDLE, NTSTATUS, PVOID};
use crate::mm::user_copy::{probe_user_write, read_user_object, write_user_object};

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerType {
    NotificationTimer = 0,
    SynchronizationTimer = 1,
}

/// Refresh a timer against its deadline clock. Relative deadlines and periodic
/// intervals use monotonic interrupt time; absolute deadlines track wall time.
/// Missed periods coalesce into one signal and the next future deadline.
pub(crate) fn refresh_timer(state: &mut TimerState) {
    let now = crate::ke::time::get_time_snapshot();
    refresh_timer_at(state, now.system_time, now.interrupt_time);
}

pub(crate) fn refresh_timer_at(state: &mut TimerState, wall: u64, uptime: u64) {
    if !state.armed {
        return;
    }
    let now = if state.absolute { wall } else { uptime };
    if now < state.deadline {
        return;
    }
    state.signaled = true;
    if state.period_ms == 0 {
        state.armed = false;
    } else {
        let period = u64::from(state.period_ms) * 10_000;
        let next = period - (now - state.deadline) % period;
        state.deadline = uptime.saturating_add(next);
        state.absolute = false;
    }
}

/// Allocate a timer for trusted kernel32 callers.
pub fn create_timer_kernel(timer_type: u32) -> Result<HANDLE, NTSTATUS> {
    if timer_type > 1 {
        return Err(STATUS_INVALID_PARAMETER);
    }
    let handle = alloc_dispatcher_handle(
        HandleKind::Timer,
        0,
        DispatcherState::Timer(TimerState {
            timer_type,
            deadline: 0,
            absolute: false,
            period_ms: 0,
            signaled: false,
            armed: false,
        }),
    );
    if handle.is_null() {
        Err(STATUS_NO_MEMORY)
    } else {
        Ok(handle)
    }
}

/// Create a native timer object.
///
/// # Safety
/// The output pointer is a user address validated by user_copy.
pub unsafe extern "C" fn nt_create_timer(
    timer_handle: *mut HANDLE,
    desired_access: u32,
    object_attributes: *mut super::types::ObjectAttributes,
    timer_type: u32,
) -> NTSTATUS {
    if timer_type > 1 {
        return STATUS_INVALID_PARAMETER;
    }
    if let Err(status) = probe_user_write(timer_handle as u64, core::mem::size_of::<HANDLE>()) {
        return status;
    }
    super::object::create_native_sync(
        timer_handle,
        desired_access,
        object_attributes,
        HandleKind::Timer,
        0,
        DispatcherState::Timer(TimerState {
            timer_type,
            deadline: 0,
            absolute: false,
            period_ms: 0,
            signaled: false,
            armed: false,
        }),
    )
}

/// Open an existing timer in the dispatcher namespace.
///
/// # Safety
/// Pointers belong to the calling user process.
pub unsafe extern "C" fn nt_open_timer(
    timer_handle: *mut HANDLE,
    desired_access: u32,
    object_attributes: *mut super::types::ObjectAttributes,
) -> NTSTATUS {
    super::object::open_native_sync(
        timer_handle,
        desired_access,
        object_attributes,
        HandleKind::Timer,
    )
}

pub type TimerApcRoutine = unsafe extern "C" fn(PVOID, u32, i32);

/// Win7 TIMER_SET_COALESCABLE_TIMER_INFO, shared by all supported 64-bit ABIs.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct TimerSetCoalescableTimerInfo {
    pub due_time: i64,
    pub apc_routine: PVOID,
    pub context: PVOID,
    pub wake_context: PVOID,
    pub period: u32,
    pub tolerable_delay: u32,
    pub previous_state: *mut u8,
}

const _: () = assert!(core::mem::size_of::<TimerSetCoalescableTimerInfo>() == 48);

/// Arm a coalescable timer. Delivering at the requested deadline is permitted
/// for any tolerance; the fixed-tick backend need not postpone an expiration.
/// APC delivery and wake requests have the same explicit limitations as
/// NtSetTimer and fail before changing the timer.
///
/// # Safety
/// The information buffer and nested output address are user addresses.
pub unsafe extern "C" fn nt_set_timer_ex(
    timer_handle: HANDLE,
    class: u32,
    information: PVOID,
    length: u32,
) -> NTSTATUS {
    if class != 0 {
        return STATUS_INVALID_INFO_CLASS;
    }
    if length as usize != core::mem::size_of::<TimerSetCoalescableTimerInfo>() {
        return STATUS_INFO_LENGTH_MISMATCH;
    }
    let info = match read_user_object::<TimerSetCoalescableTimerInfo>(information as u64) {
        Ok(info) => info,
        Err(status) => return status,
    };
    if info.period > i32::MAX as u32 {
        return STATUS_INVALID_PARAMETER;
    }
    if !info.previous_state.is_null() {
        if let Err(status) = probe_user_write(info.previous_state as u64, 1) {
            return status;
        }
    }
    change_timer(
        timer_handle,
        info.due_time,
        info.apc_routine,
        info.context,
        u8::from(!info.wake_context.is_null()),
        info.period as i32,
        |signaled| {
            if info.previous_state.is_null() {
                Ok(())
            } else {
                write_user_object(info.previous_state as u64, &u8::from(signaled))
            }
        },
    )
}

/// Arm/rearm a timer, clearing its old signal state. The previous-state output
/// is the BOOLEAN signal state before resetting, not whether it was armed.
///
/// # Safety
/// Input/output pointers are user addresses captured via user_copy.
pub unsafe extern "C" fn nt_set_timer(
    timer_handle: HANDLE,
    due_time: *const LargeInteger,
    timer_apc_routine: PVOID,
    timer_context: PVOID,
    resume: u8,
    period: i32,
    previous_state: *mut u8,
) -> NTSTATUS {
    if period < 0 {
        return STATUS_INVALID_PARAMETER;
    }
    let due = match read_user_object::<LargeInteger>(due_time as u64) {
        Ok(value) => value.quad_part,
        Err(status) => return status,
    };
    if !previous_state.is_null() {
        if let Err(status) = probe_user_write(previous_state as u64, 1) {
            return status;
        }
    }
    change_timer(
        timer_handle,
        due,
        timer_apc_routine,
        timer_context,
        resume,
        period,
        |signaled| {
            if previous_state.is_null() {
                Ok(())
            } else {
                write_user_object(previous_state as u64, &u8::from(signaled))
            }
        },
    )
}

/// Trusted caller has already captured the 64-bit interval. No kernel address
/// is routed through the user-memory capture performed by NtSetTimer.
pub fn set_timer_kernel(
    timer_handle: HANDLE,
    due: i64,
    timer_apc_routine: PVOID,
    timer_context: PVOID,
    resume: u8,
    period: i32,
) -> NTSTATUS {
    change_timer(
        timer_handle,
        due,
        timer_apc_routine,
        timer_context,
        resume,
        period,
        |_| Ok(()),
    )
}

fn change_timer(
    timer_handle: HANDLE,
    due: i64,
    timer_apc_routine: PVOID,
    _timer_context: PVOID,
    resume: u8,
    period: i32,
    previous: impl FnOnce(bool) -> Result<(), NTSTATUS>,
) -> NTSTATUS {
    if period < 0 {
        return STATUS_INVALID_PARAMETER;
    }
    with_sync_access(timer_handle, 2, |entries| {
        if entries[0].kind != HandleKind::Timer {
            return Err(STATUS_OBJECT_TYPE_MISMATCH);
        }
        let DispatcherState::Timer(state) = &mut entries[0].dispatcher else {
            return Err(STATUS_INVALID_HANDLE);
        };
        if !timer_apc_routine.is_null() || resume != 0 {
            return Err(STATUS_NOT_SUPPORTED);
        }
        refresh_timer(state);
        previous(state.signaled)?;
        let now = crate::ke::time::get_time_snapshot();
        state.deadline = if due < 0 {
            now.interrupt_time.saturating_add(due.unsigned_abs())
        } else {
            due as u64
        };
        state.absolute = due >= 0;
        state.period_ms = period as u32;
        state.signaled = false;
        state.armed = true;
        refresh_timer_at(state, now.system_time, now.interrupt_time);
        Ok(STATUS_SUCCESS)
    })
    .unwrap_or_else(|status| status)
}

/// Cancel future expirations while preserving the current signal state.
///
/// # Safety
/// The optional output pointer is validated by user_copy.
pub unsafe extern "C" fn nt_cancel_timer(timer_handle: HANDLE, current_state: *mut u8) -> NTSTATUS {
    if !current_state.is_null() {
        if let Err(status) = probe_user_write(current_state as u64, 1) {
            return status;
        }
    }
    with_sync_access(timer_handle, 2, |entries| {
        if entries[0].kind != HandleKind::Timer {
            return Err(STATUS_OBJECT_TYPE_MISMATCH);
        }
        let DispatcherState::Timer(state) = &mut entries[0].dispatcher else {
            return Err(STATUS_INVALID_HANDLE);
        };
        refresh_timer(state);
        if !current_state.is_null() {
            write_user_object(current_state as u64, &u8::from(state.signaled))?;
        }
        state.armed = false;
        Ok(STATUS_SUCCESS)
    })
    .unwrap_or_else(|status| status)
}

#[repr(u32)]
pub enum TimerInformationClass {
    TimerBasicInformation = 0,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct TimerBasicInformation {
    pub remaining_time: LargeInteger,
    pub timer_state: u8,
}

/// Query the native 16-byte TIMER_BASIC_INFORMATION. Padding is explicitly zero.
///
/// # Safety
/// Output pointers are user addresses validated by user_copy.
pub unsafe extern "C" fn nt_query_timer(
    handle: HANDLE,
    class: u32,
    information: PVOID,
    length: u32,
    return_length: *mut u32,
) -> NTSTATUS {
    if class != 0 {
        return STATUS_INVALID_INFO_CLASS;
    }
    if let Err(status) = super::sync::query_output(information, length, 16, return_length) {
        return status;
    }
    with_sync_access(handle, 1, |entries| {
        if entries[0].kind != HandleKind::Timer {
            return Err(STATUS_OBJECT_TYPE_MISMATCH);
        }
        let DispatcherState::Timer(state) = &mut entries[0].dispatcher else {
            return Err(STATUS_INVALID_HANDLE);
        };
        let now = crate::ke::time::get_time_snapshot();
        refresh_timer_at(state, now.system_time, now.interrupt_time);
        let clock = if state.absolute {
            now.system_time
        } else {
            now.interrupt_time
        };
        let remaining = (i128::from(state.deadline) - i128::from(clock))
            .clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64;
        let mut bytes = [0; 16];
        bytes[..8].copy_from_slice(&remaining.to_le_bytes());
        bytes[8] = u8::from(state.signaled);
        super::sync::write_query_output(information, &bytes, return_length)
    })
    .unwrap_or_else(|status| status)
}

/// # Safety
/// Name and security must be readable trusted Win32 arguments.
pub unsafe fn create_timer_win32(
    security: *const u8,
    name: *const u16,
    timer_type: u32,
) -> Result<(HANDLE, NTSTATUS), NTSTATUS> {
    let attributes = unsafe { super::object::win32_sync_attributes(security, name)? };
    super::file::create_sync_handle(
        HandleKind::Timer,
        0,
        DispatcherState::Timer(TimerState {
            timer_type,
            deadline: 0,
            absolute: false,
            period_ms: 0,
            signaled: false,
            armed: false,
        }),
        0x001f0003,
        attributes,
        true,
    )
}
