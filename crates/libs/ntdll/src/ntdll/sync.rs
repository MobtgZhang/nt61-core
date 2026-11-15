//! ntdll — Nt* synchronisation primitives
//
//! `nt_create_event`, `nt_create_mutant`, `nt_create_semaphore`,
//! `nt_wait_for_single_object`, `nt_wait_for_multiple_objects`,
//! `nt_delay_execution`. All objects are tracked in the kernel
//! handle table. Wait requests pin shared objects, consume signals atomically,
//! and observe signals, abandonment and deadlines while yielding the scheduler.
//
//! References: MSDN Library "Windows 7" — `ntdll.dll` sync
//! APIs.

use super::file::{
    alloc_dispatcher_handle, alloc_handle, free_handle, with_sync_handles, DispatcherState,
    HandleKind, MutantState,
};
use super::status::{
    STATUS_ACCESS_VIOLATION, STATUS_INVALID_HANDLE, STATUS_INVALID_PARAMETER, STATUS_SUCCESS,
};
use super::types::{HANDLE, NTSTATUS};
use crate::mm::user_copy::{
    copy_from_user, copy_to_user, probe_user_write, read_user_object, write_user_object,
};

/// Event state tracking for proper kernel event integration.
pub(crate) struct EventState {
    pub signaled: bool,
}

/// Decode event state from handle target.
fn decode_event_state(target: u64) -> EventState {
    EventState {
        signaled: (target & 0x01) != 0,
    }
}

/// Encode event state into handle target.
fn encode_event_state(event_type: u32, signaled: bool, manual_reset: bool) -> u64 {
    let et = (event_type as u64) << 32;
    let sig = if signaled { 0x01u64 } else { 0x00 };
    let mr = if manual_reset { 0x100u64 } else { 0x00 };
    et | mr | sig
}

