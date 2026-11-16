use super::state_guard;
use crate::ntdll::file::{alloc_access_handle, free_handle, HandleKind};
use crate::ntdll::status::*;
use crate::ntdll::token_privileges::*;
use crate::ntdll::types::HANDLE;
use crate::se::token::*;

fn source(token: &mut Token, access: u32) -> HANDLE {
    alloc_access_handle(HandleKind::Token, token as *mut Token as u64, access, false).unwrap()
}

fn packet(entries: &[(u64, u32)], control: Option<u32>) -> Vec<u32> {
    let mut words = vec![entries.len() as u32];
    if let Some(control) = control {
        words.push(control);
    }
    for &(id, attributes) in entries {
        words.extend([id as u32, (id >> 32) as u32, attributes]);
    }
    words
}

fn token() -> Token {
    let mut token = Token::new();
    token.add_privilege(TokenPrivilege::new(
        luid::from_u64(17),
        SE_PRIVILEGE_ENABLED_BY_DEFAULT | SE_PRIVILEGE_ENABLED,
    ));
    token.add_privilege(TokenPrivilege::new(
        luid::from_u64(18),
        SE_PRIVILEGE_ENABLED_BY_DEFAULT,
    ));
    token.add_privilege(TokenPrivilege::new(
        luid::from_u64(23),
        SE_PRIVILEGE_ENABLED,
    ));
    token
}

#[test]
fn native_privilege_layout_uses_twelve_byte_entries_and_four_byte_alignment() {
    assert_eq!(core::mem::size_of::<LuidAndAttributes>(), 12);
    assert_eq!(core::mem::align_of::<LuidAndAttributes>(), 4);
    assert_eq!(core::mem::size_of::<TOKEN_PRIVILEGES>(), 16);
    assert_eq!(core::mem::offset_of!(TOKEN_PRIVILEGES, privileges), 4);
    assert_eq!(core::mem::size_of::<PrivilegeSet>(), 20);
    assert_eq!(core::mem::offset_of!(PrivilegeSet, privilege), 8);
}

#[test]
fn privilege_check_evaluates_all_or_any_and_marks_only_enabled_matches() {
    let _guard = state_guard();
    let mut token = token();
    let handle = source(&mut token, 8);
    for (control, expected) in [(0, 1), (1, 0)] {
        let mut required = packet(&[(17, 1), (18, 0), (99, 0)], Some(control));
        let mut result = 9;
        assert_eq!(
            unsafe { nt_privilege_check(handle, required.as_mut_ptr().cast(), &mut result) },
            STATUS_SUCCESS
        );
        assert_eq!(result, expected);
        assert_eq!(required[4], 1 | SE_PRIVILEGE_USED_FOR_ACCESS);
        assert_eq!(required[7], 0);
        assert_eq!(required[10], 0);
    }
    let mut required = packet(&[(17, 0), (23, 0)], Some(1));
    let mut result = 0;
    assert_eq!(
        unsafe { nt_privilege_check(handle, required.as_mut_ptr().cast(), &mut result) },
        STATUS_SUCCESS
    );
    assert_eq!(result, 1);
    assert!(free_handle(handle));
}

#[test]
fn privilege_check_handles_empty_sets_access_denial_and_anonymous_tokens() {
    let _guard = state_guard();
    let mut token = token();
    let handle = source(&mut token, 8);
    for (control, expected) in [(0, 0), (1, 1)] {
        let mut required = packet(&[], Some(control));
        let mut result = 9;
        assert_eq!(
            unsafe { nt_privilege_check(handle, required.as_mut_ptr().cast(), &mut result) },
            STATUS_SUCCESS
        );
        assert_eq!(result, expected);
    }
    let mut required = packet(&[(17, 0)], Some(1));
    let no_query = source(&mut token, 2);
    let mut result = 9;
    assert_eq!(
        unsafe { nt_privilege_check(no_query, required.as_mut_ptr().cast(), &mut result) },
        STATUS_ACCESS_DENIED
    );
    assert_eq!(result, 9);
    assert!(free_handle(no_query));
    assert!(free_handle(handle));
    token.token_type = TokenTypeEnum::TokenImpersonation;
    token.impersonation_level = ImpersonationLevel::SecurityAnonymous;
    let handle = source(&mut token, 8);
    assert_eq!(
        unsafe { nt_privilege_check(handle, required.as_mut_ptr().cast(), &mut result) },
        STATUS_BAD_IMPERSONATION_LEVEL
    );
    assert!(free_handle(handle));
}

