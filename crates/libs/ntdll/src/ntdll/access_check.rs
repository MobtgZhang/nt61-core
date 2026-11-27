//! Captured NtAccessCheck buffers and ordered discretionary access evaluation.
//!
//! Captures basic/object ACEs and object type trees. Callback/conditional ACEs
//! and auditing still require additional kernel models.

use super::file::{with_sync_access, HandleKind};
use super::status::*;
use super::token_privileges::PrivilegeSet;
use super::types::{HANDLE, NTSTATUS, PVOID};
use crate::mm::user_copy::{
    copy_from_user, copy_to_user, probe_user_write, read_user_object, write_user_object,
};
use crate::se::token::{luid, ImpersonationLevel, Token, TokenTypeEnum};

#[path = "access_check/capture.rs"]
mod capture;
#[path = "access_check/evaluate.rs"]
mod evaluate;
#[path = "access_check/object_types.rs"]
mod object_types;
pub use object_types::ObjectTypeList;
#[path = "access_check/by_type.rs"]
mod by_type;
pub use by_type::{nt_access_check_by_type, nt_access_check_by_type_result_list};
#[cfg(test)]
#[path = "access_check/object_tests.rs"]
mod object_tests;

const STATUS_INVALID_SECURITY_DESCR: NTSTATUS = 0xc000_0079u32 as i32;
const STATUS_INVALID_ACL: NTSTATUS = 0xc000_0077u32 as i32;
const STATUS_INVALID_SID: NTSTATUS = 0xc000_0078u32 as i32;
const STATUS_NO_IMPERSONATION_TOKEN: NTSTATUS = 0xc000_005cu32 as i32;
const STATUS_GENERIC_NOT_MAPPED: NTSTATUS = 0xc000_00e6u32 as i32;
const MAXIMUM_ALLOWED: u32 = 0x0200_0000;
const ACCESS_SYSTEM_SECURITY: u32 = 0x0100_0000;
const READ_CONTROL: u32 = 0x0002_0000;
const WRITE_DAC: u32 = 0x0004_0000;
const WRITE_OWNER: u32 = 0x0008_0000;

