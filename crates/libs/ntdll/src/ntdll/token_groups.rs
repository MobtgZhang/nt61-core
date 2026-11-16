//! Captured NT64 group adjustment, shared by all four syscall dispatchers.

use super::file::{with_sync_access, HandleKind};
use super::status::*;
use super::token_information::encode_groups;
use super::types::{HANDLE, NTSTATUS};
use crate::mm::user_copy::{
    copy_from_user, copy_to_user, probe_user_read, probe_user_write, read_user_object,
    write_user_object,
};
use crate::se::sid::sid;
use crate::se::token::{GroupAdjustmentError, Token, TokenGroup, TOKEN_MAX_GROUPS};
use alloc::vec::Vec;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct SidAndAttributes {
    pub sid: *const u8,
    pub attributes: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct TOKEN_GROUPS {
    pub group_count: u32,
    pub groups: [SidAndAttributes; 1],
}

const STATUS_DATATYPE_MISALIGNMENT: NTSTATUS = 0x8000_0002u32 as i32;

pub(crate) struct Request {
    // Windows permits 15 SID subauthorities. A valid SID longer than the
    // kernel's current token storage is simply absent, rather than invalid.
    pub(crate) sid: [u8; 68],
    pub(crate) attributes: u32,
}

impl Request {
    pub(crate) fn matches(&self, value: &sid) -> bool {
        self.sid[0] == value.revision
            && self.sid[1] == value.sub_authority_count
            && self.sid[2..8] == value.identifier_authority
            && value
                .sub_authority
                .iter()
                .take(value.sub_authority_count as usize)
                .zip(self.sid[8..].chunks_exact(4))
                .all(|(authority, bytes)| authority.to_le_bytes() == bytes)
    }
}

pub(crate) fn capture(new_state: *const TOKEN_GROUPS) -> Result<Vec<Request>, NTSTATUS> {
    if new_state.is_null() {
        return Err(STATUS_INVALID_PARAMETER);
    }
    if new_state as usize % 4 != 0 {
        return Err(STATUS_DATATYPE_MISALIGNMENT);
    }
    let count: u32 = read_user_object(new_state as u64)?;
    let length = (count as usize)
        .checked_mul(16)
        .and_then(|n| n.checked_add(8))
        .ok_or(STATUS_INVALID_PARAMETER)?;
    probe_user_read(new_state as u64, length)?;
    let mut requests = Vec::new();
    requests
        .try_reserve_exact(count as usize)
        .map_err(|_| STATUS_NO_MEMORY)?;
    for index in 0..count as u64 {
        let mut entry = [0; 16];
        let address = (new_state as u64)
            .checked_add(8 + index * 16)
            .ok_or(STATUS_ACCESS_VIOLATION)?;
        copy_from_user(&mut entry, address)?;
        let sid_address = u64::from_le_bytes(entry[..8].try_into().unwrap());
        if sid_address % 4 != 0 {
            return Err(STATUS_DATATYPE_MISALIGNMENT);
        }
        let mut header = [0; 8];
        copy_from_user(&mut header, sid_address)?;
        if header[0] != 1 || header[1] > 15 {
            return Err(STATUS_INVALID_SID);
        }
        let length = 8 + header[1] as usize * 4;
        let mut request = Request {
            sid: [0; 68],
            attributes: u32::from_le_bytes(entry[8..12].try_into().unwrap()),
        };
        copy_from_user(&mut request.sid[..length], sid_address)?;
        if request.sid[0] != 1 || request.sid[1] != header[1] {
            return Err(STATUS_INVALID_SID);
        }
        requests.push(request);
    }
    Ok(requests)
}

struct Adjustment {
    attributes: [u32; TOKEN_MAX_GROUPS],
    previous: [TokenGroup; TOKEN_MAX_GROUPS],
    changed: usize,
    missing: bool,
}

fn plan(token: &Token, reset: bool, requests: &[Request]) -> Result<Adjustment, NTSTATUS> {
    let mut adjustment = Adjustment {
        attributes: [0; TOKEN_MAX_GROUPS],
        previous: [TokenGroup::default(); TOKEN_MAX_GROUPS],
        changed: 0,
        missing: false,
    };
    for (index, group) in token.groups[..token.group_count].iter().enumerate() {
        let request = requests.iter().find(|request| request.matches(&group.sid));
        let attributes = group
            .adjusted_attributes(reset, request.map(|r| r.attributes))
            .map_err(|error| match error {
                GroupAdjustmentError::CannotDisableMandatory => STATUS_CANT_DISABLE_MANDATORY,
                GroupAdjustmentError::CannotEnableDenyOnly => STATUS_CANT_ENABLE_DENY_ONLY,
            })?;
        adjustment.attributes[index] = attributes;
        if attributes != group.attributes {
            adjustment.previous[adjustment.changed] = *group;
            adjustment.changed += 1;
        }
    }
    adjustment.missing = !reset
        && requests.iter().any(|request| {
            !token.groups[..token.group_count]
                .iter()
                .any(|group| request.matches(&group.sid))
        });
    Ok(adjustment)
}

/// # Safety
/// NewState and its SIDs name readable caller memory. Optional PreviousState
/// and ReturnLength name writable caller memory. No caller references survive
/// capture, and no token mutation occurs before output validation succeeds.
pub unsafe extern "C" fn nt_adjust_groups_token(
    handle: HANDLE,
    reset_to_default: u8,
    new_state: *const TOKEN_GROUPS,
    buffer_length: u32,
    previous_state: *mut TOKEN_GROUPS,
    return_length: *mut u32,
) -> NTSTATUS {
    let operation = || -> Result<NTSTATUS, NTSTATUS> {
        let reset = reset_to_default != 0;
        let requests = if reset {
            Vec::new()
        } else {
            capture(new_state)?
        };
        if !previous_state.is_null() {
            if (buffer_length != 0 && previous_state as usize % 4 != 0)
                || return_length as usize % 4 != 0
            {
                return Err(STATUS_DATATYPE_MISALIGNMENT);
            }
            probe_user_write(previous_state as u64, buffer_length as usize)?;
            probe_user_write(return_length as u64, 4)?;
        }
        with_sync_access(
            handle,
            0x40 | if previous_state.is_null() { 0 } else { 8 },
            |entries| {
                if entries[0].kind != HandleKind::Token {
                    return Err(STATUS_OBJECT_TYPE_MISMATCH);
                }
                // SAFETY: the handle transaction pins and serializes this payload.
                let token = unsafe { (entries[0].target as *mut Token).as_mut() }
                    .ok_or(STATUS_INVALID_HANDLE)?;
                let adjustment = plan(token, reset, &requests)?;
                if !previous_state.is_null() {
                    let bytes = encode_groups(
                        adjustment.previous[..adjustment.changed].iter(),
                        previous_state as u64,
                    )?;
                    write_user_object(return_length as u64, &(bytes.len() as u32))?;
                    if (buffer_length as usize) < bytes.len() {
                        return Err(STATUS_BUFFER_TOO_SMALL);
                    }
                    copy_to_user(previous_state as u64, &bytes)?;
                }
                for (group, attributes) in token.groups[..token.group_count]
                    .iter_mut()
                    .zip(adjustment.attributes)
                {
                    group.attributes = attributes;
                }
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
