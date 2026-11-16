//! Token duplication with independent payload ownership.
//!
//! Service shape and QoS/type validation follow NT 6.1.7601. This model still
//! lacks token object security descriptors: expanding a source handle's rights
//! and supplying an explicit descriptor are therefore unsupported.

use super::file::{clone_owned_handle, free_handle, HandleKind};
use super::status::*;
use super::types::{ObjectAttributes, HANDLE, NTSTATUS};
use crate::mm::user_copy::{probe_user_write, read_user_object, write_user_object};
use crate::se::token::{self, ImpersonationLevel, Token, TokenTypeEnum};

#[repr(C)]
#[derive(Clone, Copy)]
pub struct SecurityQualityOfService {
    pub length: u32,
    pub impersonation_level: u32,
    pub context_tracking_mode: u8,
    pub effective_only: u8,
}

pub(crate) fn map_token_access(access: u32) -> Result<u32, NTSTATUS> {
    super::object::map_sync_access(HandleKind::Token, access)
}

fn level(value: u32) -> Result<ImpersonationLevel, NTSTATUS> {
    match value {
        0 => Ok(ImpersonationLevel::SecurityAnonymous),
        1 => Ok(ImpersonationLevel::SecurityIdentification),
        2 => Ok(ImpersonationLevel::SecurityImpersonation),
        3 => Ok(ImpersonationLevel::SecurityDelegation),
        _ => Err(STATUS_BAD_IMPERSONATION_LEVEL),
    }
}

/// Final object teardown. No handle table operations are allowed here.
pub(crate) unsafe fn release_token(target: u64) {
    let token = target as *mut Token;
    // SAFETY: clone_owned_handle transfers one independently allocated payload
    // and calls this only after its last handle/pinned operation has gone away.
    unsafe {
        if !(*token).default_dacl.is_null() {
            crate::mm::pool::free((*token).default_dacl.cast());
        }
        crate::mm::pool::free(token.cast());
    }
}

/// Trusted entry shared by the native service and advapi32.
pub(crate) fn duplicate_token(
    source: HANDLE,
    desired_access: u32,
    effective_only: u8,
    token_type: u32,
    inherit: bool,
    requested_level: Option<u32>,
) -> Result<HANDLE, NTSTATUS> {
    let kind = match token_type {
        1 => TokenTypeEnum::TokenPrimary,
        2 => TokenTypeEnum::TokenImpersonation,
        _ => return Err(STATUS_INVALID_PARAMETER),
    };
    let requested_level = requested_level.map(level).transpose()?;
    let access = if desired_access == 0 {
        None
    } else {
        Some(map_token_access(desired_access)?)
    };
    clone_owned_handle(source, HandleKind::Token, 2, access, inherit, |target| {
        // SAFETY: the handle table pins the payload and serializes token writes
        // for the entire initializer, including the independent DACL copy.
        let original = unsafe { (target as *const Token).as_ref() }.ok_or(STATUS_INVALID_HANDLE)?;
        let impersonation = requested_level.unwrap_or(original.impersonation_level);
        if original.token_type == TokenTypeEnum::TokenImpersonation
            && ((kind == TokenTypeEnum::TokenImpersonation
                && impersonation as u32 > original.impersonation_level as u32)
                || (kind == TokenTypeEnum::TokenPrimary
                    && (original.impersonation_level as u32) < 2))
        {
            return Err(STATUS_BAD_IMPERSONATION_LEVEL);
        }
        let id = super::luid::allocate()?;
        let duplicate = token::duplicate_for_process(original);
        if duplicate.is_null() {
            return Err(STATUS_NO_MEMORY);
        }
        // SAFETY: the new token is exclusively owned until handle insertion.
        let value = unsafe { &mut *duplicate };
        value.token_id = token::luid::from_u64(id);
        value.refcount = 1;
        value.token_type = kind;
        value.impersonation_level = impersonation;
        if effective_only != 0 {
            // NT 6.1 retains deny-only and integrity groups even when disabled.
            let mut count = 0;
            for index in 0..value.group_count {
                let group = value.groups[index];
                if group.attributes
                    & (token::SE_GROUP_ENABLED
                        | token::SE_GROUP_USE_FOR_DENY_ONLY
                        | token::SE_GROUP_INTEGRITY)
                    != 0
                {
                    value.groups[count] = group;
                    count += 1;
                }
            }
            value.groups[count..].fill(token::TokenGroup::default());
            value.group_count = count;
            count = 0;
            for index in 0..value.privilege_count {
                let privilege = value.privileges[index];
                if privilege.is_enabled() {
                    value.privileges[count] = privilege;
                    count += 1;
                }
            }
            value.privileges[count..].fill(token::TokenPrivilege::default());
            value.privilege_count = count;
        }
        Ok((duplicate as u64, release_token as unsafe fn(u64)))
    })
}

