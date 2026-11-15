//! kernel32 — handle management
//
//! `close_handle`, `duplicate_handle`, `is_valid_handle`,
//! `get_handle_information`, `set_handle_information`. Wraps
//! the shared native handle table. Attributes are per handle slot.

use super::types::{
    BOOL, DWORD, FALSE, HANDLE, HANDLE_CURRENT_PROCESS, HANDLE_CURRENT_THREAD, TRUE,
};
use crate::libs::ntdll::file::nt_close;
use crate::libs::ntdll::{object, status};

fn fail(error: i32) -> BOOL {
    super::error::set_last_error(status::nt_status_to_dos_error(error));
    FALSE
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `close_handle`.
pub unsafe extern "C" fn close_handle(handle: HANDLE) -> BOOL {
    let result = nt_close(handle);
    if result >= 0 {
        TRUE
    } else {
        fail(result)
    }
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// Duplicate a dispatcher handle in the current process, preserving shared
/// signal state and storing inheritance/protection in independent slots.
pub unsafe extern "C" fn duplicate_handle(
    source_process: HANDLE,
    source: HANDLE,
    target_process: HANDLE,
    target: *mut HANDLE,
    desired_access: DWORD,
    inherit: i32,
    options: DWORD,
) -> BOOL {
    if options & !3 != 0 {
        return fail(status::STATUS_INVALID_PARAMETER);
    }
    if target.is_null() {
        return fail(status::STATUS_INVALID_PARAMETER);
    }
    match object::duplicate_object_kernel(
        source_process,
        source,
        target_process,
        desired_access,
        if inherit != 0 { 2 } else { 0 },
        options,
    ) {
        Ok(handle) => {
            // SAFETY: this trusted wrapper's caller supplies a writable HANDLE.
            unsafe {
                target.write_unaligned(handle);
            }
            TRUE
        }
        Err(error) => fail(error),
    }
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `is_valid_handle` — true if the handle is non-null and
/// not a pseudo-handle.
pub unsafe extern "C" fn is_valid_handle(handle: HANDLE) -> BOOL {
    if handle.is_null() {
        return FALSE;
    }
    if handle == HANDLE_CURRENT_PROCESS || handle == HANDLE_CURRENT_THREAD {
        return TRUE;
    }
    i32::from(object::query_handle_flags(handle).is_ok())
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// Return the slot's inheritance/protection flags, preserving LastError on success.
pub unsafe extern "C" fn get_handle_information(handle: HANDLE, flags: *mut DWORD) -> BOOL {
    if flags.is_null() {
        return fail(status::STATUS_INVALID_PARAMETER);
    }
    match object::query_handle_flags(handle) {
        Ok(value) => {
            // SAFETY: this trusted wrapper's caller supplies a writable DWORD.
            unsafe {
                flags.write_unaligned(value);
            }
            TRUE
        }
        Err(error) => fail(error),
    }
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
pub unsafe extern "C" fn set_handle_information(handle: HANDLE, mask: DWORD, flags: DWORD) -> BOOL {
    match object::set_handle_flags(handle, mask, flags) {
        Ok(()) => TRUE,
        Err(error) => fail(error),
    }
}
