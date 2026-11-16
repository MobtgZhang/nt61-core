//! Native token information with captured buffers and caller-relative pointers.
//!
//! NT 6.1 uses 8-byte pointers but 4-byte LUID alignment on its 64-bit ABI.
//! Encode fields explicitly so neither Rust padding nor kernel addresses escape
//! into the caller's result. The four 64-bit ports share this implementation.

use super::file::{with_sync_access, HandleKind};
use super::status::*;
use super::types::{HANDLE, NTSTATUS, PVOID};
use crate::mm::user_copy::{copy_from_user, copy_to_user, probe_user_write, write_user_object};
use crate::se::sid::sid;
use crate::se::token::{self, Token, TokenGroup, TokenTypeEnum};
use alloc::vec::Vec;

const STATUS_DATATYPE_MISALIGNMENT: NTSTATUS = 0x8000_0002u32 as i32;
const STATUS_INVALID_PRIMARY_GROUP: NTSTATUS = 0xc000_005bu32 as i32;

fn query_class(class: u32) -> Result<(), NTSTATUS> {
    match class {
        1..=13 | 15 | 17..=28 => Ok(()),
        _ => Err(STATUS_INVALID_INFO_CLASS),
    }
}

fn bytes(length: usize) -> Result<Vec<u8>, NTSTATUS> {
    let mut output = Vec::new();
    output
        .try_reserve_exact(length)
        .map_err(|_| STATUS_NO_MEMORY)?;
    output.resize(length, 0);
    Ok(output)
}

