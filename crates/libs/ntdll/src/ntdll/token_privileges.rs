//! Captured, variable-length NT privilege operations.

use super::file::{with_sync_access, HandleKind};
use super::status::*;
use super::types::{HANDLE, NTSTATUS};
use crate::mm::user_copy::{
    copy_from_user, copy_to_user, probe_user_write, read_user_object, write_user_object,
};
use crate::se::token::{
    self, ImpersonationLevel, Token, TokenPrivilege, TokenTypeEnum, TOKEN_MAX_PRIVILEGES,
};
use alloc::vec::Vec;

// Windows LUID is two 32-bit fields, with 4-byte alignment. Representing it as
// an ordinary u64 in a repr(C) struct would insert incompatible padding on all
// four 64-bit ports. Never form references to the packed luid field.
#[repr(C, packed(4))]
#[derive(Clone, Copy, Default)]
pub struct LuidAndAttributes {
    pub luid: u64,
    pub attributes: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct TOKEN_PRIVILEGES {
    pub privilege_count: u32,
    pub privileges: [LuidAndAttributes; 1],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PrivilegeSet {
    pub privilege_count: u32,
    pub control: u32,
    pub privilege: [LuidAndAttributes; 1],
}

fn capture(address: u64, count: u32) -> Result<Vec<LuidAndAttributes>, NTSTATUS> {
    let length = (count as usize)
        .checked_mul(12)
        .ok_or(STATUS_INVALID_PARAMETER)?;
    crate::mm::user_copy::probe_user_read(address, length)?;
    let mut entries = Vec::new();
    entries
        .try_reserve_exact(count as usize)
        .map_err(|_| STATUS_NO_MEMORY)?;
    for index in 0..count as u64 {
        let mut bytes = [0; 12];
        copy_from_user(
            &mut bytes,
            address
                .checked_add(index * 12)
                .ok_or(STATUS_ACCESS_VIOLATION)?,
        )?;
        entries.push(LuidAndAttributes {
            luid: u64::from_le_bytes(bytes[..8].try_into().unwrap()),
            attributes: u32::from_le_bytes(bytes[8..].try_into().unwrap()),
        });
    }
    Ok(entries)
}

fn encode(entries: &[LuidAndAttributes], header: usize) -> Vec<u8> {
    // At most TOKEN_MAX_PRIVILEGES entries, bounded by the kernel token model.
    let mut output = alloc::vec![0; header + entries.len() * 12];
    output[..4].copy_from_slice(&(entries.len() as u32).to_le_bytes());
    for (entry, bytes) in entries.iter().zip(output[header..].chunks_exact_mut(12)) {
        bytes[..8].copy_from_slice(&entry.luid.to_le_bytes());
        bytes[8..].copy_from_slice(&entry.attributes.to_le_bytes());
    }
    output
}

fn check(token: &Token, control: u32, entries: &mut [LuidAndAttributes]) -> Result<bool, NTSTATUS> {
    if token.token_type == TokenTypeEnum::TokenImpersonation
        && token.impersonation_level == ImpersonationLevel::SecurityAnonymous
    {
        return Err(STATUS_BAD_IMPERSONATION_LEVEL);
    }
    let all = control & 1 != 0;
    let mut matched = 0;
    for entry in entries.iter_mut() {
        if token.has_privilege(&token::luid::from_u64(entry.luid)) {
            entry.attributes |= token::SE_PRIVILEGE_USED_FOR_ACCESS;
            matched += 1;
        }
    }
    Ok(if all {
        matched == entries.len()
    } else {
        matched != 0
    })
}

/// Capture and evaluate the caller's variable-length PRIVILEGE_SET.
///
/// The returned value is independent of BOOLEAN/BOOL output encoding. Both NT
/// and Win32 entry points use this path, including attribute writeback.
pub fn check_privileges_for_caller(
    handle: HANDLE,
    required: *const PrivilegeSet,
) -> Result<bool, NTSTATUS> {
    if required as usize % 4 != 0 {
        return Err(0x8000_0002u32 as i32); // STATUS_DATATYPE_MISALIGNMENT
    }
    let count: u32 = read_user_object(required as u64)?;
    let control: u32 = read_user_object(
        (required as u64)
            .checked_add(4)
            .ok_or(STATUS_ACCESS_VIOLATION)?,
    )?;
    let entries_address = (required as u64)
        .checked_add(8)
        .ok_or(STATUS_ACCESS_VIOLATION)?;
    let mut entries = capture(entries_address, count)?;
    probe_user_write(required as u64, 8 + entries.len() * 12)?;
    let value = with_sync_access(handle, 8, |values| {
        if values[0].kind != HandleKind::Token {
            return Err(STATUS_OBJECT_TYPE_MISMATCH);
        }
        // SAFETY: the table lock pins and serializes the token payload.
        let token =
            unsafe { (values[0].target as *const Token).as_ref() }.ok_or(STATUS_INVALID_HANDLE)?;
        check(token, control, &mut entries)
    })?;
    for (index, entry) in entries.iter().enumerate() {
        write_user_object(entries_address + index as u64 * 12 + 8, &entry.attributes)?;
    }
    Ok(value)
}

/// # Safety
/// RequiredPrivileges is an in/out variable-length PRIVILEGE_SET in caller memory.
pub unsafe extern "C" fn nt_privilege_check(
    handle: HANDLE,
    required: *const PrivilegeSet,
    result: *mut u8,
) -> NTSTATUS {
    let operation = || -> Result<(), NTSTATUS> {
        probe_user_write(result as u64, 1)?;
        let value = check_privileges_for_caller(handle, required)?;
        write_user_object(result as u64, &u8::from(value))
    };
    operation().map_or_else(|status| status, |()| STATUS_SUCCESS)
}

pub fn nt_privilege_check_kernel(
    handle: HANDLE,
    required: Option<&PrivilegeSet>,
    result: &mut u8,
) -> NTSTATUS {
    let Some(required) = required else {
        return STATUS_INVALID_PARAMETER;
    };
    if required.privilege_count > 1 {
        return STATUS_INVALID_PARAMETER;
    }
    let mut entries = required.privilege;
    with_sync_access(handle, 8, |values| {
        if values[0].kind != HandleKind::Token {
            return Err(STATUS_OBJECT_TYPE_MISMATCH);
        }
        let token =
            unsafe { (values[0].target as *const Token).as_ref() }.ok_or(STATUS_INVALID_HANDLE)?;
        *result = u8::from(check(
            token,
            required.control,
            &mut entries[..required.privilege_count as usize],
        )?);
        Ok(STATUS_SUCCESS)
    })
    .unwrap_or_else(|status| status)
}

struct Adjustment {
    privileges: [TokenPrivilege; TOKEN_MAX_PRIVILEGES],
    count: usize,
    previous: [LuidAndAttributes; TOKEN_MAX_PRIVILEGES],
    changed: usize,
    missing: bool,
}

fn plan(token: &Token, disable_all: bool, requested: &[LuidAndAttributes]) -> Adjustment {
    let mut adjustment = Adjustment {
        privileges: [TokenPrivilege::default(); TOKEN_MAX_PRIVILEGES],
        count: 0,
        previous: [LuidAndAttributes::default(); TOKEN_MAX_PRIVILEGES],
        changed: 0,
        missing: false,
    };
    let mut assigned = 0;
    for privilege in token.privileges.iter().take(token.privilege_count) {
        let request = requested
            .iter()
            .find(|entry| entry.luid == privilege.luid.to_u64());
        assigned += usize::from(request.is_some());
        let Some(attributes) =
            privilege.adjusted_attributes(disable_all, request.map(|entry| entry.attributes))
        else {
            continue;
        };
        if attributes != privilege.attributes {
            adjustment.previous[adjustment.changed] = LuidAndAttributes {
                luid: privilege.luid.to_u64(),
                attributes: privilege.attributes,
            };
            adjustment.changed += 1;
        }
        adjustment.privileges[adjustment.count] = TokenPrivilege {
            luid: privilege.luid,
            attributes,
        };
        adjustment.count += 1;
    }
    // Repeated requests for the same LUID do not name additional privileges.
    if !disable_all && assigned < requested.len() {
        adjustment.missing = requested.iter().any(|entry| {
            !token.privileges[..token.privilege_count]
                .iter()
                .any(|privilege| privilege.luid.to_u64() == entry.luid)
        });
    }
    adjustment
}

fn commit(token: &mut Token, adjustment: &Adjustment) {
    token.privileges = adjustment.privileges;
    token.privilege_count = adjustment.count;
    token.refresh_privilege_flags();
}

/// # Safety
/// User buffers are captured before changes; previous state is written through
/// user-copy, never by constructing Rust references into caller memory.
pub unsafe extern "C" fn nt_adjust_privileges_token(
    handle: HANDLE,
    disable_all: u8,
    new_state: *const TOKEN_PRIVILEGES,
    buffer_length: u32,
    previous: *mut TOKEN_PRIVILEGES,
    return_length: *mut u32,
) -> NTSTATUS {
    let operation = || -> Result<NTSTATUS, NTSTATUS> {
        let requested = if disable_all != 0 {
            Vec::new()
        } else {
            let count: u32 = read_user_object(new_state as u64)?;
            capture(
                (new_state as u64)
                    .checked_add(4)
                    .ok_or(STATUS_ACCESS_VIOLATION)?,
                count,
            )?
        };
        if !previous.is_null() {
            probe_user_write(previous as u64, buffer_length as usize)?;
            probe_user_write(return_length as u64, 4)?;
        }
        with_sync_access(
            handle,
            0x20 | if previous.is_null() { 0 } else { 8 },
            |values| {
                if values[0].kind != HandleKind::Token {
                    return Err(STATUS_OBJECT_TYPE_MISMATCH);
                }
                let token = unsafe { (values[0].target as *mut Token).as_mut() }
                    .ok_or(STATUS_INVALID_HANDLE)?;
                let adjustment = plan(token, disable_all != 0, &requested);
                if !previous.is_null() {
                    let bytes = encode(&adjustment.previous[..adjustment.changed], 4);
                    write_user_object(return_length as u64, &(bytes.len() as u32))?;
                    if (buffer_length as usize) < bytes.len() {
                        return Err(STATUS_BUFFER_TOO_SMALL);
                    }
                    copy_to_user(previous as u64, &bytes)?;
                }
                commit(token, &adjustment);
                Ok(if adjustment.missing {
                    STATUS_NOT_ALL_ASSIGNED
                } else {
                    STATUS_SUCCESS
                })
            },
        )
    };
    operation().unwrap_or_else(|status| status)
}

/// The trusted fixed-size helper accepts at most its one inline entry. Native
/// callers with variable arrays use the captured syscall entry above.
pub fn nt_adjust_privileges_token_kernel(
    handle: HANDLE,
    disable_all: u8,
    requested: Option<&TOKEN_PRIVILEGES>,
    previous: Option<&mut TOKEN_PRIVILEGES>,
    return_length: Option<&mut u32>,
) -> NTSTATUS {
    let requested = match requested {
        Some(value) if disable_all == 0 && value.privilege_count <= 1 => {
            &value.privileges[..value.privilege_count as usize]
        }
        _ if disable_all != 0 => &[],
        _ => return STATUS_INVALID_PARAMETER,
    };
    with_sync_access(
        handle,
        0x20 | if previous.is_some() { 8 } else { 0 },
        |values| {
            if values[0].kind != HandleKind::Token {
                return Err(STATUS_OBJECT_TYPE_MISMATCH);
            }
            let token = unsafe { (values[0].target as *mut Token).as_mut() }
                .ok_or(STATUS_INVALID_HANDLE)?;
            let adjustment = plan(token, disable_all != 0, requested);
            if let Some(previous) = previous {
                if let Some(length) = return_length {
                    *length = 4 + adjustment.changed as u32 * 12;
                }
                if adjustment.changed > 1 {
                    return Err(STATUS_BUFFER_TOO_SMALL);
                }
                previous.privilege_count = adjustment.changed as u32;
                if adjustment.changed == 1 {
                    previous.privileges[0] = adjustment.previous[0];
                }
            }
            commit(token, &adjustment);
            Ok(if adjustment.missing {
                STATUS_NOT_ALL_ASSIGNED
            } else {
                STATUS_SUCCESS
            })
        },
    )
    .unwrap_or_else(|status| status)
}
