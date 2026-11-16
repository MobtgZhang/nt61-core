//! Exercise the production native handlers, including pointer relocation.

use super::state_guard;
use crate::ntdll::file::{alloc_access_handle, free_handle, HandleKind};
use crate::ntdll::status::*;
use crate::ntdll::token_information::*;
use crate::ntdll::types::{HANDLE, PVOID};
use crate::se::acl::{AceFlags, Acl};
use crate::se::sid::{sid, WellKnownSid, SID_USERS};
use crate::se::token::*;

fn source(token: &mut Token, access: u32) -> HANDLE {
    alloc_access_handle(HandleKind::Token, token as *mut Token as u64, access, false).unwrap()
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn u64_at(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}

fn query(handle: HANDLE, class: u32, output: &mut [u64; 128]) -> (i32, u32) {
    let mut length = 0;
    let status = unsafe {
        nt_query_information_token(handle, class, output.as_mut_ptr().cast(), 1024, &mut length)
    };
    (status, length)
}

fn output_bytes(output: &[u64; 128]) -> &[u8] {
    // SAFETY: u64 storage is initialized and valid to read as bytes.
    unsafe { core::slice::from_raw_parts(output.as_ptr().cast(), 1024) }
}

#[test]
fn user_sid_is_variable_length_and_points_inside_the_callers_buffer() {
    let _guard = state_guard();
    let mut token = Token::new_primary(sid::well_known(WellKnownSid::LocalSystem));
    let handle = source(&mut token, 8);
    let mut length = 0;
    assert_eq!(
        unsafe { nt_query_information_token(handle, 1, core::ptr::null_mut(), 0, &mut length) },
        STATUS_BUFFER_TOO_SMALL
    );
    assert_eq!(length, 28);
    let mut output = [0xcccc_cccc_cccc_cccc; 128];
    assert_eq!(query(handle, 1, &mut output), (STATUS_SUCCESS, 28));
    let bytes = output_bytes(&output);
    assert_eq!(u64_at(bytes, 0), bytes.as_ptr() as u64 + 16);
    assert_eq!(u32_at(bytes, 8), 0);
    assert_eq!(u32_at(bytes, 12), 0);
    assert_eq!(&bytes[16..28], &[1, 1, 0, 0, 0, 0, 0, 5, 18, 0, 0, 0]);
    assert_eq!(bytes[28], 0xcc);
    assert!(free_handle(handle));
}

#[test]
fn groups_use_sixteen_byte_entries_without_leaking_fixed_sid_capacity() {
    let _guard = state_guard();
    let mut token = Token::new();
    token.add_group(TokenGroup::new(SID_USERS, SE_GROUP_ENABLED));
    token.add_group(TokenGroup::new(
        sid::well_known(WellKnownSid::LocalSystem),
        SE_GROUP_LOGON_ID | SE_GROUP_ENABLED,
    ));
    let handle = source(&mut token, 8);
    let mut output = [0; 128];
    assert_eq!(query(handle, 2, &mut output), (STATUS_SUCCESS, 68));
    let bytes = output_bytes(&output);
    assert_eq!(u32_at(bytes, 0), 2);
    assert_eq!(u32_at(bytes, 4), 0);
    assert_eq!(u64_at(bytes, 8), bytes.as_ptr() as u64 + 40);
    assert_eq!(u64_at(bytes, 24), bytes.as_ptr() as u64 + 56);
    assert_eq!(u32_at(bytes, 16), SE_GROUP_ENABLED);
    assert_eq!(u32_at(bytes, 32), SE_GROUP_LOGON_ID | SE_GROUP_ENABLED);
    assert!(sid::from_bytes(&bytes[40..56]).unwrap().equals(&SID_USERS));
    assert_eq!(query(handle, 28, &mut output), (STATUS_SUCCESS, 36));
    assert_eq!(u32_at(output_bytes(&output), 0), 1);
    assert!(free_handle(handle));
}

#[test]
fn privilege_query_matches_the_twelve_byte_nt_adjustment_layout() {
    let _guard = state_guard();
    let mut token = Token::new();
    token.add_privilege(TokenPrivilege::new(
        luid::from_u64(0x1234_5678_0000_0017),
        3,
    ));
    token.add_privilege(TokenPrivilege::new(luid::from_u64(18), 0));
    let handle = source(&mut token, 8);
    let mut output = [0; 128];
    assert_eq!(query(handle, 3, &mut output), (STATUS_SUCCESS, 28));
    let bytes = output_bytes(&output);
    assert_eq!(u32_at(bytes, 0), 2);
    assert_eq!(u64_at(bytes, 4), 0x1234_5678_0000_0017);
    assert_eq!(u32_at(bytes, 12), 3);
    assert_eq!(u64_at(bytes, 16), 18);
    assert_eq!(u32_at(bytes, 24), 0);
    assert!(free_handle(handle));
}

#[test]
fn default_acl_is_serialized_as_a_real_acl_and_null_acl_remains_null() {
    let _guard = state_guard();
    let mut token = Token::new();
    let handle = source(&mut token, 8);
    let mut output = [u64::MAX; 128];
    assert_eq!(query(handle, 6, &mut output), (STATUS_SUCCESS, 8));
    assert_eq!(output[0], 0);
    assert!(free_handle(handle));
    let mut acl = Acl::new();
    assert!(acl.add_access_allowed(&SID_USERS, 0x123, AceFlags::from_bits(0)));
    token.default_dacl = &mut acl;
    let handle = source(&mut token, 8);
    assert_eq!(query(handle, 6, &mut output), (STATUS_SUCCESS, 40));
    let bytes = output_bytes(&output);
    assert_eq!(u64_at(bytes, 0), bytes.as_ptr() as u64 + 8);
    assert_eq!(&bytes[8..16], &[2, 0, 32, 0, 1, 0, 0, 0]);
    assert_eq!(&bytes[16..20], &[0, 0, 24, 0]);
    assert_eq!(u32_at(bytes, 20), 0x123);
    assert!(sid::from_bytes(&bytes[24..40]).unwrap().equals(&SID_USERS));
    assert!(free_handle(handle));
}

#[test]
fn scalar_and_integrity_queries_report_the_real_token_values() {
    let _guard = state_guard();
    let mut token = Token::new_impersonation(SID_USERS, ImpersonationLevel::SecurityDelegation);
    token.session_id = 17;
    token.integrity_level_rid = SECURITY_MANDATORY_HIGH;
    let handle = source(&mut token, 8);
    let mut output = [0; 128];
    for (class, expected) in [(8, 2), (9, 3), (12, 17)] {
        assert_eq!(query(handle, class, &mut output), (STATUS_SUCCESS, 4));
        assert_eq!(u32_at(output_bytes(&output), 0), expected);
    }
    assert_eq!(query(handle, 25, &mut output), (STATUS_SUCCESS, 28));
    let bytes = output_bytes(&output);
    assert_eq!(
        u32_at(bytes, 8),
        SE_GROUP_INTEGRITY | SE_GROUP_INTEGRITY_ENABLED
    );
    assert_eq!(&bytes[16..24], &[1, 1, 0, 0, 0, 0, 0, 16]);
    assert_eq!(u32_at(bytes, 24), SECURITY_MANDATORY_HIGH);
    assert!(free_handle(handle));
}

#[test]
fn query_rejects_bad_handles_wrong_types_missing_rights_and_unknown_classes() {
    let _guard = state_guard();
    let mut token = Token::new();
    let handle = source(&mut token, 0x10);
    let mut output = [0; 128];
    assert_eq!(query(handle, 3, &mut output).0, STATUS_ACCESS_DENIED);
    assert_eq!(query(handle, 7, &mut output).0, STATUS_NOT_SUPPORTED);
    assert!(free_handle(handle));
    let handle = source(&mut token, 8);
    assert_eq!(query(handle, 9, &mut output).0, STATUS_INVALID_INFO_CLASS);
    assert_eq!(query(handle, 29, &mut output).0, STATUS_INVALID_INFO_CLASS);
    assert_eq!(query(handle, 10, &mut output).0, STATUS_NOT_SUPPORTED);
    assert!(free_handle(handle));
    assert_eq!(query(handle, 3, &mut output).0, STATUS_INVALID_HANDLE);
    let handle = alloc_access_handle(HandleKind::Process, 100, 8, false).unwrap();
    assert_eq!(query(handle, 3, &mut output).0, STATUS_OBJECT_TYPE_MISMATCH);
    assert!(free_handle(handle));
}

#[test]
fn small_or_faulting_outputs_do_not_overwrite_the_callers_information() {
    let _guard = state_guard();
    let mut token = Token::new();
    let handle = source(&mut token, 8);
    let mut output = [0xcccc_cccc_u32; 4];
    let mut length = 0;
    assert_eq!(
        unsafe {
            nt_query_information_token(handle, 8, output.as_mut_ptr().cast(), 3, &mut length)
        },
        STATUS_BUFFER_TOO_SMALL
    );
    assert_eq!(length, 4);
    assert_eq!(output, [0xcccc_cccc; 4]);
    assert_eq!(
        unsafe {
            nt_query_information_token(
                handle,
                8,
                output.as_mut_ptr().cast(),
                4,
                core::ptr::null_mut(),
            )
        },
        STATUS_ACCESS_VIOLATION
    );
    assert_eq!(output, [0xcccc_cccc; 4]);
    crate::mm::user_copy::FAIL_WRITE.with(|fault| fault.set(output.as_mut_ptr() as u64));
    assert_eq!(
        unsafe {
            nt_query_information_token(handle, 8, output.as_mut_ptr().cast(), 4, &mut length)
        },
        STATUS_ACCESS_VIOLATION
    );
    crate::mm::user_copy::FAIL_WRITE.with(|fault| fault.set(0));
    assert_eq!(output, [0xcccc_cccc; 4]);
    assert!(free_handle(handle));
}

#[test]
fn primary_group_changes_require_membership_and_adjust_default_access() {
    let _guard = state_guard();
    let mut token = Token::new_primary(sid::well_known(WellKnownSid::LocalSystem));
    let group = SID_USERS;
    token.add_group(TokenGroup::new(group, SE_GROUP_ENABLED));
    let handle = source(&mut token, 0x88);
    let pointer = &group as *const sid as u64;
    assert_eq!(
        unsafe { nt_set_information_token(handle, 5, &pointer as *const u64 as PVOID, 8) },
        STATUS_SUCCESS
    );
    assert!(token.primary_group.equals(&group));
    let denied = sid::with_subs([0, 0, 0, 0, 0, 1], 0, 0, 0, 0, 0, 0, 0, 0);
    let pointer = &denied as *const sid as u64;
    assert_eq!(
        unsafe { nt_set_information_token(handle, 5, &pointer as *const u64 as PVOID, 8) },
        0xc000_005bu32 as i32
    );
    assert!(token.primary_group.equals(&group));
    assert!(free_handle(handle));
    let handle = source(&mut token, 8);
    assert_eq!(
        unsafe { nt_set_information_token(handle, 5, &pointer as *const u64 as PVOID, 8) },
        STATUS_ACCESS_DENIED
    );
    assert!(free_handle(handle));
}

#[test]
fn session_assignment_requires_the_callers_tcb_privilege_and_session_access() {
    let _guard = state_guard();
    let mut token = Token::new();
    token.add_privilege(TokenPrivilege::new(luid::from_u64(7), SE_PRIVILEGE_ENABLED));
    let handle = source(&mut token, 0x108);
    let session = 42_u32;
    let call =
        || unsafe { nt_set_information_token(handle, 12, &session as *const u32 as PVOID, 4) };
    // A privilege in the target token must not authorize an unrelated caller.
    assert_eq!(call(), STATUS_PRIVILEGE_NOT_HELD);
    assert_eq!(token.session_id, 0);
    crate::ntdll::privilege::ENABLED.with(|current| current.set(true));
    assert_eq!(call(), STATUS_SUCCESS);
    assert_eq!(token.session_id, session);
    crate::ntdll::privilege::ENABLED.with(|current| current.set(false));
    assert!(free_handle(handle));
}

#[test]
fn unsupported_setters_no_longer_report_success_without_updating_anything() {
    let _guard = state_guard();
    let mut token = Token::new();
    let handle = source(&mut token, 0x1ff);
    let mut input = [0_u64; 2];
    for class in [4, 6, 14, 16, 17, 19, 23, 24, 25, 26, 27] {
        assert_eq!(
            unsafe { nt_set_information_token(handle, class, input.as_mut_ptr().cast(), 16) },
            STATUS_NOT_SUPPORTED
        );
    }
    assert_eq!(
        unsafe { nt_set_information_token(handle, 3, input.as_mut_ptr().cast(), 16) },
        STATUS_INVALID_INFO_CLASS
    );
    assert_eq!(
        unsafe { nt_set_information_token(handle, 12, input.as_mut_ptr().cast(), 3) },
        STATUS_INFO_LENGTH_MISMATCH
    );
    assert!(free_handle(handle));
}

#[test]
fn trusted_query_relocates_sid_pointers_to_the_supplied_slice() {
    let _guard = state_guard();
    let mut token = Token::new_primary(SID_USERS);
    let handle = source(&mut token, 8);
    let mut output = [0_u8; 64];
    let mut length = 0;
    assert_eq!(
        nt_query_information_token_kernel(handle, 5, Some(&mut output), &mut length),
        STATUS_SUCCESS
    );
    assert_eq!(length, 24);
    assert_eq!(u64_at(&output, 0), output.as_ptr() as u64 + 8);
    assert!(sid::from_bytes(&output[8..24]).unwrap().equals(&SID_USERS));
    assert!(free_handle(handle));
}
