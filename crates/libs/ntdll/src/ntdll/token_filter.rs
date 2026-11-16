//! Restricted token creation with independent DACL ownership and captured lists.
//! LUA policy and WRITE_RESTRICTED are explicitly unsupported until the access
//! evaluator can enforce their distinct policy; they never degrade to success.

use super::file::{clone_owned_handle, free_handle, HandleKind};
use super::status::*;
use super::token_duplicate::release_token;
use super::token_groups::{self, Request, TOKEN_GROUPS};
use super::token_privileges::{self, TOKEN_PRIVILEGES};
use super::types::{HANDLE, NTSTATUS};
use crate::mm::user_copy::{probe_user_write, read_user_object, write_user_object};
use crate::se::{sid::sid, token::{self, Token, TokenGroup}};
use alloc::vec::Vec;

fn groups(address: *const TOKEN_GROUPS) -> Result<Vec<Request>, NTSTATUS> {
    if address.is_null() { Ok(Vec::new()) } else { token_groups::capture(address) }
}

fn restricting_groups(requests: &[Request]) -> Result<Vec<TokenGroup>, NTSTATUS> {
    let mut groups = Vec::new();
    groups.try_reserve_exact(requests.len()).map_err(|_| STATUS_NO_MEMORY)?;
    for request in requests {
        if request.attributes != 0 { return Err(STATUS_INVALID_PARAMETER); }
        let count = usize::from(request.sid[1]);
        // Valid long SIDs cannot be truncated into a different identity.
        if count > 8 { return Err(STATUS_NOT_SUPPORTED); }
        let mut value = sid::default();
        value.revision = request.sid[0];
        value.sub_authority_count = count as u8;
        value.identifier_authority.copy_from_slice(&request.sid[2..8]);
        for (index, authority) in value.sub_authority[..count].iter_mut().enumerate() {
            let offset = 8 + index * 4;
            *authority = u32::from_le_bytes(request.sid[offset..offset + 4].try_into()
                .map_err(|_| STATUS_INVALID_SID)?);
        }
        if !groups.iter().any(|group: &TokenGroup| group.sid.equals(&value)) {
            if groups.len() == token::TOKEN_MAX_GROUPS { return Err(STATUS_INSUFFICIENT_RESOURCES); }
            groups.push(TokenGroup::new(value, 0));
        }
    }
    Ok(groups)
}

/// # Safety
/// Input structures and their embedded SIDs name caller memory. The output is
/// probed before publication and a failed final copy releases the new handle.
pub unsafe extern "C" fn nt_filter_token(
    source: HANDLE,
    flags: u32,
    disable: *const TOKEN_GROUPS,
    delete: *const TOKEN_PRIVILEGES,
    restrict: *const TOKEN_GROUPS,
    output: *mut HANDLE,
) -> NTSTATUS {
    (|| {
        if flags & !0xf != 0 { return Err(STATUS_INVALID_PARAMETER); }
        if flags & 0xc != 0 { return Err(STATUS_NOT_SUPPORTED); }
        let disable = groups(disable)?;
        let restrict = restricting_groups(&groups(restrict)?)?;
        let mut deleted = Vec::new();
        if !delete.is_null() {
            if delete as usize % 4 != 0 { return Err(0x8000_0002u32 as i32); }
            let count = read_user_object::<u32>(delete as u64)?;
            let entries = token_privileges::capture((delete as u64)
                .checked_add(4).ok_or(STATUS_ACCESS_VIOLATION)?, count)?;
            deleted.try_reserve_exact(entries.len()).map_err(|_| STATUS_NO_MEMORY)?;
            deleted.extend(entries.iter().map(|entry| token::luid::from_u64(entry.luid)));
        }
        if output as usize % core::mem::align_of::<HANDLE>() != 0 {
            return Err(0x8000_0002u32 as i32);
        }
        probe_user_write(output as u64, core::mem::size_of::<HANDLE>())?;
        let handle = clone_owned_handle(source, HandleKind::Token, 2, None, false, |target| {
            // SAFETY: clone_owned_handle pins the payload under the table lock.
            let original = unsafe { (target as *const Token).as_ref() }.ok_or(STATUS_INVALID_HANDLE)?;
            let mut disabled = Vec::new();
            disabled.try_reserve_exact(original.group_count + 1).map_err(|_| STATUS_NO_MEMORY)?;
            if disable.iter().any(|request| request.matches(&original.user)) {
                disabled.push(TokenGroup::new(original.user, 0));
            }
            for group in &original.groups[..original.group_count] {
                if disable.iter().any(|request| request.matches(&group.sid)) { disabled.push(*group); }
            }
            let id = super::luid::allocate()?;
            let duplicate = token::duplicate_for_process(original);
            if duplicate.is_null() { return Err(STATUS_NO_MEMORY); }
            // SAFETY: duplicate owns fresh token/DACL storage until publication.
            let value = unsafe { &mut *duplicate };
            if let Err(status) = value.filter(flags, &disabled, &deleted, &restrict) {
                // SAFETY: this unpublished payload is exclusively owned here.
                unsafe { release_token(duplicate as u64); }
                return Err(status);
            }
            value.token_id = token::luid::from_u64(id);
            value.refcount = 1;
            Ok((duplicate as u64, release_token as unsafe fn(u64)))
        })?;
        if let Err(status) = write_user_object(output as u64, &handle) {
            free_handle(handle);
            return Err(status);
        }
        Ok(STATUS_SUCCESS)
    })().unwrap_or_else(|status| status)
}