fn capture_attributes(address: *const ObjectAttributes) -> Result<(bool, Option<u32>), NTSTATUS> {
    if address.is_null() {
        return Ok((false, None));
    }
    if address as usize % 4 != 0 {
        return Err(0x8000_0002u32 as i32);
    }
    let attributes: ObjectAttributes = read_user_object(address as u64)?;
    if attributes.length as usize != core::mem::size_of::<ObjectAttributes>() {
        return Err(STATUS_INVALID_PARAMETER);
    }
    let qos = attributes.security_quality_of_service;
    let requested_level = if qos.is_null() {
        None
    } else {
        if qos as usize % 4 != 0 {
            return Err(0x8000_0002u32 as i32);
        }
        let qos: SecurityQualityOfService = read_user_object(qos as u64)?;
        if qos.length as usize != core::mem::size_of::<SecurityQualityOfService>()
            || qos.context_tracking_mode > 1
        {
            return Err(STATUS_INVALID_PARAMETER);
        }
        level(qos.impersonation_level)?;
        Some(qos.impersonation_level)
    };
    if !attributes.object_name.is_null() || !attributes.root_directory.is_null() {
        return Err(STATUS_NOT_SUPPORTED);
    }
    if !attributes.security_descriptor.is_null() || attributes.attributes & !0x42 != 0 {
        return Err(STATUS_NOT_SUPPORTED);
    }
    Ok((attributes.attributes & 2 != 0, requested_level))
}

/// # Safety
/// Pointer arguments name caller memory and are captured through user-copy.
pub unsafe extern "C" fn nt_duplicate_token(
    source: HANDLE,
    desired_access: u32,
    attributes: *mut ObjectAttributes,
    effective_only: u8,
    token_type: u32,
    output: *mut HANDLE,
) -> NTSTATUS {
    if !matches!(token_type, 1 | 2) {
        return STATUS_INVALID_PARAMETER;
    }
    if let Err(status) = probe_user_write(output as u64, core::mem::size_of::<HANDLE>()) {
        return status;
    }
    let (inherit, requested_level) = match capture_attributes(attributes) {
        Ok(value) => value,
        Err(status) => return status,
    };
    let handle = match duplicate_token(
        source,
        desired_access,
        effective_only,
        token_type,
        inherit,
        requested_level,
    ) {
        Ok(handle) => handle,
        Err(status) => return status,
    };
    if let Err(status) = write_user_object(output as u64, &handle) {
        free_handle(handle);
        return status;
    }
    STATUS_SUCCESS
}

/// Trusted helper preserving the historical library call shape. The fourth
/// argument is TOKEN_TYPE, never SECURITY_IMPERSONATION_LEVEL.
pub fn nt_duplicate_token_kernel(
    source: HANDLE,
    desired_access: u32,
    effective_only: u32,
    token_type: u32,
    output: &mut HANDLE,
) -> NTSTATUS {
    match duplicate_token(
        source,
        desired_access,
        effective_only as u8,
        token_type,
        false,
        None,
    ) {
        Ok(handle) => {
            *output = handle;
            STATUS_SUCCESS
        }
        Err(status) => status,
    }
}
