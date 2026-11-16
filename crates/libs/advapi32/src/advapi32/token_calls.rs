//! Win32 token calls backed by the captured native token operations.

use crate::libs::ntdll::{
    security, status,
    types::{HANDLE, PVOID},
};
use crate::mm::user_copy::{probe_user_write, write_user_object};

fn failed(error: i32) -> i32 {
    crate::libs::kernel32::error::set_last_error(status::nt_status_to_dos_error(error));
    0
}

fn boolean_status(result: i32) -> i32 {
    if result < 0 {
        failed(result)
    } else {
        1
    }
}

/// # Safety
/// Output pointers must name writable caller memory of the declared lengths.
#[no_mangle]
pub unsafe extern "C" fn GetTokenInformation(
    token: HANDLE,
    class: u32,
    information: PVOID,
    length: u32,
    return_length: *mut u32,
) -> i32 {
    boolean_status(unsafe {
        security::nt_query_information_token(token, class, information, length, return_length)
    })
}

/// # Safety
/// Information and embedded pointers must name readable caller memory.
#[no_mangle]
pub unsafe extern "C" fn SetTokenInformation(
    token: HANDLE,
    class: u32,
    information: PVOID,
    length: u32,
) -> i32 {
    boolean_status(unsafe { security::nt_set_information_token(token, class, information, length) })
}

/// # Safety
/// Required privileges is a writable variable-length PRIVILEGE_SET. Result
/// points to a four-byte Win32 BOOL, rather than the native one-byte BOOLEAN.
#[no_mangle]
pub unsafe extern "C" fn PrivilegeCheck(token: HANDLE, required: PVOID, result: *mut i32) -> i32 {
    let operation = || {
        probe_user_write(result as u64, 4)?;
        let allowed = security::check_privileges_for_caller(token, required.cast())?;
        write_user_object(result as u64, &i32::from(allowed))
    };
    boolean_status(operation().err().unwrap_or(status::STATUS_SUCCESS))
}

/// # Safety
/// Descriptor, mapping and privilege buffers must name caller memory. Both
/// granted and access_status point to four-byte output values.
#[no_mangle]
pub unsafe extern "C" fn AccessCheck(
    descriptor: *mut security::SecurityDescriptor,
    token: HANDLE,
    desired: u32,
    mapping: PVOID,
    privileges: PVOID,
    privilege_length: *mut u32,
    granted: *mut u32,
    access_status: *mut i32,
) -> i32 {
    let operation = || {
        probe_user_write(granted as u64, 4)?;
        probe_user_write(access_status as u64, 4)?;
        let result = security::check_access_for_caller(
            descriptor,
            token,
            desired,
            mapping.cast(),
            privileges.cast(),
            privilege_length,
        )?;
        write_user_object(granted as u64, &result.granted)?;
        write_user_object(access_status as u64, &i32::from(result.status >= 0))?;
        if result.status < 0 {
            crate::libs::kernel32::error::set_last_error(status::nt_status_to_dos_error(
                result.status,
            ));
        }
        Ok::<(), i32>(())
    };
    boolean_status(operation().err().unwrap_or(status::STATUS_SUCCESS))
}

/// # Safety
/// All buffers must name caller memory matching the NT TOKEN_PRIVILEGES layout.
#[no_mangle]
pub unsafe extern "C" fn AdjustTokenPrivileges(
    token: HANDLE,
    disable_all: i32,
    new_state: *const security::TOKEN_PRIVILEGES,
    buffer_length: u32,
    previous_state: *mut security::TOKEN_PRIVILEGES,
    return_length: *mut u32,
) -> i32 {
    let result = unsafe {
        security::nt_adjust_privileges_token(
            token,
            u8::from(disable_all != 0),
            new_state,
            buffer_length,
            previous_state,
            return_length,
        )
    };
    // STATUS_NOT_ALL_ASSIGNED returns TRUE with ERROR_NOT_ALL_ASSIGNED.
    crate::libs::kernel32::error::set_last_error(status::nt_status_to_dos_error(result));
    i32::from(result >= 0)
}

/// # Safety
/// NewState and embedded SIDs are readable; optional outputs name writable
/// buffers following the NT64 TOKEN_GROUPS layout.
#[no_mangle]
pub unsafe extern "C" fn AdjustTokenGroups(
    token: HANDLE,
    reset_to_default: i32,
    new_state: *const security::TOKEN_GROUPS,
    buffer_length: u32,
    previous_state: *mut security::TOKEN_GROUPS,
    return_length: *mut u32,
) -> i32 {
    let result = unsafe {
        security::nt_adjust_groups_token(
            token,
            u8::from(reset_to_default != 0),
            new_state,
            buffer_length,
            previous_state,
            return_length,
        )
    };
    crate::libs::kernel32::error::set_last_error(status::nt_status_to_dos_error(result));
    i32::from(result >= 0)
}
