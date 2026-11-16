//! Captured multi-value queries and open-subkey accounting.
use super::*;
use crate::mm::user_copy::{copy_to_user, probe_user_write};
use alloc::vec::Vec;

/// # Safety
/// Arguments point to caller memory. Only the NT 6.1 open-link option exists.
pub unsafe extern "C" fn nt_open_key_ex(
    output: *mut HANDLE,
    access: u32,
    attributes: *mut ObjectAttributes,
    options: u32,
) -> NTSTATUS {
    if options & !8 != 0 {
        return super::super::status::STATUS_INVALID_PARAMETER;
    }
    if let Err(status) = probe_user_write(output as u64, core::mem::size_of::<HANDLE>()) {
        return status;
    }
    unsafe { nt_open_key(output, access, attributes) }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct ValueEntry {
    name: u64,
    length: u32,
    offset: u32,
    kind: u32,
    padding: u32,
}

/// # Safety
/// Entries and result buffers are caller addresses and are captured first.
pub unsafe extern "C" fn nt_query_multiple_value_key(
    handle: HANDLE,
    entries: *mut u8,
    count: u32,
    output: *mut u8,
    buffer_length: *mut u32,
    required_length: *mut u32,
) -> NTSTATUS {
    let _operation = REGISTRY_OPERATIONS.lock();
    use super::super::status::*;
    (|| {
        if count > 65536 {
            return Err(STATUS_INVALID_PARAMETER);
        }
        let capacity = read_user_object::<u32>(buffer_length as u64)? as usize;
        probe_user_write(buffer_length as u64, 4)?;
        if !required_length.is_null() {
            probe_user_write(required_length as u64, 4)?;
        }
        let mut captured = Vec::new();
        captured
            .try_reserve_exact(count as usize)
            .map_err(|_| STATUS_NO_MEMORY)?;
        let mut names = Vec::new();
        names
            .try_reserve_exact(count as usize)
            .map_err(|_| STATUS_NO_MEMORY)?;
        for index in 0..count as u64 {
            let address = (entries as u64)
                .checked_add(index * 24)
                .ok_or(STATUS_ACCESS_VIOLATION)?;
            let entry = read_user_object::<ValueEntry>(address)?;
            names.push(capture_value_name(entry.name)?);
            captured.push(entry);
        }
        let mut bytes = Vec::new();
        {
            let table = KEY_HANDLE_TABLE.lock();
            let slot = (handle as usize)
                .checked_sub(1)
                .ok_or(STATUS_INVALID_HANDLE)?;
            let entry = table
                .get(slot)
                .and_then(Option::as_ref)
                .ok_or(STATUS_INVALID_HANDLE)?;
            if entry.desired_access & 1 == 0 {
                return Err(STATUS_ACCESS_DENIED);
            }
            // The handle guard retains the key while values are captured.
            let key = unsafe { entry.key_ptr.as_ref() }.ok_or(STATUS_INVALID_HANDLE)?;
            for (entry, name) in captured.iter_mut().zip(&names) {
                let value = key
                    .values
                    .iter()
                    .flatten()
                    .find(|value| {
                        let stored = &value.name[..value.name_len as usize];
                        stored.len() == name.len()
                            && stored
                                .iter()
                                .zip(name)
                                .all(|(a, b)| ascii_lower(*a) == ascii_lower(*b))
                    })
                    .ok_or(STATUS_OBJECT_NAME_NOT_FOUND)?;
                let aligned = bytes.len().checked_add(3).ok_or(STATUS_NO_MEMORY)? & !3;
                let end = aligned
                    .checked_add(value.data_size as usize)
                    .ok_or(STATUS_NO_MEMORY)?;
                bytes
                    .try_reserve(end - bytes.len())
                    .map_err(|_| STATUS_NO_MEMORY)?;
                bytes.resize(aligned, 0);
                bytes.extend_from_slice(&value.data[..value.data_size as usize]);
                entry.offset = aligned as u32;
                entry.length = value.data_size as u32;
                entry.kind = value.value_type.0;
                entry.padding = 0;
            }
        }
        let required = u32::try_from(bytes.len()).map_err(|_| STATUS_NO_MEMORY)?;
        if !required_length.is_null() {
            write_user_object(required_length as u64, &required)?;
        }
        write_user_object(buffer_length as u64, &required)?;
        if capacity < bytes.len() {
            return Err(STATUS_BUFFER_TOO_SMALL);
        }
        if !bytes.is_empty() {
            copy_to_user(output as u64, &bytes)?;
        }
        for (index, entry) in captured.iter().enumerate() {
            write_user_object(entries as u64 + index as u64 * 24, entry)?;
        }
        Ok(STATUS_SUCCESS)
    })()
    .unwrap_or_else(|status| status)
}

/// # Safety
/// Object attributes and the count point to caller memory.
pub unsafe extern "C" fn nt_query_open_sub_keys(
    attributes: *mut ObjectAttributes,
    output: *mut u32,
) -> NTSTATUS {
    let _operation = REGISTRY_OPERATIONS.lock();
    use super::super::status::*;
    (|| {
        probe_user_write(output as u64, 4)?;
        let attributes = read_user_object::<ObjectAttributes>(attributes as u64)?;
        if attributes.length as usize != core::mem::size_of::<ObjectAttributes>() {
            return Err(STATUS_INVALID_PARAMETER);
        }
        if !attributes.root_directory.is_null() {
            return Err(STATUS_NOT_SUPPORTED);
        }
        let path = read_user_unicode_string(attributes.object_name as u64, MAX_VALUE_NAME_LEN * 2)?;
        let root = REGISTRY_ROOT.lock();
        let mut key = root.ok_or(STATUS_OBJECT_NAME_NOT_FOUND)?;
        for part in path
            .split(|unit| *unit == 92)
            .filter(|part| !part.is_empty())
        {
            key = unsafe { find_subkey(key, part) }.ok_or(STATUS_OBJECT_NAME_NOT_FOUND)?;
        }
        if !registry_access_allowed(key, 8) {
            return Err(STATUS_ACCESS_DENIED);
        }
        let table = KEY_HANDLE_TABLE.lock();
        let mut count = 0u32;
        for entry in table.iter().flatten() {
            let mut current = entry.key_ptr;
            // Registry depth is bounded during traversal even for corrupt links.
            for _ in 0..1024 {
                if current == key {
                    count += 1;
                    break;
                }
                let Some(parent) = (unsafe { current.as_ref() }) else {
                    break;
                };
                current = parent.parent;
            }
        }
        drop(table);
        drop(root);
        write_user_object(output as u64, &count)?;
        Ok(STATUS_SUCCESS)
    })()
    .unwrap_or_else(|status| status)
}

fn ascii_lower(unit: u16) -> u16 {
    if (65..=90).contains(&unit) {
        unit + 32
    } else {
        unit
    }
}
