//! kernel32 — synchronisation primitives
//
//! `wait_for_single_object`, `wait_for_multiple_objects`,
//! `create_event_w`, `set_event`, `reset_event`, `pulse_event`,
//! `create_mutex_w`, `release_mutex`, `create_semaphore_w`,
//! `release_semaphore`, `initialize_critical_section`,
//! `enter_critical_section`, `leave_critical_section`,
//! `delete_critical_section`. Object operations forward to the native dispatcher.

use super::error::set_last_error;
use super::types::{BOOL, DWORD, FALSE, HANDLE, LPCWSTR, TRUE};
use crate::libs::ntdll::status::nt_status_to_dos_error;
use crate::libs::ntdll::sync as ntdll_sync;
use core::ptr;
use core::sync::atomic::{AtomicU32, Ordering};

fn bool_result(status: i32) -> BOOL {
    if status >= 0 {
        TRUE
    } else {
        set_last_error(nt_status_to_dos_error(status));
        FALSE
    }
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `create_event_w`.
pub unsafe extern "C" fn create_event_w(
    _security_attributes: *const u8,
    manual_reset: BOOL,
    initial_state: BOOL,
    _name: LPCWSTR,
) -> HANDLE {
    let event_type = if manual_reset != 0 { 0 } else { 1 };
    match ntdll_sync::create_event_kernel(event_type, initial_state != 0) {
        Ok(handle) => handle,
        Err(status) => {
            set_last_error(crate::libs::ntdll::status::nt_status_to_dos_error(status));
            ptr::null_mut()
        }
    }
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `set_event` / `reset_event` / `pulse_event`.
pub unsafe extern "C" fn set_event(event: HANDLE) -> BOOL {
    bool_result(ntdll_sync::nt_set_event(event, ptr::null_mut()))
}
/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
pub unsafe extern "C" fn reset_event(event: HANDLE) -> BOOL {
    bool_result(ntdll_sync::nt_reset_event(event, ptr::null_mut()))
}
/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
pub unsafe extern "C" fn pulse_event(event: HANDLE) -> BOOL {
    bool_result(ntdll_sync::nt_pulse_event(event, ptr::null_mut()))
}

// ---------------------------------------------------------------------------
// Mutexes
// ---------------------------------------------------------------------------

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `create_mutex_w`.
pub unsafe extern "C" fn create_mutex_w(
    _security: *const u8,
    initial_owner: BOOL,
    _name: LPCWSTR,
) -> HANDLE {
    match ntdll_sync::create_mutant_kernel(initial_owner != 0) {
        Ok(handle) => handle,
        Err(status) => {
            set_last_error(nt_status_to_dos_error(status));
            ptr::null_mut()
        }
    }
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `release_mutex`.
pub unsafe extern "C" fn release_mutex(mutex: HANDLE) -> BOOL {
    bool_result(ntdll_sync::nt_release_mutant(mutex, ptr::null_mut()))
}

// ---------------------------------------------------------------------------
// Semaphores
// ---------------------------------------------------------------------------

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `create_semaphore_w`.
pub unsafe extern "C" fn create_semaphore_w(
    _security: *const u8,
    initial_count: DWORD,
    maximum_count: DWORD,
    _name: LPCWSTR,
) -> HANDLE {
    match ntdll_sync::create_semaphore_kernel(initial_count, maximum_count) {
        Ok(handle) => handle,
        Err(status) => {
            set_last_error(crate::libs::ntdll::status::nt_status_to_dos_error(status));
            ptr::null_mut()
        }
    }
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `release_semaphore`.
pub unsafe extern "C" fn release_semaphore(
    semaphore: HANDLE,
    release_count: DWORD,
    previous_count: *mut DWORD,
) -> BOOL {
    bool_result(ntdll_sync::nt_release_semaphore(
        semaphore,
        release_count,
        previous_count,
    ))
}

// ---------------------------------------------------------------------------
// Wait
// ---------------------------------------------------------------------------

const WAIT_OBJECT_0: DWORD = 0;
pub const WAIT_ABANDONED: DWORD = 0x80;
const WAIT_TIMEOUT: DWORD = 0x102;
const WAIT_FAILED: DWORD = 0xFFFF_FFFF;
pub const INFINITE: DWORD = 0xFFFF_FFFF;

fn wait_timeout(milliseconds: DWORD) -> Option<i64> {
    if milliseconds == INFINITE {
        None
    } else {
        Some(-(i64::from(milliseconds) * 10_000))
    }
}

fn wait_result(status: i32) -> DWORD {
    if (0..64).contains(&status)
        || (0x80..0xc0).contains(&status)
        || status == 0x102
        || status == 0xc0
    {
        status as DWORD
    } else {
        set_last_error(crate::libs::ntdll::status::nt_status_to_dos_error(status));
        WAIT_FAILED
    }
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `wait_for_single_object`.
pub unsafe extern "C" fn wait_for_single_object(handle: HANDLE, milliseconds: DWORD) -> DWORD {
    wait_result(ntdll_sync::nt_wait_for_single_object_kernel(
        handle,
        0,
        wait_timeout(milliseconds),
    ))
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `wait_for_multiple_objects`.
pub unsafe extern "C" fn wait_for_multiple_objects(
    count: DWORD,
    handles: *const HANDLE,
    wait_all: BOOL,
    milliseconds: DWORD,
) -> DWORD {
    if count == 0 || count > 64 || handles.is_null() {
        set_last_error(87);
        return WAIT_FAILED;
    }
    let wait_type = if wait_all != 0 { 0 } else { 1 };
    let status = ntdll_sync::nt_wait_for_multiple_objects_kernel(
        count,
        unsafe { core::slice::from_raw_parts(handles, count as usize) },
        wait_type,
        0,
        wait_timeout(milliseconds),
    );
    wait_result(status)
}

/// # Safety
/// Handles identify dispatcher objects owned by the caller.
pub unsafe extern "C" fn signal_object_and_wait(
    signal: HANDLE,
    handle: HANDLE,
    milliseconds: DWORD,
    alertable: BOOL,
) -> DWORD {
    wait_result(ntdll_sync::signal_and_wait_kernel(
        signal,
        handle,
        u8::from(alertable != 0),
        wait_timeout(milliseconds),
    ))
}

// ---------------------------------------------------------------------------
// Critical section (user-mode spinlock wrapper)
// ---------------------------------------------------------------------------

const CRITICAL_SECTION_MAGIC: u32 = 0xDEAD_C0DE;

/// `CRITICAL_SECTION` (RTL_CRITICAL_SECTION in user mode).
#[repr(C)]
pub struct CriticalSection {
    pub lock: AtomicU32,
    pub recursion_count: u32,
    pub owning_thread: u32,
    pub magic: u32,
    pub spin_count: u32,
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `initialize_critical_section`.
pub unsafe extern "C" fn initialize_critical_section(cs: *mut CriticalSection) {
    if cs.is_null() {
        return;
    }
    (*cs).lock = AtomicU32::new(0);
    (*cs).recursion_count = 0;
    (*cs).owning_thread = 0;
    (*cs).magic = CRITICAL_SECTION_MAGIC;
    (*cs).spin_count = 4000;
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `enter_critical_section`.
pub unsafe extern "C" fn enter_critical_section(cs: *mut CriticalSection) {
    if cs.is_null() {
        return;
    }
    let thread_id = current_thread_id();
    let spin_count = (*cs).spin_count.max(4000);
    let mut spins = 0;
    loop {
        match (*cs).lock.load(Ordering::Acquire) {
            0 => {
                if (*cs)
                    .lock
                    .compare_exchange_weak(0, thread_id, Ordering::Acquire, Ordering::Relaxed)
                    .is_ok()
                {
                    (*cs).recursion_count = 1;
                    (*cs).owning_thread = thread_id;
                    return;
                }
            }
            owner if owner == thread_id => {
                (*cs).recursion_count += 1;
                return;
            }
            _ => {
                if spins >= spin_count {
                    loop {
                        if (*cs)
                            .lock
                            .compare_exchange_weak(
                                0,
                                thread_id,
                                Ordering::Acquire,
                                Ordering::Relaxed,
                            )
                            .is_ok()
                        {
                            (*cs).recursion_count = 1;
                            (*cs).owning_thread = thread_id;
                            return;
                        }
                        core::hint::spin_loop();
                    }
                }
                core::hint::spin_loop();
                spins = spins.saturating_add(1);
            }
        }
    }
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `leave_critical_section`.
pub unsafe extern "C" fn leave_critical_section(cs: *mut CriticalSection) {
    if cs.is_null() {
        return;
    }
    let thread_id = current_thread_id();
    if (*cs).owning_thread != thread_id || (*cs).recursion_count == 0 {
        return;
    }
    (*cs).recursion_count -= 1;
    if (*cs).recursion_count == 0 {
        (*cs).owning_thread = 0;
        (*cs).lock.store(0, Ordering::Release);
    }
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `delete_critical_section`.
pub unsafe extern "C" fn delete_critical_section(cs: *mut CriticalSection) {
    if cs.is_null() {
        return;
    }
    (*cs).magic = 0;
}

fn current_thread_id() -> u32 {
    let thread = crate::arch::common::percpu::get_current_thread();
    if thread.is_null() {
        return 0;
    }
    unsafe { (*thread).client_id.unique_thread as u32 }
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `initialize_critical_section_and_spin_count` / `set_critical_section_spin_count`.
pub unsafe extern "C" fn initialize_critical_section_and_spin_count(
    cs: *mut CriticalSection,
    spins: DWORD,
) -> BOOL {
    initialize_critical_section(cs);
    (*cs).spin_count = spins & 0xFFFFFFFF;
    TRUE
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
pub unsafe extern "C" fn set_critical_section_spin_count(
    cs: *mut CriticalSection,
    spins: DWORD,
) -> DWORD {
    if cs.is_null() {
        return 0;
    }
    let old = (*cs).spin_count;
    (*cs).spin_count = spins & 0xFFFFFFFF;
    old
}
