//! ntdll — Security and Token Management (NT 6.1.7601)
//!
//! Security-related Native API functions for Windows 7:
//! - Token operations (nt_open_process_token, nt_open_thread_token)
//! - Privilege management (nt_adjust_privileges_token)
//! - Access checking (nt_access_check, nt_privilege_check)
//! - Security descriptors (nt_query_security_object, nt_set_security_object)
//! - Impersonation (nt_impersonate_thread)
//!
//! Integrates with the kernel security subsystem (se/).
//!
//! References:
//!   * MSDN Library "Windows 7" — Security Functions
//!   * Windows Internals 7th Ed. - Chapter 7 (Security)

use super::file::{alloc_handle, lookup_handle, HandleKind};
use super::status::{
    STATUS_BUFFER_TOO_SMALL, STATUS_INVALID_HANDLE,
    STATUS_INVALID_PARAMETER, STATUS_NO_TOKEN, STATUS_SUCCESS,
};
use super::types::{HANDLE, NTSTATUS};
use core::ptr;

use crate::mm::user_copy::{
    copy_from_user, copy_to_user, probe_user_read, probe_user_write, write_user_object,
};

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TOKENTYPE {
    TokenPrimary = 1,
    TokenImpersonation = 2,
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenInformationClass {
    TOKENUSER = 1,
    TOKENGROUPS = 2,
    TOKENPRIVILEGES = 3,
    TOKENOWNER = 4,
    TOKENPRIMARYGROUP = 5,
    TOKENDEFAULTDACL = 6,
    TOKENSOURCE = 7,
    TOKENTYPE = 8,
    TOKENIMPERSONATIONLEVEL = 9,
    TOKENSTATISTICS = 10,
    TOKENRESTRICTEDSIDS = 11,
    TOKENSESSIONID = 12,
    TOKENGROUPSANDPRIVILEGES = 13,
    TOKENSESSIONREFERENCE = 14,
    TOKENSANDBOXINERT = 15,
}

pub const SE_PRIVILEGE_ENABLED_BY_DEFAULT: u32 = 0x00000001;
pub const SE_PRIVILEGE_ENABLED: u32 = 0x00000002;
pub const SE_PRIVILEGE_REMOVED: u32 = 0x00000004;
pub const SE_PRIVILEGE_USED_FOR_ACCESS: u32 = 0x80000000;

pub use super::token_privileges::{
    check_privileges_for_caller, nt_adjust_privileges_token, nt_adjust_privileges_token_kernel,
    nt_privilege_check, nt_privilege_check_kernel, LuidAndAttributes,
    PrivilegeSet, TOKEN_PRIVILEGES,
};

pub use super::token_groups::{nt_adjust_groups_token, SidAndAttributes, TOKEN_GROUPS};

pub use super::access_check::{
    check_access_for_caller, nt_access_check, nt_access_check_kernel,
    GenericMapping, SecurityDescriptor,
};

pub const OWNER_SECURITY_INFORMATION: u32 = 0x00000001;
pub const GROUP_SECURITY_INFORMATION: u32 = 0x00000002;
pub const DACL_SECURITY_INFORMATION: u32 = 0x00000004;
pub const SACL_SECURITY_INFORMATION: u32 = 0x00000008;

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
pub unsafe extern "C" fn nt_open_process_token(
    process_handle: HANDLE,
    desired_access: u32,
    token_handle: *mut HANDLE,
) -> NTSTATUS {
    if token_handle.is_null() {
        return STATUS_INVALID_PARAMETER;
    }
    if crate::mm::user_copy::probe_user_write(token_handle as u64, core::mem::size_of::<HANDLE>())
        .is_err()
    {
        return crate::libs::ntdll::status::STATUS_ACCESS_VIOLATION;
    }

    let mut handle = HANDLE::default();
    let status = nt_open_process_token_kernel(process_handle, desired_access, &mut handle);
    if status == STATUS_SUCCESS
        && crate::mm::user_copy::write_user_object(token_handle as u64, &handle).is_err()
    {
        return crate::libs::ntdll::status::STATUS_ACCESS_VIOLATION;
    }
    status
}

/// Trusted-kernel entry point for `nt_open_process_token`.
pub fn nt_open_process_token_kernel(
    process_handle: HANDLE,
    desired_access: u32,
    token_handle: &mut HANDLE,
) -> NTSTATUS {
    let proc_entry = match lookup_handle(process_handle) {
        Some(entry) if entry.kind == HandleKind::Process => entry,
        _ => return STATUS_INVALID_HANDLE,
    };

    let token = crate::ps::process::get_by_pid(proc_entry.target)
        .map(|process| process.get_token())
        .filter(|token| !token.is_null())
        .unwrap_or_else(crate::se::token::create_system_token);
    if token.is_null() {
        return crate::libs::ntdll::status::STATUS_NO_MEMORY;
    }

    let _ = desired_access;
    let token_handle_value = alloc_handle(HandleKind::Token, token as u64);
    if token_handle_value.is_null() {
        return STATUS_INVALID_HANDLE;
    }
    *token_handle = token_handle_value;
    STATUS_SUCCESS
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
pub unsafe extern "C" fn nt_open_thread_token(
    thread_handle: HANDLE,
    desired_access: u32,
    open_as_self: u8,
    token_handle: *mut HANDLE,
) -> NTSTATUS {
    if token_handle.is_null() {
        return STATUS_INVALID_PARAMETER;
    }
    if crate::mm::user_copy::probe_user_write(token_handle as u64, core::mem::size_of::<HANDLE>())
        .is_err()
    {
        return crate::libs::ntdll::status::STATUS_ACCESS_VIOLATION;
    }

    let mut handle = HANDLE::default();
    let status =
        nt_open_thread_token_kernel(thread_handle, desired_access, open_as_self, &mut handle);
    if status == STATUS_SUCCESS
        && crate::mm::user_copy::write_user_object(token_handle as u64, &handle).is_err()
    {
        return crate::libs::ntdll::status::STATUS_ACCESS_VIOLATION;
    }
    status
}

/// Trusted-kernel entry point for `nt_open_thread_token`.
pub fn nt_open_thread_token_kernel(
    thread_handle: HANDLE,
    desired_access: u32,
    open_as_self: u8,
    token_handle: &mut HANDLE,
) -> NTSTATUS {
    let thread_entry = match lookup_handle(thread_handle) {
        Some(entry) if entry.kind == HandleKind::Thread => entry,
        _ => return STATUS_INVALID_HANDLE,
    };

    let _ = (thread_entry, desired_access, open_as_self);
    let _ = token_handle;
    STATUS_NO_TOKEN
}

pub use super::token_duplicate::{nt_duplicate_token, nt_duplicate_token_kernel, SecurityQualityOfService};

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
pub unsafe extern "C" fn nt_query_security_object(
    handle: HANDLE,
    security_information: u32,
    security_descriptor: *mut SecurityDescriptor,
    length: u32,
    length_needed: *mut u32,
) -> NTSTATUS {
    if handle.is_null() {
        return STATUS_INVALID_HANDLE;
    }

    if !length_needed.is_null()
        && probe_user_write(length_needed as u64, core::mem::size_of::<u32>()).is_err()
    {
        return crate::libs::ntdll::status::STATUS_ACCESS_VIOLATION;
    }
    if !security_descriptor.is_null()
        && length >= core::mem::size_of::<SecurityDescriptor>() as u32
        && probe_user_write(
            security_descriptor as u64,
            core::mem::size_of::<SecurityDescriptor>(),
        )
        .is_err()
    {
        return crate::libs::ntdll::status::STATUS_ACCESS_VIOLATION;
    }

    let _entry = match lookup_handle(handle) {
        Some(e) => e,
        _ => return STATUS_INVALID_HANDLE,
    };

    let needed = core::mem::size_of::<SecurityDescriptor>() as u32;

    let mut kernel_descriptor = SecurityDescriptor {
        revision: 1,
        sbz1: 0,
        control: 0,
        owner: ptr::null_mut(),
        group: ptr::null_mut(),
        sacl: ptr::null_mut(),
        dacl: ptr::null_mut(),
    };
    let mut kernel_length_needed = needed;
    let status = nt_query_security_object_kernel(
        handle,
        security_information,
        if security_descriptor.is_null() || length < needed {
            None
        } else {
            Some(&mut kernel_descriptor)
        },
        length,
        &mut kernel_length_needed,
    );

    if !length_needed.is_null()
        && write_user_object(length_needed as u64, &kernel_length_needed).is_err()
    {
        return crate::libs::ntdll::status::STATUS_ACCESS_VIOLATION;
    }
    if !security_descriptor.is_null() && length >= needed {
        let descriptor_bytes = core::slice::from_raw_parts(
            &kernel_descriptor as *const SecurityDescriptor as *const u8,
            needed as usize,
        );
        if copy_to_user(security_descriptor as u64, descriptor_bytes).is_err() {
            return crate::libs::ntdll::status::STATUS_ACCESS_VIOLATION;
        }
    }

    status
}

/// Trusted-kernel entry point for `nt_query_security_object`.
pub fn nt_query_security_object_kernel(
    handle: HANDLE,
    security_information: u32,
    security_descriptor: Option<&mut SecurityDescriptor>,
    length: u32,
    length_needed: &mut u32,
) -> NTSTATUS {
    if handle.is_null() {
        return STATUS_INVALID_HANDLE;
    }

    let _entry = match lookup_handle(handle) {
        Some(e) => e,
        _ => return STATUS_INVALID_HANDLE,
    };

    let _ = security_information;
    let needed = core::mem::size_of::<SecurityDescriptor>() as u32;
    *length_needed = needed;

    if security_descriptor.is_none() || length < needed {
        return STATUS_BUFFER_TOO_SMALL;
    }

    if let Some(security_descriptor) = security_descriptor {
        *security_descriptor = SecurityDescriptor {
            revision: 1,
            sbz1: 0,
            control: 0,
            owner: ptr::null_mut(),
            group: ptr::null_mut(),
            sacl: ptr::null_mut(),
            dacl: ptr::null_mut(),
        };
    }

    STATUS_SUCCESS
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
pub unsafe extern "C" fn nt_set_security_object(
    handle: HANDLE,
    security_information: u32,
    security_descriptor: *const SecurityDescriptor,
) -> NTSTATUS {
    if handle.is_null() || security_descriptor.is_null() {
        return STATUS_INVALID_PARAMETER;
    }

    if probe_user_read(
        security_descriptor as u64,
        core::mem::size_of::<SecurityDescriptor>(),
    )
    .is_err()
    {
        return crate::libs::ntdll::status::STATUS_ACCESS_VIOLATION;
    }

    let mut kernel_descriptor = alloc::vec![0u8; core::mem::size_of::<SecurityDescriptor>()];
    if copy_from_user(&mut kernel_descriptor, security_descriptor as u64).is_err() {
        return crate::libs::ntdll::status::STATUS_ACCESS_VIOLATION;
    }

    nt_set_security_object_kernel(handle, security_information, Some(&kernel_descriptor))
}

/// Trusted-kernel entry point for `nt_set_security_object`.
pub fn nt_set_security_object_kernel(
    handle: HANDLE,
    security_information: u32,
    security_descriptor: Option<&[u8]>,
) -> NTSTATUS {
    if handle.is_null() || security_descriptor.is_none() {
        return STATUS_INVALID_PARAMETER;
    }

    let _entry = match lookup_handle(handle) {
        Some(e) => e,
        _ => return STATUS_INVALID_HANDLE,
    };

    let _ = security_information;

    STATUS_SUCCESS
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
pub unsafe extern "C" fn nt_impersonate_thread(
    server_thread_handle: HANDLE,
    client_thread_handle: HANDLE,
    security_qos: *const SecurityQualityOfService,
) -> NTSTATUS {
    if server_thread_handle.is_null() || client_thread_handle.is_null() {
        return STATUS_INVALID_PARAMETER;
    }

    let _server = match lookup_handle(server_thread_handle) {
        Some(e) if e.kind == HandleKind::Thread => e,
        _ => return STATUS_INVALID_HANDLE,
    };

    let _client = match lookup_handle(client_thread_handle) {
        Some(e) if e.kind == HandleKind::Thread => e,
        _ => return STATUS_INVALID_HANDLE,
    };

    if !security_qos.is_null()
        && probe_user_read(
            security_qos as u64,
            core::mem::size_of::<SecurityQualityOfService>(),
        )
        .is_err()
    {
        return crate::libs::ntdll::status::STATUS_ACCESS_VIOLATION;
    }

    let _ = security_qos;

    STATUS_SUCCESS
}

pub use super::token_information::{
    nt_query_information_token, nt_query_information_token_kernel,
    nt_set_information_token, nt_set_information_token_kernel,
};

/// The Ex forms accept only OBJ_INHERIT and retain the requested access on the
/// returned handle. Pointer faults after allocation retire the new handle.
unsafe fn open_token_ex(
    object: HANDLE,
    access: u32,
    attributes: u32,
    output: *mut HANDLE,
    thread: bool,
    as_self: u8,
) -> NTSTATUS {
    use super::status::*;
    if attributes & !2 != 0 { return STATUS_INVALID_PARAMETER; }
    if access & !(0x1f01ff | 0xf2000000) != 0 { return STATUS_ACCESS_DENIED; }
    if let Err(status) = probe_user_write(output as u64, core::mem::size_of::<HANDLE>()) { return status; }
    let current = crate::ps::thread::get_current_ethread();
    if current.is_null() { return STATUS_INVALID_HANDLE; }
    // SAFETY: a syscall's current thread and owning process remain live.
    let (pid, process) = unsafe { ((*current).client_id.unique_process, (*current).threads_process) };
    let pseudo = if thread { -2i64 as u64 } else { -1i64 as u64 };
    if object as u64 != pseudo {
        let entry = match lookup_handle(object) {
            Some(entry) => entry,
            None => return STATUS_INVALID_HANDLE,
        };
        let expected = if thread { HandleKind::Thread } else { HandleKind::Process };
        if entry.kind != expected { return STATUS_OBJECT_TYPE_MISMATCH; }
        // Tokens do not yet have an independent object security descriptor.
        // Until that is implemented, never grant access to another process's
        // token based on the token's default DACL (which protects new objects).
        if !thread && entry.target != pid { return STATUS_ACCESS_DENIED; }
    }
    if thread {
        let _ = as_self;
        return STATUS_NO_TOKEN;
    }
    let token = if process.is_null() { core::ptr::null() } else {
        unsafe { (*process).get_token() }
    };
    if token.is_null() { return STATUS_NO_TOKEN; }
    let mut granted = access & 0x001f01ff;
    if access & 0x02000000 != 0 { return STATUS_NOT_SUPPORTED; }
    if access & 0x10000000 != 0 { granted |= 0xf01ff; }
    if access & 0x80000000 != 0 { granted |= 0x20008; }
    if access & 0x40000000 != 0 { granted |= 0x200e0; }
    if access & 0x20000000 != 0 { granted |= 0x20000; }
    let handle = match super::file::alloc_access_handle(HandleKind::Token, token as u64, granted, attributes & 2 != 0) {
        Ok(handle) => handle,
        Err(status) => return status,
    };
    if let Err(status) = write_user_object(output as u64, &handle) {
        super::file::free_handle(handle);
        return status;
    }
    STATUS_SUCCESS
}

/// # Safety
/// Output points to caller memory.
pub unsafe extern "C" fn nt_open_process_token_ex(process: HANDLE, access: u32, attributes: u32, output: *mut HANDLE) -> NTSTATUS {
    unsafe { open_token_ex(process, access, attributes, output, false, 0) }
}

/// # Safety
/// Output points to caller memory.
pub unsafe extern "C" fn nt_open_thread_token_ex(thread: HANDLE, access: u32, as_self: u8, attributes: u32, output: *mut HANDLE) -> NTSTATUS {
    unsafe { open_token_ex(thread, access, attributes, output, true, as_self) }
}

/// Compare security content, ignoring token IDs and modification counters.
pub extern "C" fn nt_compare_tokens(first: HANDLE, second: HANDLE, output: *mut u8) -> NTSTATUS {
    use super::status::*;
    let result = super::file::with_sync_handles_access(&[first, second], 8, |entries| {
        if entries.iter().any(|entry| entry.kind != HandleKind::Token) { return Err(STATUS_OBJECT_TYPE_MISMATCH); }
        // The table guard pins both payloads, including owned duplicated tokens,
        // and prevents close/reuse while comparing security content.
        let (Some(a), Some(b)) = (unsafe { (entries[0].target as *const crate::se::token::Token).as_ref() }, unsafe { (entries[1].target as *const crate::se::token::Token).as_ref() }) else {
            return Err(STATUS_INVALID_HANDLE);
        };
        let equal = a.user.equals(&b.user) && a.primary_group.equals(&b.primary_group)
            && a.authentication_id == b.authentication_id && a.token_type == b.token_type
            && a.impersonation_level == b.impersonation_level && a.session_id == b.session_id
            && a.flags == b.flags && a.integrity_level_rid == b.integrity_level_rid
            && equal_token_acl(a.default_dacl, b.default_dacl)
            && a.group_count == b.group_count && a.privilege_count == b.privilege_count
            && a.groups[..a.group_count.min(a.groups.len())].iter().all(|group| b.groups[..b.group_count.min(b.groups.len())].iter().any(|other| group.sid.equals(&other.sid) && group.attributes == other.attributes))
            && a.privileges[..a.privilege_count.min(a.privileges.len())].iter().all(|privilege| b.privileges[..b.privilege_count.min(b.privileges.len())].iter().any(|other| privilege.luid == other.luid && privilege.attributes == other.attributes));
        Ok(u8::from(equal))
    });
    match result {
        Ok(equal) => write_user_object(output as u64, &equal).err().unwrap_or(STATUS_SUCCESS),
        Err(status) => status,
    }
}

fn equal_token_acl(first: *const crate::se::acl::Acl, second: *const crate::se::acl::Acl) -> bool {
    if first == second { return true; }
    // SAFETY: token-owned ACL storage has the same lifetime as the tokens.
    let (Some(a), Some(b)) = (unsafe { first.as_ref() }, unsafe { second.as_ref() }) else {
        return false;
    };
    let length = usize::from(a.acl_size).saturating_sub(8);
    a.acl_size >= 8 && b.acl_size == a.acl_size && a.revision == b.revision
        && a.ace_count == b.ace_count && length <= a.data.len()
        && a.data[..length] == b.data[..length]
}
