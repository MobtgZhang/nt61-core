//! Object handle attributes shared by native syscalls and trusted Win32 callers.
//! Attributes belong to a handle slot, not the referenced object's signal state.
use super::file::{handle_flags, update_handle_flags};
use super::status::*;
use super::types::{HANDLE, NTSTATUS, PVOID};
use crate::mm::user_copy::{probe_user_write, read_user_object, write_user_object};

/// Win32 HANDLE_FLAG_INHERIT and HANDLE_FLAG_PROTECT_FROM_CLOSE bit values.
pub fn query_handle_flags(handle: HANDLE) -> Result<u32, NTSTATUS> {
    handle_flags(handle).map(u32::from)
}

/// Modify only bits selected by the Win32 mask. Unknown bits have no effect.
pub fn set_handle_flags(handle: HANDLE, mask: u32, flags: u32) -> Result<(), NTSTATUS> {
    update_handle_flags(handle, (mask & 3) as u8, (flags & 3) as u8)
}

/// Query ObjectHandleFlagInformation (two BOOLEANs, with no padding).
/// Other object information classes still require namespace/type metadata.
///
/// # Safety
/// Output pointers belong to the calling user process and are probed/captured.
pub unsafe extern "C" fn nt_query_object(
    handle: HANDLE,
    class: u32,
    information: PVOID,
    length: u32,
    return_length: *mut u32,
) -> NTSTATUS {
    if class > 5 {
        return STATUS_INVALID_INFO_CLASS;
    }
    if class != 4 {
        return STATUS_NOT_IMPLEMENTED;
    }
    if length != 0 {
        if let Err(status) = probe_user_write(information as u64, length as usize) {
            return status;
        }
    }
    if !return_length.is_null() {
        if let Err(status) = probe_user_write(return_length as u64, 4) {
            return status;
        }
    }
    let flags = match query_handle_flags(handle) {
        Ok(value) => value,
        Err(status) => return status,
    };
    let result = if length != 2 {
        STATUS_INFO_LENGTH_MISMATCH
    } else {
        let output = [u8::from(flags & 1 != 0), u8::from(flags & 2 != 0)];
        match write_user_object(information as u64, &output) {
            Ok(()) => STATUS_SUCCESS,
            Err(status) => return status,
        }
    };
    if !return_length.is_null() {
        if let Err(status) = write_user_object(return_length as u64, &2u32) {
            return status;
        }
    }
    result
}

/// Set ObjectHandleFlagInformation. Capture both BOOLEANs before mutation.
///
/// # Safety
/// The input pointer belongs to the calling user process and is captured.
pub unsafe extern "C" fn nt_set_information_object(
    handle: HANDLE,
    class: u32,
    information: PVOID,
    length: u32,
) -> NTSTATUS {
    if class == 5 {
        return STATUS_NOT_IMPLEMENTED;
    }
    if class != 4 {
        return STATUS_INVALID_INFO_CLASS;
    }
    if length != 2 {
        return STATUS_INFO_LENGTH_MISMATCH;
    }
    let input = match read_user_object::<[u8; 2]>(information as u64) {
        Ok(value) => value,
        Err(status) => return status,
    };
    let flags = u32::from(input[0] != 0) | (u32::from(input[1] != 0) << 1);
    set_handle_flags(handle, 3, flags).map_or_else(|status| status, |()| STATUS_SUCCESS)
}
