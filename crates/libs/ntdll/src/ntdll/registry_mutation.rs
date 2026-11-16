//! Captured mutations of the volatile registry tree. Native tree operations
//! share a lock so existing handles continue to refer to the renamed key.
use super::*;
use crate::libs::ntdll::status::{
    STATUS_INFO_LENGTH_MISMATCH, STATUS_INVALID_INFO_CLASS, STATUS_NOT_SUPPORTED,
    STATUS_OBJECT_NAME_COLLISION, STATUS_OBJECT_NAME_INVALID,
};

/// Rename a key within its parent, retaining all values, children and handles.
/// # Safety
/// `name` designates a caller UNICODE_STRING; its bytes are captured before locks.
pub unsafe extern "C" fn nt_rename_key(handle: HANDLE, name: *const UnicodeString) -> NTSTATUS {
    let header = match read_user_object::<UnicodeString>(name as u64) {
        Ok(header) => header,
        Err(status) => return status,
    };
    if header.length == 0
        || header.length % 2 != 0
        || header.length > 510
        || header.length > header.maximum_length
    {
        return STATUS_OBJECT_NAME_INVALID;
    }
    let name = match read_user_unicode_string(name as u64, 510) {
        Ok(name) => name,
        Err(status) => return status,
    };
    if name.is_empty() || name.iter().any(|unit| *unit == 0 || *unit == BACKSLASH_U16) {
        return STATUS_OBJECT_NAME_INVALID;
    }
    let _operation = REGISTRY_OPERATIONS.lock();
    let table = KEY_HANDLE_TABLE.lock();
    let Some(entry) = (handle as usize)
        .checked_sub(1)
        .and_then(|slot| table.get(slot))
        .and_then(Option::as_ref)
    else {
        return STATUS_INVALID_HANDLE;
    };
    if entry.desired_access & DELETE_RIGHT == 0 {
        return STATUS_ACCESS_DENIED;
    }
    // SAFETY: REGISTRY_OPERATIONS excludes deletion and tree mutation, and the
    // handle-table guard pins this entry through collision checking and commit.
    let Some(key) = (unsafe { entry.key_ptr.as_mut() }) else {
        return STATUS_INVALID_HANDLE;
    };
    let key_pointer = entry.key_ptr;
    // SAFETY: the operation lock retains the parent as well as the key.
    // A root has no parent namespace in which it could be renamed.
    let Some(parent) = (unsafe { key.parent.as_ref() }) else {
        return 0xc0000121u32 as i32;
    };
    let fold = |unit: u16| {
        if (65..=90).contains(&unit) {
            unit + 32
        } else {
            unit
        }
    };
    for child in parent.children.iter().flatten() {
        if *child == entry.key_ptr {
            continue;
        }
        // SAFETY: all sibling pointers remain live under the operation lock.
        let sibling = unsafe { &**child };
        if sibling.name_len as usize == name.len()
            && sibling.name[..name.len()]
                .iter()
                .zip(&name)
                .all(|(a, b)| fold(*a) == fold(*b))
        {
            return STATUS_OBJECT_NAME_COLLISION;
        }
    }
    key.name.fill(0);
    key.name[..name.len()].copy_from_slice(&name);
    key.name_len = name.len() as u16;
    key.last_write_time = crate::ke::time::get_system_time() as i64;
    drop(table);
    notify_registry_change(key_pointer);
    STATUS_SUCCESS
}

/// KeyWriteTimeInformation is the signed 64-bit NT timestamp, not a host time.
/// Other classes need registry virtualization/hive support and are rejected.
/// # Safety
/// `input` designates `length` caller bytes; no pointer survives capture.
pub unsafe extern "C" fn nt_set_information_key(
    handle: HANDLE,
    class: u32,
    input: PVOID,
    length: u32,
) -> NTSTATUS {
    if class != 0 {
        return if class <= 4 {
            STATUS_NOT_SUPPORTED
        } else {
            STATUS_INVALID_INFO_CLASS
        };
    }
    if length != 8 {
        return STATUS_INFO_LENGTH_MISMATCH;
    }
    let value = match read_user_object::<i64>(input as u64) {
        Ok(value) => value,
        Err(status) => return status,
    };
    let _operation = REGISTRY_OPERATIONS.lock();
    let table = KEY_HANDLE_TABLE.lock();
    let Some(entry) = (handle as usize)
        .checked_sub(1)
        .and_then(|slot| table.get(slot))
        .and_then(Option::as_ref)
    else {
        return STATUS_INVALID_HANDLE;
    };
    if entry.desired_access & KEY_SET_VALUE == 0 {
        return STATUS_ACCESS_DENIED;
    }
    // SAFETY: native readers/deletion are excluded by the operation lock.
    let Some(key) = (unsafe { entry.key_ptr.as_mut() }) else {
        return STATUS_INVALID_HANDLE;
    };
    key.last_write_time = value;
    STATUS_SUCCESS
}