#[test]
fn adjustment_captures_multiple_requests_and_returns_only_changed_previous_values() {
    let _guard = state_guard();
    let mut token = token();
    let handle = source(&mut token, 0x28);
    let requested = packet(
        &[
            (17, 0),
            (18, SE_PRIVILEGE_ENABLED),
            (23, SE_PRIVILEGE_ENABLED),
        ],
        None,
    );
    let mut previous = [0xcc_u32; 10];
    let mut length = 0;
    assert_eq!(
        unsafe {
            nt_adjust_privileges_token(
                handle,
                0,
                requested.as_ptr().cast(),
                40,
                previous.as_mut_ptr().cast(),
                &mut length,
            )
        },
        STATUS_SUCCESS
    );
    assert_eq!(length, 28);
    assert_eq!(previous[..7], [2, 17, 0, 3, 18, 0, 1]);
    assert_eq!(previous[7], 0xcc);
    assert_eq!(
        token.privileges[0].attributes,
        SE_PRIVILEGE_ENABLED_BY_DEFAULT
    );
    assert_eq!(
        token.privileges[1].attributes,
        SE_PRIVILEGE_ENABLED_BY_DEFAULT | SE_PRIVILEGE_ENABLED
    );
    assert_eq!(token.flags & TOKEN_HAS_BACKUP_PRIVILEGE, 0);
    assert_ne!(token.flags & TOKEN_HAS_RESTORE_PRIVILEGE, 0);
    assert!(free_handle(handle));
}

#[test]
fn removal_is_irreversible_and_missing_privileges_return_informational_status() {
    let _guard = state_guard();
    let mut token = token();
    let handle = source(&mut token, 0x28);
    let requested = packet(
        &[(17, SE_PRIVILEGE_REMOVED), (99, SE_PRIVILEGE_ENABLED)],
        None,
    );
    let mut previous = [0xcc_u32; 10];
    let mut length = 0;
    assert_eq!(
        unsafe {
            nt_adjust_privileges_token(
                handle,
                0,
                requested.as_ptr().cast(),
                40,
                previous.as_mut_ptr().cast(),
                &mut length,
            )
        },
        STATUS_NOT_ALL_ASSIGNED
    );
    assert_eq!(length, 4);
    assert_eq!(previous[0], 0);
    assert_eq!(token.privilege_count, 2);
    assert_eq!(token.privileges[0].luid.to_u64(), 18);
    let requested = packet(&[(17, SE_PRIVILEGE_ENABLED)], None);
    assert_eq!(
        unsafe {
            nt_adjust_privileges_token(
                handle,
                0,
                requested.as_ptr().cast(),
                0,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
            )
        },
        STATUS_NOT_ALL_ASSIGNED
    );
    assert!(!token.has_privilege(&luid::from_u64(17)));
    assert_eq!(
        crate::ntdll::status::nt_status_to_dos_error(STATUS_NOT_ALL_ASSIGNED),
        1300
    );
    assert!(free_handle(handle));
}

#[test]
fn insufficient_previous_buffer_reports_length_without_changing_token() {
    let _guard = state_guard();
    let mut token = token();
    let handle = source(&mut token, 0x28);
    let mut previous = [0xcc_u32; 1];
    let mut length = 0;
    assert_eq!(
        unsafe {
            nt_adjust_privileges_token(
                handle,
                1,
                core::ptr::null(),
                4,
                previous.as_mut_ptr().cast(),
                &mut length,
            )
        },
        STATUS_BUFFER_TOO_SMALL
    );
    assert_eq!(length, 28);
    assert_eq!(previous[0], 0xcc);
    assert!(token.has_privilege(&luid::from_u64(17)));
    assert!(token.has_privilege(&luid::from_u64(23)));
    let mut previous = [0xcc_u32; 7];
    assert_eq!(
        unsafe {
            nt_adjust_privileges_token(
                handle,
                1,
                core::ptr::null(),
                28,
                previous.as_mut_ptr().cast(),
                &mut length,
            )
        },
        STATUS_SUCCESS
    );
    assert_eq!(previous, [2, 17, 0, 3, 23, 0, 2]);
    assert!(token.privileges[..token.privilege_count]
        .iter()
        .all(|privilege| !privilege.is_enabled()));
    assert!(free_handle(handle));
}

#[test]
fn output_faults_and_missing_adjust_right_do_not_change_privileges() {
    let _guard = state_guard();
    let mut token = token();
    let query_only = source(&mut token, 8);
    assert_eq!(
        unsafe {
            nt_adjust_privileges_token(
                query_only,
                1,
                core::ptr::null(),
                0,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
            )
        },
        STATUS_ACCESS_DENIED
    );
    assert!(free_handle(query_only));
    let handle = source(&mut token, 0x28);
    let mut previous = [0_u32; 7];
    let mut length = 0;
    crate::mm::user_copy::FAIL_WRITE.with(|address| address.set(previous.as_mut_ptr() as u64));
    assert_eq!(
        unsafe {
            nt_adjust_privileges_token(
                handle,
                1,
                core::ptr::null(),
                28,
                previous.as_mut_ptr().cast(),
                &mut length,
            )
        },
        STATUS_ACCESS_VIOLATION
    );
    crate::mm::user_copy::FAIL_WRITE.with(|address| address.set(0));
    assert!(token.has_privilege(&luid::from_u64(17)));
    assert!(token.has_privilege(&luid::from_u64(23)));
    assert!(free_handle(handle));
}