fn put_u32(output: &mut [u8], offset: usize, value: u32) {
    output[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn pointer(output: &mut [u8], offset: usize, base: u64, target: usize) -> Result<(), NTSTATUS> {
    let address = base
        .checked_add(target as u64)
        .ok_or(STATUS_ACCESS_VIOLATION)?;
    output[offset..offset + 8].copy_from_slice(&address.to_le_bytes());
    Ok(())
}

fn sid_size(value: &sid) -> Result<usize, NTSTATUS> {
    if !value.is_valid() {
        return Err(STATUS_INVALID_PARAMETER);
    }
    Ok(value.size())
}

fn put_sid(output: &mut [u8], offset: usize, value: &sid) {
    output[offset] = value.revision;
    output[offset + 1] = value.sub_authority_count;
    output[offset + 2..offset + 8].copy_from_slice(&value.identifier_authority);
    for (index, authority) in value
        .sub_authority
        .iter()
        .take(value.sub_authority_count as usize)
        .enumerate()
    {
        put_u32(output, offset + 8 + index * 4, *authority);
    }
}

fn sid_information(value: &sid, attributes: Option<u32>, base: u64) -> Result<Vec<u8>, NTSTATUS> {
    let header = if attributes.is_some() { 16 } else { 8 };
    let mut output = bytes(header + sid_size(value)?)?;
    pointer(&mut output, 0, base, header)?;
    if let Some(attributes) = attributes {
        put_u32(&mut output, 8, attributes);
    }
    put_sid(&mut output, header, value);
    Ok(output)
}

fn groups(token: &Token, logon_only: bool, base: u64) -> Result<Vec<u8>, NTSTATUS> {
    let selected = token.groups[..token.group_count].iter().filter(|group| {
        !logon_only || group.attributes & token::SE_GROUP_LOGON_ID == token::SE_GROUP_LOGON_ID
    });
    encode_groups(selected, base)
}

pub(super) fn encode_groups<'a>(
    selected: impl Iterator<Item = &'a TokenGroup> + Clone,
    base: u64,
) -> Result<Vec<u8>, NTSTATUS> {
    let header = 8 + selected.clone().count() * 16;
    let mut length = header;
    for group in selected.clone() {
        length += sid_size(&group.sid)?;
    }
    let mut output = bytes(length)?;
    put_u32(&mut output, 0, selected.clone().count() as u32);
    let mut sid_offset = header;
    for (index, group) in selected.enumerate() {
        pointer(&mut output, 8 + index * 16, base, sid_offset)?;
        put_u32(&mut output, 16 + index * 16, group.attributes);
        put_sid(&mut output, sid_offset, &group.sid);
        sid_offset += group.sid.size();
    }
    Ok(output)
}

fn encode(token: &Token, class: u32, base: u64) -> Result<Vec<u8>, NTSTATUS> {
    match class {
        1 => sid_information(&token.user, Some(token.user_attributes), base),
        2 => groups(token, false, base),
        3 => {
            let mut output = bytes(4 + token.privilege_count * 12)?;
            put_u32(&mut output, 0, token.privilege_count as u32);
            for (index, privilege) in token.privileges[..token.privilege_count].iter().enumerate() {
                let offset = 4 + index * 12;
                output[offset..offset + 8].copy_from_slice(&privilege.luid.to_u64().to_le_bytes());
                put_u32(&mut output, offset + 8, privilege.attributes);
            }
            Ok(output)
        }
        11 => encode_groups(token.restricted_groups[..token.restricted_count].iter(), base),
        15 => Ok(u32::from(token.flags & token::TOKEN_SANDBOX_INERT != 0).to_le_bytes().to_vec()),
        21 => Ok(u32::from(token.flags & token::TOKEN_IS_FILTERED != 0).to_le_bytes().to_vec()),
        5 => sid_information(&token.primary_group, None, base),
        6 => {
            // SAFETY: token operations hold the handle-table lock. The token
            // owns the ACL and it cannot be replaced or destroyed while locked.
            let Some(acl) = (unsafe { token.default_dacl.as_ref() }) else {
                return bytes(8);
            };
            let length = acl.used_size().ok_or(STATUS_INVALID_PARAMETER)?;
            let mut output = bytes(8 + length)?;
            pointer(&mut output, 0, base, 8)?;
            output[8] = acl.revision;
            output[10..12].copy_from_slice(&(length as u16).to_le_bytes());
            output[12..14].copy_from_slice(&acl.ace_count.to_le_bytes());
            output[16..].copy_from_slice(&acl.data[8..length]);
            Ok(output)
        }
        8 | 9 | 12 => {
            let value = match class {
                8 => token.token_type as u32,
                9 if token.token_type == TokenTypeEnum::TokenPrimary => {
                    return Err(STATUS_INVALID_INFO_CLASS)
                }
                9 => token.impersonation_level as u32,
                _ => token.session_id,
            };
            Ok(value.to_le_bytes().to_vec())
        }
        25 => {
            let label = sid::with_subs(
                [0, 0, 0, 0, 0, 16],
                token.integrity_level_rid,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
            );
            sid_information(
                &label,
                Some(token::SE_GROUP_INTEGRITY | token::SE_GROUP_INTEGRITY_ENABLED),
                base,
            )
        }
        28 => groups(token, true, base),
        // Do not invent values for fields not yet represented by Token.
        _ => Err(STATUS_NOT_SUPPORTED),
    }
}

fn capture_information(handle: HANDLE, class: u32, base: u64) -> Result<Vec<u8>, NTSTATUS> {
    query_class(class)?;
    with_sync_access(handle, if class == 7 { 0x10 } else { 8 }, |entries| {
        if entries[0].kind != HandleKind::Token {
            return Err(STATUS_OBJECT_TYPE_MISMATCH);
        }
        // SAFETY: with_sync_access pins and serializes the token payload.
        let token =
            unsafe { (entries[0].target as *const Token).as_ref() }.ok_or(STATUS_INVALID_HANDLE)?;
        encode(token, class, base)
    })
}

/// # Safety
/// Outputs name caller memory; embedded pointers are relocated to that buffer.
pub unsafe extern "C" fn nt_query_information_token(
    handle: HANDLE,
    class: u32,
    information: PVOID,
    length: u32,
    return_length: *mut u32,
) -> NTSTATUS {
    let operation = || -> Result<(), NTSTATUS> {
        if return_length as usize % 4 != 0 || (length != 0 && information as usize % 4 != 0) {
            return Err(STATUS_DATATYPE_MISALIGNMENT);
        }
        probe_user_write(return_length as u64, 4)?;
        probe_user_write(information as u64, length as usize)?;
        let output = capture_information(handle, class, information as u64)?;
        write_user_object(return_length as u64, &(output.len() as u32))?;
        if (length as usize) < output.len() {
            return Err(STATUS_BUFFER_TOO_SMALL);
        }
        copy_to_user(information as u64, &output)
    };
    operation().map_or_else(|status| status, |()| STATUS_SUCCESS)
}

/// Trusted slice-based helper. Returned pointers address this slice itself.
pub fn nt_query_information_token_kernel(
    handle: HANDLE,
    class: u32,
    information: Option<&mut [u8]>,
    return_length: &mut u32,
) -> NTSTATUS {
    let base = information
        .as_ref()
        .map_or(0, |buffer| buffer.as_ptr() as u64);
    let output = match capture_information(handle, class, base) {
        Ok(output) => output,
        Err(status) => return status,
    };
    *return_length = output.len() as u32;
    let Some(information) = information.filter(|buffer| buffer.len() >= output.len()) else {
        return STATUS_BUFFER_TOO_SMALL;
    };
    information[..output.len()].copy_from_slice(&output);
    STATUS_SUCCESS
}

enum Change {
    PrimaryGroup(sid),
    Session(u32),
}

fn set_class(class: u32) -> Result<(), NTSTATUS> {
    match class {
        5 | 12 => Ok(()),
        4 | 6 | 14 | 16 | 17 | 19 | 23..=27 => Err(STATUS_NOT_SUPPORTED),
        _ => Err(STATUS_INVALID_INFO_CLASS),
    }
}

fn capture_change(class: u32, information: &[u8]) -> Result<Change, NTSTATUS> {
    set_class(class)?;
    if class == 12 {
        return Ok(Change::Session(u32::from_le_bytes(
            information
                .get(..4)
                .ok_or(STATUS_INFO_LENGTH_MISMATCH)?
                .try_into()
                .unwrap(),
        )));
    }
    // TOKEN_PRIMARY_GROUP contains a caller pointer, even for trusted callers.
    let address = u64::from_le_bytes(
        information
            .get(..8)
            .ok_or(STATUS_INFO_LENGTH_MISMATCH)?
            .try_into()
            .unwrap(),
    );
    let mut header = [0; 8];
    copy_from_user(&mut header, address)?;
    if header[0] != 1 || header[1] as usize > crate::se::sid::SID_MAX_SUB_AUTHORITIES {
        return Err(STATUS_INVALID_PARAMETER);
    }
    let mut sid_bytes = [0; core::mem::size_of::<sid>()];
    copy_from_user(&mut sid_bytes[..8 + header[1] as usize * 4], address)?;
    let value = sid::from_bytes(&sid_bytes).ok_or(STATUS_INVALID_PARAMETER)?;
    Ok(Change::PrimaryGroup(value))
}

fn apply_change(handle: HANDLE, change: Change) -> NTSTATUS {
    let access = if matches!(change, Change::Session(_)) {
        0x100
    } else {
        0x80
    };
    // Privilege checks use the caller's effective token, never the target token.
    let may_set_session = !matches!(change, Change::Session(_)) || super::privilege::enabled(7);
    with_sync_access(handle, access, |entries| {
        if entries[0].kind != HandleKind::Token {
            return Err(STATUS_OBJECT_TYPE_MISMATCH);
        }
        if !may_set_session {
            return Err(STATUS_PRIVILEGE_NOT_HELD);
        }
        // SAFETY: the table lock pins and serializes the token payload.
        let token =
            unsafe { (entries[0].target as *mut Token).as_mut() }.ok_or(STATUS_INVALID_HANDLE)?;
        match change {
            Change::PrimaryGroup(group) => {
                if !token.user.equals(&group)
                    && !token.groups[..token.group_count]
                        .iter()
                        .any(|entry| entry.sid.equals(&group))
                {
                    return Err(STATUS_INVALID_PRIMARY_GROUP);
                }
                token.primary_group = group;
            }
            Change::Session(session) => token.session_id = session,
        }
        Ok(STATUS_SUCCESS)
    })
    .unwrap_or_else(|status| status)
}

/// # Safety
/// Input and any embedded SID pointer are copied from caller memory.
pub unsafe extern "C" fn nt_set_information_token(
    handle: HANDLE,
    class: u32,
    information: PVOID,
    length: u32,
) -> NTSTATUS {
    let operation = || -> Result<NTSTATUS, NTSTATUS> {
        set_class(class)?;
        let needed = if class == 12 { 4 } else { 8 };
        if (length as usize) < needed {
            return Err(STATUS_INFO_LENGTH_MISMATCH);
        }
        if information as usize % 4 != 0 {
            return Err(STATUS_DATATYPE_MISALIGNMENT);
        }
        let mut input = [0; 8];
        copy_from_user(&mut input[..needed], information as u64)?;
        Ok(apply_change(
            handle,
            capture_change(class, &input[..needed])?,
        ))
    };
    operation().unwrap_or_else(|status| status)
}

/// Trusted header buffer; embedded SID pointers still name caller memory.
pub fn nt_set_information_token_kernel(
    handle: HANDLE,
    class: u32,
    information: Option<&[u8]>,
) -> NTSTATUS {
    let change = capture_change(class, information.unwrap_or_default());
    match change {
        Ok(change) => apply_change(handle, change),
        Err(status) => status,
    }
}