#[repr(C)]
pub struct SecurityDescriptor {
    pub revision: u8,
    pub sbz1: u8,
    pub control: u16,
    pub owner: PVOID,
    pub group: PVOID,
    pub sacl: PVOID,
    pub dacl: PVOID,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct GenericMapping {
    pub generic_read: u32,
    pub generic_write: u32,
    pub generic_execute: u32,
    pub generic_all: u32,
}

impl GenericMapping {
    fn map(&self, mut mask: u32) -> u32 {
        for (bit, specific) in [
            (0x8000_0000, self.generic_read),
            (0x4000_0000, self.generic_write),
            (0x2000_0000, self.generic_execute),
            (0x1000_0000, self.generic_all),
        ] {
            if mask & bit != 0 {
                mask |= specific;
            }
        }
        mask & !0xf000_0000
    }
}

pub(crate) const FILE_MAPPING: GenericMapping = GenericMapping {
    generic_read: 0x0012_0089,
    generic_write: 0x0012_0116,
    generic_execute: 0x0012_00a0,
    generic_all: 0x001f_01ff,
};

pub(crate) fn map_file_access(desired: u32) -> u32 {
    FILE_MAPPING.map(desired)
}

pub(crate) fn check_file_descriptor(
    bytes: &[u8],
    token: &Token,
    desired: u32,
) -> Result<u32, NTSTATUS> {
    use crate::fs::ntfs::relative_security_descriptor::RelativeDescriptor;

    if token.flags & crate::se::token::TOKEN_WRITE_RESTRICTED != 0 {
        return Err(STATUS_NOT_SUPPORTED);
    }
    let descriptor = RelativeDescriptor::parse(bytes).ok_or(STATUS_INVALID_SECURITY_DESCR)?;
    let (integrity, policy) = file_integrity(descriptor.components[2])?;
    let desired = map_file_access(desired);
    if desired & !(FILE_MAPPING.generic_all | ACCESS_SYSTEM_SECURITY | MAXIMUM_ALLOWED) != 0 {
        return Err(STATUS_ACCESS_DENIED);
    }
    let mut prohibited = 0;
    if token.integrity_level_rid < integrity {
        if policy & 1 != 0 {
            prohibited |= 0x000d_0156;
        }
        if policy & 2 != 0 {
            prohibited |= 0x0002_0089;
        }
        if policy & 4 != 0 {
            prohibited |= 0x0000_0020;
        }
        if desired & prohibited != 0 {
            return Err(STATUS_ACCESS_DENIED);
        }
    }
    let descriptor = capture::descriptor(capture::Memory::Trusted(bytes))?;
    let decision = evaluate::evaluate(&descriptor, token, desired, &FILE_MAPPING)?;
    if decision.status != STATUS_SUCCESS {
        return Err(decision.status);
    }
    let granted = decision.granted & !prohibited;
    if granted == 0 && desired & MAXIMUM_ALLOWED != 0 {
        return Err(STATUS_ACCESS_DENIED);
    }
    Ok(granted)
}

fn file_integrity(sacl: Option<&[u8]>) -> Result<(u32, u32), NTSTATUS> {
    let mut label = None;
    if let Some(acl) = sacl {
        let mut offset = 8;
        for _ in 0..u16::from_le_bytes([acl[4], acl[5]]) {
            let size = u16::from_le_bytes([acl[offset + 2], acl[offset + 3]]) as usize;
            let ace = &acl[offset..offset + size];
            offset += size;
            if ace[1] & 8 != 0 {
                continue;
            }
            match ace[0] {
                2 | 3 | 7 | 8 => continue,
                0x11 => {}
                _ => return Err(STATUS_NOT_SUPPORTED),
            }
            if label.is_some() || ace.len() != 20 || ace[8..16] != [1, 1, 0, 0, 0, 0, 0, 16] {
                return Err(STATUS_INVALID_SECURITY_DESCR);
            }
            let policy = u32::from_le_bytes(ace[4..8].try_into().unwrap());
            let rid = u32::from_le_bytes(ace[16..20].try_into().unwrap());
            if policy & !7 != 0 {
                return Err(STATUS_NOT_SUPPORTED);
            }
            label = Some((rid, policy));
        }
    }
    Ok(label.unwrap_or((0x2000, 1)))
}

/// Access denial is a successfully executed check with a failing access status.
#[derive(Clone, Copy)]
pub struct AccessDecision {
    pub granted: u32,
    pub status: NTSTATUS,
    used_privileges: [u32; 2],
    privilege_count: usize,
}

impl AccessDecision {
    fn new(granted: u32, status: NTSTATUS) -> Self {
        Self {
            granted,
            status,
            used_privileges: [0; 2],
            privilege_count: 0,
        }
    }

