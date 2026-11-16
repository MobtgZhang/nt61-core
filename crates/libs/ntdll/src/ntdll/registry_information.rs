//! Serialize NT registry information without borrowing caller memory as a C
//! struct. This also preserves byte-sized truncation on every supported ISA.
use super::*;
use crate::libs::ntdll::status::STATUS_BUFFER_OVERFLOW;
use crate::mm::user_copy::copy_to_user;
use alloc::vec::Vec;

fn word(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
fn name(bytes: &mut [u8], offset: usize, units: &[u16]) {
    for (pair, unit) in bytes[offset..].chunks_exact_mut(2).zip(units) {
        pair.copy_from_slice(&unit.to_le_bytes());
    }
}
fn output(bytes: &[u8], header: usize, address: PVOID, length: u32, result: *mut u32) -> NTSTATUS {
    if !write_result_length(result, bytes.len()) {
        return STATUS_ACCESS_VIOLATION;
    }
    if (length as usize) < header {
        return STATUS_BUFFER_TOO_SMALL;
    }
    let count = bytes.len().min(length as usize);
    if let Err(status) = copy_to_user(address as u64, &bytes[..count]) {
        return status;
    }
    if count < bytes.len() {
        STATUS_BUFFER_OVERFLOW
    } else {
        STATUS_SUCCESS
    }
}

/// Capture the full live namespace path, including any renamed ancestors.
/// # Safety
/// The operation lock must keep `key` and its parent chain alive and immutable.
pub(super) unsafe fn key_name_information(
    key: &RegistryKey,
    address: PVOID,
    length: u32,
    result: *mut u32,
) -> NTSTATUS {
    use crate::libs::ntdll::status::{STATUS_NAME_TOO_LONG, STATUS_NO_MEMORY};
    let mut components = Vec::new();
    let mut current = Some(key);
    let mut size = 4usize;
    while let Some(node) = current {
        if components.len() == 1024 {
            return STATUS_NAME_TOO_LONG;
        }
        if components.try_reserve(1).is_err() {
            return STATUS_NO_MEMORY;
        }
        size += node.name_len as usize * 2;
        if !node.parent.is_null() {
            size += 2;
        }
        components.push(node);
        // SAFETY: the operation lock pins every node in this parent chain.
        current = unsafe { node.parent.as_ref() };
    }
    let mut bytes = Vec::new();
    if bytes.try_reserve_exact(size).is_err() {
        return STATUS_NO_MEMORY;
    }
    bytes.resize(size, 0);
    word(&mut bytes, 0, (size - 4) as u32);
    let mut offset = 4;
    for node in components.into_iter().rev() {
        if offset != 4 {
            name(&mut bytes, offset, &[BACKSLASH_U16]);
            offset += 2;
        }
        name(&mut bytes, offset, &node.name[..node.name_len as usize]);
        offset += node.name_len as usize * 2;
    }
    output(&bytes, 4, address, length, result)
}

/// # Safety
/// The registry operation lock must pin the key and all child pointers.
pub(super) unsafe fn key_information(
    key: &RegistryKey,
    class: u32,
    address: PVOID,
    length: u32,
    result: *mut u32,
    cached: bool,
) -> NTSTATUS {
    let mut bytes = [0u8; 24 + 512];
    bytes[..8].copy_from_slice(&key.last_write_time.to_le_bytes());
    let name_bytes = key.name_len as usize * 2;
    let (header, size) = match class {
        0 => {
            word(&mut bytes, 12, name_bytes as u32);
            name(&mut bytes, 16, &key.name[..key.name_len as usize]);
            (16, 16 + name_bytes)
        }
        1 => {
            word(&mut bytes, 12, u32::MAX); // no key class is installed
            word(&mut bytes, 20, name_bytes as u32);
            name(&mut bytes, 24, &key.name[..key.name_len as usize]);
            (24, 24 + name_bytes)
        }
        2 | 4 if class == 2 || cached => {
            let max_name = key
                .children
                .iter()
                .flatten()
                .map(|child| {
                    // SAFETY: all child nodes remain live under the operation lock.
                    unsafe { (**child).name_len as u32 * 2 }
                })
                .max()
                .unwrap_or(0);
            let max_value_name = key
                .values
                .iter()
                .flatten()
                .map(|value| value.name_len as u32 * 2)
                .max()
                .unwrap_or(0);
            let max_data = key
                .values
                .iter()
                .flatten()
                .map(|value| value.data_size)
                .max()
                .unwrap_or(0);
            if class == 2 {
                word(&mut bytes, 12, u32::MAX);
                word(&mut bytes, 20, key.child_count as u32);
                word(&mut bytes, 24, max_name);
                word(&mut bytes, 32, key.value_count as u32);
                word(&mut bytes, 36, max_value_name);
                word(&mut bytes, 40, max_data);
                (44, 44)
            } else {
                word(&mut bytes, 12, key.child_count as u32);
                word(&mut bytes, 16, max_name);
                word(&mut bytes, 20, key.value_count as u32);
                word(&mut bytes, 24, max_value_name);
                word(&mut bytes, 28, max_data);
                word(&mut bytes, 32, name_bytes as u32);
                (36, 36)
            }
        }
        _ => return STATUS_INVALID_PARAMETER,
    };
    output(&bytes[..size], header, address, length, result)
}

pub(super) fn value_information(
    stored_name: &[u16],
    kind: ValueType,
    data: &[u8],
    class: u32,
    address: PVOID,
    length: u32,
    result: *mut u32,
) -> NTSTATUS {
    let mut bytes = [0u8; 20 + MAX_VALUE_NAME_LEN * 2 + 7 + MAX_VALUE_DATA_SIZE];
    word(&mut bytes, 4, kind.0);
    let name_size = stored_name.len() * 2;
    let (header, size) = match class {
        0 => {
            word(&mut bytes, 8, name_size as u32);
            name(&mut bytes, 12, stored_name);
            (12, 12 + name_size)
        }
        1 | 3 => {
            let alignment = if class == 3 { 8 } else { 4 };
            let offset = (20 + name_size + alignment - 1) & !(alignment - 1);
            word(&mut bytes, 8, offset as u32);
            word(&mut bytes, 12, data.len() as u32);
            word(&mut bytes, 16, name_size as u32);
            name(&mut bytes, 20, stored_name);
            bytes[offset..offset + data.len()].copy_from_slice(data);
            (20, offset + data.len())
        }
        2 => {
            word(&mut bytes, 8, data.len() as u32);
            bytes[12..12 + data.len()].copy_from_slice(data);
            (12, 12 + data.len())
        }
        4 => {
            // KEY_VALUE_PARTIAL_INFORMATION_ALIGN64 omits TitleIndex.
            word(&mut bytes, 0, kind.0);
            word(&mut bytes, 4, data.len() as u32);
            bytes[8..8 + data.len()].copy_from_slice(data);
            (8, 8 + data.len())
        }
        _ => return STATUS_INVALID_PARAMETER,
    };
    output(&bytes[..size], header, address, length, result)
}
