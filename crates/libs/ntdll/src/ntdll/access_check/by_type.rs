//! Native object-tree access checking; both interfaces share capture and policy.

use super::*;

#[allow(clippy::too_many_arguments)]
fn check_by_type(
    descriptor: *const SecurityDescriptor,
    principal: PVOID,
    token: HANDLE,
    desired: u32,
    types: *const ObjectTypeList,
    type_count: u32,
    mapping: *const GenericMapping,
    privileges: *mut PrivilegeSet,
    privilege_length: *mut u32,
    granted: *mut u32,
    access_status: *mut NTSTATUS,
    list: bool,
) -> Result<(), NTSTATUS> {
    if list && type_count == 0 {
        return Err(STATUS_INVALID_PARAMETER);
    }
    let count = if list { type_count as usize } else { 1 };
    let output_length = count.checked_mul(4).ok_or(STATUS_INVALID_PARAMETER)?;
    if list && ((granted as u64) & 3 != 0 || (access_status as u64) & 3 != 0) {
        return Err(STATUS_DATATYPE_MISALIGNMENT);
    }
    probe_user_write(granted as u64, output_length)?;
    probe_user_write(access_status as u64, output_length)?;
    let available: u32 = read_user_object(privilege_length as u64)?;
    probe_user_write(privilege_length as u64, 4)?;
    if available != 0 && (privileges as u64) & 3 != 0 {
        return Err(STATUS_DATATYPE_MISALIGNMENT);
    }
    probe_user_write(privileges as u64, available as usize)?;
    if available >= 20 && !privileges.is_null() {
        write_user_object(privileges as u64, &0u32)?;
    }
    if (mapping as u64) & 3 != 0 {
        return Err(STATUS_DATATYPE_MISALIGNMENT);
    }
    let mapping = read_user_object(mapping as u64)?;
    if desired & 0xf000_0000 != 0 {
        return Err(STATUS_GENERIC_NOT_MAPPED);
    }
    let types = object_types::capture(types as u64, type_count)?;
    let descriptor = capture::descriptor(capture::Memory::Caller(descriptor as u64))?;
    let principal = if principal.is_null() {
        None
    } else {
        Some(capture::sid(capture::Memory::Caller(principal as u64), 0)?)
    };
    let result = with_token(token, |token| {
        evaluate::evaluate_types(
            &descriptor,
            token,
            desired,
            &mapping,
            &types,
            principal.as_deref(),
            list,
        )
    })?;
    write_privileges(&result[0], privileges, privilege_length, available)?;
    for (index, decision) in result.iter().enumerate() {
        write_user_object((granted as u64) + index as u64 * 4, &decision.granted)?;
        write_user_object((access_status as u64) + index as u64 * 4, &decision.status)?;
    }
    Ok(())
}

/// # Safety
/// All buffers follow the 64-bit NT ABI. The principal SID may be null. A
/// zero-length type list is accepted by the scalar interface.
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn nt_access_check_by_type(
    descriptor: *const SecurityDescriptor,
    principal: PVOID,
    token: HANDLE,
    desired: u32,
    types: *const ObjectTypeList,
    type_count: u32,
    mapping: *const GenericMapping,
    privileges: *mut PrivilegeSet,
    privilege_length: *mut u32,
    granted: *mut u32,
    access_status: *mut NTSTATUS,
) -> NTSTATUS {
    check_by_type(
        descriptor,
        principal,
        token,
        desired,
        types,
        type_count,
        mapping,
        privileges,
        privilege_length,
        granted,
        access_status,
        false,
    )
    .map_or_else(|status| status, |()| STATUS_SUCCESS)
}

/// # Safety
/// GrantedAccess and AccessStatus each contain type_count DWORDs. The list
/// must contain a root and cannot be empty.
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn nt_access_check_by_type_result_list(
    descriptor: *const SecurityDescriptor,
    principal: PVOID,
    token: HANDLE,
    desired: u32,
    types: *const ObjectTypeList,
    type_count: u32,
    mapping: *const GenericMapping,
    privileges: *mut PrivilegeSet,
    privilege_length: *mut u32,
    granted: *mut u32,
    access_status: *mut NTSTATUS,
) -> NTSTATUS {
    check_by_type(
        descriptor,
        principal,
        token,
        desired,
        types,
        type_count,
        mapping,
        privileges,
        privilege_length,
        granted,
        access_status,
        true,
    )
    .map_or_else(|status| status, |()| STATUS_SUCCESS)
}