    fn use_privilege(&mut self, number: u32) {
        self.used_privileges[self.privilege_count] = number;
        self.privilege_count += 1;
    }
}

fn check(
    descriptor: &capture::Descriptor,
    handle: HANDLE,
    desired: u32,
    mapping: &GenericMapping,
) -> Result<AccessDecision, NTSTATUS> {
    if desired & 0xf000_0000 != 0 {
        return Err(STATUS_GENERIC_NOT_MAPPED);
    }
    with_token(handle, |token| {
        evaluate::evaluate(descriptor, token, desired, mapping)
    })
}

fn with_token<R>(
    handle: HANDLE,
    operation: impl FnOnce(&Token) -> Result<R, NTSTATUS>,
) -> Result<R, NTSTATUS> {
    with_sync_access(handle, 8, |entries| {
        if entries[0].kind != HandleKind::Token {
            return Err(STATUS_OBJECT_TYPE_MISMATCH);
        }
        // SAFETY: the table lock pins and serializes the token payload.
        let token =
            unsafe { (entries[0].target as *const Token).as_ref() }.ok_or(STATUS_INVALID_HANDLE)?;
        if token.token_type != TokenTypeEnum::TokenImpersonation {
            return Err(STATUS_NO_IMPERSONATION_TOKEN);
        }
        if token.impersonation_level == ImpersonationLevel::SecurityAnonymous {
            return Err(STATUS_BAD_IMPERSONATION_LEVEL);
        }
        if token.flags & crate::se::token::TOKEN_WRITE_RESTRICTED != 0 {
            return Err(STATUS_NOT_SUPPORTED);
        }
        operation(token)
    })
}

/// Shared NT/Win32 path. All raw pointers are captured through user-copy.
/// Writes the caller's privilege set; leaves access-result encoding to the API.
pub fn check_access_for_caller(
    descriptor: *const SecurityDescriptor,
    token: HANDLE,
    desired: u32,
    mapping: *const GenericMapping,
    privileges: *mut PrivilegeSet,
    privilege_length: *mut u32,
) -> Result<AccessDecision, NTSTATUS> {
    let available: u32 = read_user_object(privilege_length as u64)?;
    probe_user_write(privilege_length as u64, 4)?;
    probe_user_write(privileges as u64, available as usize)?;
    let mapping = read_user_object(mapping as u64)?;
    // Generic rights are an API error even when the privilege buffer is small.
    if desired & 0xf000_0000 != 0 {
        return Err(STATUS_GENERIC_NOT_MAPPED);
    }
    let descriptor = capture::descriptor(capture::Memory::Caller(descriptor as u64))?;
    let result = check(&descriptor, token, desired, &mapping)?;
    // NT requires at least sizeof(PRIVILEGE_SET), including its first entry,
    // even when PrivilegeCount is zero. On success its input length is retained.
    write_privileges(&result, privileges, privilege_length, available)?;
    Ok(result)
}

fn write_privileges(
    result: &AccessDecision,
    privileges: *mut PrivilegeSet,
    privilege_length: *mut u32,
    available: u32,
) -> Result<(), NTSTATUS> {
    let required = 8 + result.privilege_count.max(1) * 12;
    if (available as usize) < required {
        write_user_object(privilege_length as u64, &(required as u32))?;
        return Err(STATUS_BUFFER_TOO_SMALL);
    }
    let mut output = [0u8; 32];
    output[..4].copy_from_slice(&(result.privilege_count as u32).to_le_bytes());
    for (index, number) in result.used_privileges[..result.privilege_count]
        .iter()
        .enumerate()
    {
        let offset = 8 + index * 12;
        output[offset..offset + 8].copy_from_slice(&(*number as u64).to_le_bytes());
        output[offset + 8..offset + 12].copy_from_slice(&0x8000_0000u32.to_le_bytes());
    }
    copy_to_user(
        privileges as u64,
        &output[..8 + result.privilege_count * 12],
    )?;
    Ok(())
}

/// # Safety
/// Arguments name caller memory using the NT 64-bit layouts. AccessStatus is
/// an NTSTATUS; advapi32 converts it separately to a four-byte BOOL.
pub unsafe extern "C" fn nt_access_check(
    descriptor: *const SecurityDescriptor,
    token: HANDLE,
    desired: u32,
    mapping: *const GenericMapping,
    privileges: *mut PrivilegeSet,
    privilege_length: *mut u32,
    granted: *mut u32,
    access_status: *mut NTSTATUS,
) -> NTSTATUS {
    let operation = || {
        probe_user_write(granted as u64, 4)?;
        probe_user_write(access_status as u64, 4)?;
        let result = check_access_for_caller(
            descriptor,
            token,
            desired,
            mapping,
            privileges,
            privilege_length,
        )?;
        write_user_object(granted as u64, &result.granted)?;
        write_user_object(access_status as u64, &result.status)
    };
    operation().map_or_else(|error| error, |()| STATUS_SUCCESS)
}

/// Trusted descriptor helper. Relative offsets must stay within the supplied
/// slice; absolute descriptors still name caller memory for embedded pointers.
pub fn nt_access_check_kernel(
    descriptor: Option<&[u8]>,
    token: HANDLE,
    desired: u32,
    mapping: Option<&GenericMapping>,
    granted: &mut u32,
    access_status: &mut NTSTATUS,
) -> NTSTATUS {
    let mut operation = || {
        let descriptor = capture::descriptor(capture::Memory::Trusted(
            descriptor.ok_or(STATUS_INVALID_PARAMETER)?,
        ))?;
        let result = check(
            &descriptor,
            token,
            desired,
            mapping.ok_or(STATUS_INVALID_PARAMETER)?,
        )?;
        *granted = result.granted;
        *access_status = result.status;
        Ok(())
    };
    operation().map_or_else(|error| error, |()| STATUS_SUCCESS)
}
