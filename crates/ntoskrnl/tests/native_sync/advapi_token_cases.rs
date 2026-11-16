use super::{advapi_token_calls::*, kernel32::error, state_guard};
use crate::ntdll::file::{alloc_access_handle, free_handle, HandleKind};
use crate::se::token::*;
use core::ptr;

#[test]
fn win32_token_query_returns_native_values_and_sets_last_error_only_on_failure() {
    let _guard = state_guard();
    let mut token = Token::new();
    token.session_id = 39;
    let handle =
        alloc_access_handle(HandleKind::Token, &mut token as *mut _ as u64, 8, false).unwrap();
    let mut length = 99;
    error::set_last_error(1234);
    assert_eq!(
        unsafe { GetTokenInformation(handle, 12, ptr::null_mut(), 0, &mut length) },
        0
    );
    assert_eq!(length, 4);
    assert_eq!(error::get_last_error(), 122);
    let mut session = 0u32;
    error::set_last_error(1234);
    assert_eq!(
        unsafe {
            GetTokenInformation(
                handle,
                12,
                (&mut session as *mut u32).cast(),
                4,
                &mut length,
            )
        },
        1
    );
    assert_eq!(session, 39);
    assert_eq!(error::get_last_error(), 1234);
    assert!(free_handle(handle));
    assert_eq!(
        unsafe {
            GetTokenInformation(
                handle,
                12,
                (&mut session as *mut u32).cast(),
                4,
                &mut length,
            )
        },
        0
    );
    assert_eq!(error::get_last_error(), 6);
}

#[test]
fn win32_privilege_check_writes_full_bool_and_preserves_result_on_failure() {
    let _guard = state_guard();
    let mut token = Token::new();
    token.add_privilege(TokenPrivilege::new(
        luid::from_u64(17),
        SE_PRIVILEGE_ENABLED,
    ));
    let handle =
        alloc_access_handle(HandleKind::Token, &mut token as *mut _ as u64, 8, false).unwrap();
    let mut required = [1u32, 1, 17, 0, 0];
    let mut result = -1;
    error::set_last_error(1234);
    assert_eq!(
        unsafe { PrivilegeCheck(handle, required.as_mut_ptr().cast(), &mut result) },
        1
    );
    assert_eq!(result, 1);
    assert_eq!(required[4], SE_PRIVILEGE_USED_FOR_ACCESS);
    assert_eq!(error::get_last_error(), 1234);
    required[2] = 18;
    required[4] = 0;
    assert_eq!(
        unsafe { PrivilegeCheck(handle, required.as_mut_ptr().cast(), &mut result) },
        1
    );
    assert_eq!(result, 0);
    assert_eq!(error::get_last_error(), 1234);
    assert!(free_handle(handle));
    result = -1;
    assert_eq!(
        unsafe { PrivilegeCheck(handle, required.as_mut_ptr().cast(), &mut result) },
        0
    );
    assert_eq!(result, -1);
    assert_eq!(error::get_last_error(), 6);
}

#[test]
fn win32_set_token_information_propagates_access_and_privilege_errors() {
    let _guard = state_guard();
    let mut token = Token::new();
    let handle =
        alloc_access_handle(HandleKind::Token, &mut token as *mut _ as u64, 0x100, false).unwrap();
    let mut session = 42u32;
    crate::ntdll::privilege::ENABLED.with(|value| value.set(false));
    assert_eq!(
        unsafe { SetTokenInformation(handle, 12, (&mut session as *mut u32).cast(), 4) },
        0
    );
    assert_eq!(error::get_last_error(), 1314);
    assert_eq!(token.session_id, 0);
    crate::ntdll::privilege::ENABLED.with(|value| value.set(true));
    assert_eq!(
        unsafe { SetTokenInformation(handle, 12, (&mut session as *mut u32).cast(), 4) },
        1
    );
    assert_eq!(token.session_id, 42);
    crate::ntdll::privilege::ENABLED.with(|value| value.set(false));
    assert!(free_handle(handle));
}

#[test]
fn win32_adjustment_distinguishes_not_all_assigned_and_normalizes_bool() {
    let _guard = state_guard();
    let mut token = Token::new();
    token.add_privilege(TokenPrivilege::new(
        luid::from_u64(17),
        SE_PRIVILEGE_ENABLED,
    ));
    let handle =
        alloc_access_handle(HandleKind::Token, &mut token as *mut _ as u64, 0x20, false).unwrap();
    let request = [1u32, 99, 0, SE_PRIVILEGE_ENABLED];
    assert_eq!(
        unsafe {
            AdjustTokenPrivileges(
                handle,
                0,
                request.as_ptr().cast(),
                0,
                ptr::null_mut(),
                ptr::null_mut(),
            )
        },
        1
    );
    assert_eq!(error::get_last_error(), 1300);
    assert_eq!(
        unsafe {
            AdjustTokenPrivileges(
                handle,
                256,
                ptr::null(),
                0,
                ptr::null_mut(),
                ptr::null_mut(),
            )
        },
        1
    );
    assert_eq!(error::get_last_error(), 0);
    assert!(!token.has_privilege(&luid::from_u64(17)));
    assert!(free_handle(handle));
}