fn change_event(handle: HANDLE, previous: *mut u32, signaled: bool) -> NTSTATUS {
    if !previous.is_null() {
        if let Err(status) = probe_user_write(previous as u64, core::mem::size_of::<u32>()) {
            return status;
        }
    }
    with_sync_handles(&[handle], |entries| {
        let entry = &mut entries[0];
        if entry.kind != HandleKind::Event {
            return Err(super::status::STATUS_OBJECT_TYPE_MISMATCH);
        }
        if !previous.is_null() {
            write_user_object(previous as u64, &((entry.target & 1) as u32))?;
        }
        entry.target = (entry.target & !1) | u64::from(signaled);
        Ok(STATUS_SUCCESS)
    })
    .unwrap_or_else(|status| status)
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `nt_create_event`.
pub unsafe extern "C" fn nt_create_event(
    event_handle: *mut HANDLE,
    desired_access: u32,
    object_attributes: *mut super::types::ObjectAttributes,
    event_type: u32,
    initial_state: u8,
) -> NTSTATUS {
    use super::status::STATUS_INVALID_PARAMETER;

    if event_handle.is_null() || event_type > 1 {
        return STATUS_INVALID_PARAMETER;
    }
    if let Err(status) = probe_user_write(event_handle as u64, core::mem::size_of::<HANDLE>()) {
        return status;
    }
    let _ = (desired_access, object_attributes);

    match create_event_kernel(event_type, initial_state != 0) {
        Ok(handle) => match write_user_object(event_handle as u64, &handle) {
            Ok(()) => STATUS_SUCCESS,
            Err(status) => {
                free_handle(handle);
                status
            }
        },
        Err(status) => status,
    }
}

/// Create an unnamed event for trusted callers without routing a kernel pointer
/// through the user-mode output contract.
pub fn create_event_kernel(event_type: u32, signaled: bool) -> Result<HANDLE, NTSTATUS> {
    if event_type > 1 {
        return Err(STATUS_INVALID_PARAMETER);
    }
    let handle = alloc_handle(
        HandleKind::Event,
        encode_event_state(event_type, signaled, event_type == 0),
    );
    if handle.is_null() {
        Err(super::status::STATUS_NO_MEMORY)
    } else {
        Ok(handle)
    }
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `nt_set_event` / `nt_reset_event` / `nt_pulse_event`.
///
/// Set the stored signal state and return its previous value.
pub unsafe extern "C" fn nt_set_event(event_handle: HANDLE, previous_state: *mut u32) -> NTSTATUS {
    change_event(event_handle, previous_state, true)
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// nt_reset_event sets the event to the non-signaled state.
pub unsafe extern "C" fn nt_reset_event(
    event_handle: HANDLE,
    previous_state: *mut u32,
) -> NTSTATUS {
    change_event(event_handle, previous_state, false)
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// nt_clear_event clears the event to non-signaled state (same as reset_event).
pub unsafe extern "C" fn nt_clear_event(event_handle: HANDLE) -> NTSTATUS {
    change_event(event_handle, core::ptr::null_mut(), false)
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// nt_pulse_event sets the event to signaled state, releases waiting threads,
/// then resets to non-signaled. For auto-reset events, only one thread is released.
pub unsafe extern "C" fn nt_pulse_event(
    event_handle: HANDLE,
    previous_state: *mut u32,
) -> NTSTATUS {
    if !previous_state.is_null() {
        if let Err(status) = probe_user_write(previous_state as u64, 4) {
            return status;
        }
    }
    super::file::pulse_event(event_handle, previous_state)
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `nt_create_mutant` (mutex).
/// A mutex can be created with or without initial ownership.
pub unsafe extern "C" fn nt_create_mutant(
    mutant_handle: *mut HANDLE,
    desired_access: u32,
    object_attributes: *mut super::types::ObjectAttributes,
    initial_owner: u8,
) -> NTSTATUS {
    use super::status::STATUS_INVALID_PARAMETER;

    if mutant_handle.is_null() {
        return STATUS_INVALID_PARAMETER;
    }
    if let Err(status) = probe_user_write(mutant_handle as u64, core::mem::size_of::<HANDLE>()) {
        return status;
    }
    let _ = (desired_access, object_attributes);

    match create_mutant_kernel(initial_owner != 0) {
        Ok(handle) => match write_user_object(mutant_handle as u64, &handle) {
            Ok(()) => STATUS_SUCCESS,
            Err(status) => {
                free_handle(handle);
                status
            }
        },
        Err(status) => status,
    }
}

/// Allocate an unnamed mutant for trusted callers.
pub fn create_mutant_kernel(initial_owner: bool) -> Result<HANDLE, NTSTATUS> {
    let (owner_process, owner_thread) = current_client_id();
    if initial_owner && owner_thread == 0 {
        return Err(STATUS_INVALID_HANDLE);
    }
    let state = MutantState {
        owner_thread: if initial_owner { owner_thread } else { 0 },
        owner_process: if initial_owner { owner_process } else { 0 },
        depth: u32::from(initial_owner),
        abandoned: false,
    };
    let h = alloc_dispatcher_handle(HandleKind::Mutant, 0, DispatcherState::Mutant(state));
    if h.is_null() {
        return Err(super::status::STATUS_NO_MEMORY);
    }
    Ok(h)
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `nt_release_mutant`.
/// Releases ownership of the mutex. If the thread doesn't own the mutex,
/// returns an error.
pub unsafe extern "C" fn nt_release_mutant(
    mutant_handle: HANDLE,
    previous_count: *mut u32,
) -> NTSTATUS {
    if !previous_count.is_null() {
        if let Err(status) = probe_user_write(previous_count as u64, core::mem::size_of::<u32>()) {
            return status;
        }
    }
    let (_, tid) = current_client_id();
    with_sync_handles(&[mutant_handle], |entries| {
        let entry = &mut entries[0];
        if entry.kind != HandleKind::Mutant {
            return Err(super::status::STATUS_OBJECT_TYPE_MISMATCH);
        }
        let DispatcherState::Mutant(state) = &mut entry.dispatcher else {
            return Err(STATUS_INVALID_HANDLE);
        };
        if state.depth == 0 || state.owner_thread != tid || tid == 0 {
            return Err(super::status::STATUS_MUTEX_NOT_OWNED);
        }
        if !previous_count.is_null() {
            write_user_object(
                previous_count as u64,
                &((1i64 - i64::from(state.depth)) as i32),
            )?;
        }
        state.depth -= 1;
        if state.depth == 0 {
            state.owner_thread = 0;
            state.owner_process = 0;
        }
        Ok(STATUS_SUCCESS)
    })
    .unwrap_or_else(|status| status)
}

fn current_client_id() -> (u64, u64) {
    let thread = crate::ps::thread::get_current_ethread();
    if thread.is_null() {
        return (0, 0);
    }
    // SAFETY: the per-CPU current ETHREAD stays live throughout this nonblocking
    // read. Only scalar identifiers are copied; no borrow survives scheduling.
    unsafe {
        (
            (*thread).client_id.unique_process,
            (*thread).client_id.unique_thread,
        )
    }
}

/// Validate every query output before looking at object state. The wire
/// representation is emitted as bytes so no Rust structure padding escapes.
pub(crate) fn query_output(
    information: super::types::PVOID,
    length: u32,
    required: u32,
    return_length: *mut u32,
) -> Result<(), NTSTATUS> {
    if length < required {
        return Err(super::status::STATUS_INFO_LENGTH_MISMATCH);
    }
    probe_user_write(information as u64, required as usize)?;
    if !return_length.is_null() {
        probe_user_write(return_length as u64, 4)?;
    }
    Ok(())
}

pub(crate) fn write_query_output(
    information: super::types::PVOID,
    bytes: &[u8],
    return_length: *mut u32,
) -> Result<NTSTATUS, NTSTATUS> {
    copy_to_user(information as u64, bytes)?;
    if !return_length.is_null() {
        write_user_object(return_length as u64, &(bytes.len() as u32))?;
    }
    Ok(STATUS_SUCCESS)
}

/// Query EVENT_BASIC_INFORMATION (event type and signal state).
///
/// # Safety
/// Output addresses are user pointers and are probed/captured by user_copy.
pub unsafe extern "C" fn nt_query_event(
    handle: HANDLE,
    class: u32,
    information: super::types::PVOID,
    length: u32,
    return_length: *mut u32,
) -> NTSTATUS {
    if class != 0 {
        return super::status::STATUS_INVALID_INFO_CLASS;
    }
    if let Err(status) = query_output(information, length, 8, return_length) {
        return status;
    }
    with_sync_handles(&[handle], |entries| {
        let entry = &entries[0];
        if entry.kind != HandleKind::Event {
            return Err(super::status::STATUS_OBJECT_TYPE_MISMATCH);
        }
        let mut bytes = [0; 8];
        bytes[..4].copy_from_slice(&((entry.target >> 32) as u32).to_le_bytes());
        bytes[4..].copy_from_slice(&((entry.target & 1) as i32).to_le_bytes());
        write_query_output(information, &bytes, return_length)
    })
    .unwrap_or_else(|status| status)
}

/// Query SEMAPHORE_BASIC_INFORMATION without consuming a count.
///
/// # Safety
/// Output addresses are user pointers and are probed/captured by user_copy.
pub unsafe extern "C" fn nt_query_semaphore(
    handle: HANDLE,
    class: u32,
    information: super::types::PVOID,
    length: u32,
    return_length: *mut u32,
) -> NTSTATUS {
    if class != 0 {
        return super::status::STATUS_INVALID_INFO_CLASS;
    }
    if let Err(status) = query_output(information, length, 8, return_length) {
        return status;
    }
    with_sync_handles(&[handle], |entries| {
        let entry = &entries[0];
        if entry.kind != HandleKind::Semaphore {
            return Err(super::status::STATUS_OBJECT_TYPE_MISMATCH);
        }
        let mut bytes = [0; 8];
        bytes[..4].copy_from_slice(&(entry.target as u32).to_le_bytes());
        bytes[4..].copy_from_slice(&((entry.target >> 32) as u32).to_le_bytes());
        write_query_output(information, &bytes, return_length)
    })
    .unwrap_or_else(|status| status)
}

/// Query MUTANT_BASIC_INFORMATION or MUTANT_OWNER_INFORMATION.
///
/// # Safety
/// Output addresses are user pointers and are probed/captured by user_copy.
pub unsafe extern "C" fn nt_query_mutant(
    handle: HANDLE,
    class: u32,
    information: super::types::PVOID,
    length: u32,
    return_length: *mut u32,
) -> NTSTATUS {
    let required = match class {
        0 => 8,
        1 => 16,
        _ => return super::status::STATUS_INVALID_INFO_CLASS,
    };
    if let Err(status) = query_output(information, length, required, return_length) {
        return status;
    }
    let (_, tid) = current_client_id();
    with_sync_handles(&[handle], |entries| {
        if entries[0].kind != HandleKind::Mutant {
            return Err(super::status::STATUS_OBJECT_TYPE_MISMATCH);
        }
        let DispatcherState::Mutant(state) = entries[0].dispatcher else {
            return Err(STATUS_INVALID_HANDLE);
        };
        let mut bytes = [0; 16];
        if class == 0 {
            bytes[..4].copy_from_slice(&((1i64 - i64::from(state.depth)) as i32).to_le_bytes());
            bytes[4] = u8::from(state.depth != 0 && state.owner_thread == tid);
            bytes[5] = u8::from(state.abandoned);
        } else {
            bytes[..8].copy_from_slice(&state.owner_process.to_le_bytes());
            bytes[8..].copy_from_slice(&state.owner_thread.to_le_bytes());
        }
        write_query_output(information, &bytes[..required as usize], return_length)
    })
    .unwrap_or_else(|status| status)
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `nt_create_semaphore`.
pub unsafe extern "C" fn nt_create_semaphore(
    semaphore_handle: *mut HANDLE,
    desired_access: u32,
    object_attributes: *mut super::types::ObjectAttributes,
    initial_count: u32,
    maximum_count: u32,
) -> NTSTATUS {
    if semaphore_handle.is_null()
        || maximum_count == 0
        || maximum_count > i32::MAX as u32
        || initial_count > maximum_count
    {
        return STATUS_INVALID_PARAMETER;
    }
    if let Err(status) = probe_user_write(semaphore_handle as u64, core::mem::size_of::<HANDLE>()) {
        return status;
    }
    let _ = (desired_access, object_attributes);
    match create_semaphore_kernel(initial_count, maximum_count) {
        Ok(handle) => match write_user_object(semaphore_handle as u64, &handle) {
            Ok(()) => STATUS_SUCCESS,
            Err(status) => {
                free_handle(handle);
                status
            }
        },
        Err(status) => status,
    }
}

/// Create an unnamed semaphore for trusted callers.
pub fn create_semaphore_kernel(initial_count: u32, maximum_count: u32) -> Result<HANDLE, NTSTATUS> {
    if maximum_count == 0 || maximum_count > i32::MAX as u32 || initial_count > maximum_count {
        return Err(STATUS_INVALID_PARAMETER);
    }
    let handle = alloc_handle(
        HandleKind::Semaphore,
        (u64::from(maximum_count) << 32) | u64::from(initial_count),
    );
    if handle.is_null() {
        Err(super::status::STATUS_NO_MEMORY)
    } else {
        Ok(handle)
    }
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `nt_release_semaphore`.
pub unsafe extern "C" fn nt_release_semaphore(
    semaphore_handle: HANDLE,
    release_count: u32,
    previous_count: *mut u32,
) -> NTSTATUS {
    if release_count == 0 || release_count > i32::MAX as u32 {
        return STATUS_INVALID_PARAMETER;
    }
    if !previous_count.is_null() {
        if let Err(status) = probe_user_write(previous_count as u64, core::mem::size_of::<u32>()) {
            return status;
        }
    }
    with_sync_handles(&[semaphore_handle], |entries| {
        let entry = &mut entries[0];
        if entry.kind != HandleKind::Semaphore {
            return Err(super::status::STATUS_OBJECT_TYPE_MISMATCH);
        }
        let count = entry.target as u32;
        let maximum = (entry.target >> 32) as u32;
        let new_count = count
            .checked_add(release_count)
            .filter(|value| *value <= maximum)
            .ok_or(0xC000_0047u32 as i32)?; // STATUS_SEMAPHORE_LIMIT_EXCEEDED
        if !previous_count.is_null() {
            write_user_object(previous_count as u64, &count)?;
        }
        entry.target = (u64::from(maximum) << 32) | u64::from(new_count);
        Ok(STATUS_SUCCESS)
    })
    .unwrap_or_else(|status| status)
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `nt_create_timer`.
pub unsafe extern "C" fn nt_create_timer(
    timer_handle: *mut HANDLE,
    desired_access: u32,
    object_attributes: *mut super::types::ObjectAttributes,
    timer_type: u32,
) -> NTSTATUS {
    super::timer::nt_create_timer(timer_handle, desired_access, object_attributes, timer_type)
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `nt_set_timer` - Sets a timer to expire at a specified time.
///
/// Timer types:
/// - 0: NotificationTimer
/// - 1: SynchronizationTimer
///
/// # Arguments
/// * `timer_handle` - handle to the timer object
/// * `due_time` - Absolute or relative time (in 100ns units)
/// * `period` - Timer period in milliseconds (0 for one-shot)
/// * `callback` - APC callback routine (optional)
/// * `arg` - Callback argument
/// * `resume` - If TRUE, alerts the system to resume from sleep/hibernation
/// * `handle` - Optionally receives the timer handle
pub unsafe extern "C" fn nt_set_timer(
    timer_handle: HANDLE,
    due_time: *const i64,
    callback: super::types::PVOID,
    context: super::types::PVOID,
    resume: u8,
    period: i32,
    previous_state: *mut u8,
) -> NTSTATUS {
    super::timer::nt_set_timer(
        timer_handle,
        due_time.cast(),
        callback,
        context,
        resume,
        period,
        previous_state,
    )
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `nt_cancel_timer` - Cancels a timer.
///
/// # Arguments
/// * `timer_handle` - handle to the timer object
/// * `set_handle` - If provided, receives the previous state
pub unsafe extern "C" fn nt_cancel_timer(timer_handle: HANDLE, current_state: *mut u8) -> NTSTATUS {
    super::timer::nt_cancel_timer(timer_handle, current_state)
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `nt_wait_for_single_object` — waits for an object to become signaled.
/// Returns `STATUS_SUCCESS` (0), `STATUS_TIMEOUT`, or `STATUS_INVALID_HANDLE`.
///
/// The timeout is specified in 100-nanosecond intervals:
/// - If negative, it's a relative time (time remaining)
/// - If positive, it's an absolute time (system time when to timeout)
pub unsafe extern "C" fn nt_wait_for_single_object(
    handle: HANDLE,
    alertable: u8,
    timeout: *mut i64,
) -> NTSTATUS {
    if handle.is_null() {
        return STATUS_INVALID_HANDLE;
    }

    let raw_timeout = if timeout.is_null() {
        None
    } else {
        match read_user_object::<i64>(timeout as u64) {
            Ok(raw) => Some(raw),
            Err(STATUS_ACCESS_VIOLATION) => return STATUS_ACCESS_VIOLATION,
            Err(status) => return status,
        }
    };

    nt_wait_for_single_object_kernel(handle, alertable, raw_timeout)
}

/// Trusted-kernel entry point for `nt_wait_for_single_object`.
pub fn nt_wait_for_single_object_kernel(
    handle: HANDLE,
    alertable: u8,
    timeout: Option<i64>,
) -> NTSTATUS {
    nt_wait_for_multiple_objects_kernel(1, &[handle], 1, alertable, timeout)
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `nt_wait_for_multiple_objects`.
pub unsafe extern "C" fn nt_wait_for_multiple_objects(
    count: u32,
    handles: *mut HANDLE,
    wait_type: u32,
    alertable: u8,
    timeout: *mut i64,
) -> NTSTATUS {
    if handles.is_null() || count == 0 {
        return STATUS_INVALID_PARAMETER;
    }

    let mut handle_array = [HANDLE::default(); 64];
    if count as usize > handle_array.len() {
        return STATUS_INVALID_PARAMETER;
    }
    let bytes = core::slice::from_raw_parts_mut(
        handle_array.as_mut_ptr() as *mut u8,
        count as usize * core::mem::size_of::<HANDLE>(),
    );
    if let Err(status) = copy_from_user(bytes, handles as u64) {
        return status;
    }

    let raw_timeout = if timeout.is_null() {
        None
    } else {
        match read_user_object::<i64>(timeout as u64) {
            Ok(raw) => Some(raw),
            Err(status) => return status,
        }
    };

    nt_wait_for_multiple_objects_kernel(
        count,
        &handle_array[..count as usize],
        wait_type,
        alertable,
        raw_timeout,
    )
}

/// Trusted-kernel entry point for `nt_wait_for_multiple_objects`.
pub fn nt_wait_for_multiple_objects_kernel(
    count: u32,
    handles: &[HANDLE],
    wait_type: u32,
    alertable: u8,
    timeout: Option<i64>,
) -> NTSTATUS {
    if handles.len() != count as usize || count == 0 || count > 64 || wait_type > 1 {
        return STATUS_INVALID_PARAMETER;
    }
    let wait_all = wait_type == 0; // WAIT_TYPE: WaitAll=0, WaitAny=1.
    if wait_all
        && handles
            .iter()
            .enumerate()
            .any(|(index, handle)| handles[..index].contains(handle))
    {
        return STATUS_INVALID_PARAMETER;
    }
    let _ = alertable; // User APC delivery requires a user-return trampoline.
    let (pid, tid) = current_client_id();
    complete_wait(super::file::begin_wait(
        handles, wait_all, pid, tid, timeout,
    ))
}

fn complete_wait(start: Result<super::file::WaitStart, NTSTATUS>) -> NTSTATUS {
    match start {
        Ok(super::file::WaitStart::Complete(status)) => status,
        Ok(super::file::WaitStart::Pending(wait)) => loop {
            if let Some(status) = wait.poll() {
                return status;
            }
            // Keep the syscall stack resumable while the dispatcher owns the
            // wait request. Signal/timeout completion is stored before yield;
            // a transient pulse is never lost between checking and yielding.
            crate::ke::scheduler::yield_();
            core::hint::spin_loop();
        },
        Err(status) => status,
    }
}

/// Signal an event, semaphore or owned mutant, then wait for one object.
///
/// # Safety
/// The optional timeout address belongs to the calling user process.
pub unsafe extern "C" fn nt_signal_and_wait_for_single_object(
    signal: HANDLE,
    handle: HANDLE,
    alertable: u8,
    timeout: *const i64,
) -> NTSTATUS {
    let timeout = if timeout.is_null() {
        None
    } else {
        match read_user_object::<i64>(timeout as u64) {
            Ok(raw) => Some(raw),
            Err(status) => return status,
        }
    };
    signal_and_wait_kernel(signal, handle, alertable, timeout)
}

/// Trusted helper using an already captured timeout.
pub fn signal_and_wait_kernel(
    signal: HANDLE,
    handle: HANDLE,
    alertable: u8,
    timeout: Option<i64>,
) -> NTSTATUS {
    let _ = alertable; // APC return trampoline is still required.
    let (pid, tid) = current_client_id();
    complete_wait(super::file::signal_and_begin_wait(
        signal, handle, pid, tid, timeout,
    ))
}

/// Delay until an absolute NT wall time or a relative interrupt-time deadline,
/// expressed in 100ns units. APC interruption is not yet implemented.
pub extern "C" fn nt_delay_execution(alertable: u8, interval: *mut i64) -> NTSTATUS {
    if interval.is_null() {
        return STATUS_INVALID_PARAMETER;
    }

    let raw = match read_user_object::<i64>(interval as u64) {
        Ok(raw) => raw,
        Err(status) => return status,
    };

    nt_delay_execution_kernel(alertable, raw)
}

/// Trusted-kernel entry point for `nt_delay_execution`.
pub fn nt_delay_execution_kernel(alertable: u8, interval: i64) -> NTSTATUS {
    let _ = alertable;
    if interval == 0 {
        crate::ke::scheduler::yield_();
        return STATUS_SUCCESS;
    }
    if interval > 0 && interval as u64 <= crate::ke::time::get_system_time() {
        return STATUS_SUCCESS;
    }
    let start = crate::ke::time::get_time_snapshot();
    let deadline = if interval < 0 {
        start.interrupt_time.saturating_add(interval.unsigned_abs())
    } else {
        interval as u64
    };
    loop {
        let now = crate::ke::time::get_time_snapshot();
        if (if interval < 0 {
            now.interrupt_time
        } else {
            now.system_time
        }) >= deadline
        {
            return STATUS_SUCCESS;
        }
        crate::ke::scheduler::yield_();
        core::hint::spin_loop();
    }
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `NT_CLOSE` re-export for kernel32.
pub unsafe extern "C" fn nt_close_sync(h: HANDLE) -> NTSTATUS {
    super::file::close_handle(h).map_or_else(|status| status, |_| STATUS_SUCCESS)
}
