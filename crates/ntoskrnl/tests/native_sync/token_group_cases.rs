//! Production group capture, adjustment, rollback and Win32 error behavior.

use super::{advapi_token_calls::AdjustTokenGroups, kernel32::error, state_guard};
use crate::ntdll::file::{alloc_access_handle, free_handle, HandleKind};
use crate::ntdll::status::*;
use crate::ntdll::token_groups::*;
use crate::ntdll::types::HANDLE;
use crate::se::sid::{sid, WellKnownSid, SID_USERS};
use crate::se::token::*;
use core::ptr;

fn source(token: &mut Token, access: u32) -> HANDLE {
    alloc_access_handle(HandleKind::Token, token as *mut _ as u64, access, false).unwrap()
}

fn packet(entries: &[(&sid, u32)]) -> Vec<u64> {
    let mut words = vec![entries.len() as u64];
    for &(sid, attributes) in entries {
        words.push(sid as *const _ as u64);
        words.push(attributes as u64);
    }
    words
}

fn token() -> Token {
    let mut token = Token::new();
    token.add_group(TokenGroup::new(
        SID_USERS,
        SE_GROUP_ENABLED | SE_GROUP_OWNER,
    ));
    token.add_group(TokenGroup::new(
        sid::well_known(WellKnownSid::World),
        SE_GROUP_ENABLED_BY_DEFAULT,
    ));
    token
}

fn adjust(handle: HANDLE, request: &[u64]) -> i32 {
    unsafe {
        nt_adjust_groups_token(
            handle,
            0,
            request.as_ptr().cast(),
            0,
            ptr::null_mut(),
            ptr::null_mut(),
        )
    }
}

#[test]
fn nt64_group_layout_preserves_pointer_alignment() {
    assert_eq!(core::mem::size_of::<SidAndAttributes>(), 16);
    assert_eq!(core::mem::offset_of!(SidAndAttributes, attributes), 8);
    assert_eq!(core::mem::offset_of!(TOKEN_GROUPS, groups), 8);
    assert_eq!(core::mem::size_of::<TOKEN_GROUPS>(), 24);
}

#[test]
fn group_adjustment_only_changes_enabled_bit_and_never_adds_memberships() {
    let _guard = state_guard();
    let mut token = token();
    let handle = source(&mut token, 0x40);
    let world = sid::well_known(WellKnownSid::World);
    let unknown = sid::well_known(WellKnownSid::Administrators);
    let request = packet(&[
        (&SID_USERS, SE_GROUP_MANDATORY),
        (&world, SE_GROUP_ENABLED | SE_GROUP_USE_FOR_DENY_ONLY),
        (&unknown, SE_GROUP_ENABLED),
    ]);
    assert_eq!(adjust(handle, &request), STATUS_NOT_ALL_ASSIGNED);
    assert_eq!(token.group_count, 2);
    assert_eq!(token.groups[0].attributes, SE_GROUP_OWNER);
    assert_eq!(
        token.groups[1].attributes,
        SE_GROUP_ENABLED_BY_DEFAULT | SE_GROUP_ENABLED
    );
    assert!(free_handle(handle));
}

#[test]
fn previous_state_contains_only_changed_groups_with_relocated_sids_and_can_restore() {
    let _guard = state_guard();
    let mut token = token();
    let handle = source(&mut token, 0x48);
    let request = packet(&[(&SID_USERS, 0)]);
    let mut previous = [u64::MAX; 32];
    let mut length = 0;
    assert_eq!(
        unsafe {
            nt_adjust_groups_token(
                handle,
                0,
                request.as_ptr().cast(),
                256,
                previous.as_mut_ptr().cast(),
                &mut length,
            )
        },
        STATUS_SUCCESS
    );
    assert_eq!(length, 40); // 8-byte header + 16-byte entry + 16-byte SID.
    assert_eq!(previous[0], 1);
    assert_eq!(previous[1], previous.as_ptr() as u64 + 24);
    assert_eq!(previous[2], (SE_GROUP_ENABLED | SE_GROUP_OWNER) as u64);
    assert_eq!(previous[5], u64::MAX);
    assert_eq!(token.groups[0].attributes, SE_GROUP_OWNER);
    assert_eq!(adjust(handle, &previous), STATUS_SUCCESS);
    assert_eq!(
        token.groups[0].attributes,
        SE_GROUP_ENABLED | SE_GROUP_OWNER
    );
    // No actual change means an empty, eight-byte TOKEN_GROUPS header.
    assert_eq!(
        unsafe {
            nt_adjust_groups_token(
                handle,
                0,
                previous.as_ptr().cast(),
                256,
                previous.as_mut_ptr().cast(),
                &mut length,
            )
        },
        STATUS_SUCCESS
    );
    assert_eq!(length, 8);
    assert_eq!(previous[0], 0);
    assert!(free_handle(handle));
}

#[test]
fn undersized_previous_buffer_and_faulting_outputs_leave_groups_unchanged() {
    let _guard = state_guard();
    let mut token = token();
    let handle = source(&mut token, 0x48);
    let request = packet(&[(&SID_USERS, 0)]);
    let mut previous = [u64::MAX; 8];
    let mut length = 0;
    assert_eq!(
        unsafe {
            nt_adjust_groups_token(
                handle,
                0,
                request.as_ptr().cast(),
                39,
                previous.as_mut_ptr().cast(),
                &mut length,
            )
        },
        STATUS_BUFFER_TOO_SMALL
    );
    assert_eq!(length, 40);
    assert_eq!(previous, [u64::MAX; 8]);
    assert_eq!(
        token.groups[0].attributes,
        SE_GROUP_ENABLED | SE_GROUP_OWNER
    );
    for fault in [previous.as_ptr() as u64, &length as *const _ as u64] {
        crate::mm::user_copy::FAIL_WRITE.with(|value| value.set(fault));
        assert_eq!(
            unsafe {
                nt_adjust_groups_token(
                    handle,
                    0,
                    request.as_ptr().cast(),
                    64,
                    previous.as_mut_ptr().cast(),
                    &mut length,
                )
            },
            STATUS_ACCESS_VIOLATION
        );
        crate::mm::user_copy::FAIL_WRITE.with(|value| value.set(0));
        assert_eq!(
            token.groups[0].attributes,
            SE_GROUP_ENABLED | SE_GROUP_OWNER
        );
    }
    assert!(free_handle(handle));
}

#[test]
fn mandatory_and_deny_only_constraints_cannot_be_overwritten_by_request_attributes() {
    let _guard = state_guard();
    let mut token = token();
    token.groups[0].attributes |= SE_GROUP_MANDATORY;
    token.groups[1].attributes = SE_GROUP_USE_FOR_DENY_ONLY;
    let handle = source(&mut token, 0x40);
    assert_eq!(
        adjust(handle, &packet(&[(&SID_USERS, 0)])),
        STATUS_CANT_DISABLE_MANDATORY
    );
    assert!(token.groups[0].is_enabled());
    let world = sid::well_known(WellKnownSid::World);
    assert_eq!(
        adjust(handle, &packet(&[(&world, SE_GROUP_ENABLED)])),
        STATUS_CANT_ENABLE_DENY_ONLY
    );
    assert_eq!(token.groups[1].attributes, SE_GROUP_USE_FOR_DENY_ONLY);
    assert!(free_handle(handle));
}

#[test]
fn reset_restores_defaults_ignores_new_state_and_keeps_restricted_groups_disabled() {
    let _guard = state_guard();
    let mut token = token();
    token.add_group(TokenGroup::new(
        sid::well_known(WellKnownSid::Administrators),
        SE_GROUP_MANDATORY | SE_GROUP_ENABLED,
    ));
    token.add_group(TokenGroup::new(
        sid::well_known(WellKnownSid::LocalSystem),
        SE_GROUP_ENABLED_BY_DEFAULT | SE_GROUP_MANDATORY | SE_GROUP_USE_FOR_DENY_ONLY,
    ));
    let handle = source(&mut token, 0x40);
    assert_eq!(
        unsafe {
            nt_adjust_groups_token(handle, 255, 1usize as _, 0, ptr::null_mut(), 1usize as _)
        },
        STATUS_SUCCESS
    );
    assert_eq!(token.groups[0].attributes, SE_GROUP_OWNER);
    assert_eq!(
        token.groups[1].attributes,
        SE_GROUP_ENABLED_BY_DEFAULT | SE_GROUP_ENABLED
    );
    assert!(token.groups[2].is_enabled());
    assert!(!token.groups[3].is_enabled());
    assert!(free_handle(handle));
}

#[test]
fn adjustment_checks_access_type_handles_alignment_and_embedded_sid_validity() {
    let _guard = state_guard();
    let mut token = token();
    let request = packet(&[(&SID_USERS, 0)]);
    let no_adjust = source(&mut token, 8);
    assert_eq!(adjust(no_adjust, &request), STATUS_ACCESS_DENIED);
    assert!(free_handle(no_adjust));
    let handle = source(&mut token, 0x40);
    let mut previous = [0u64; 8];
    let mut length = 0;
    assert_eq!(
        unsafe {
            nt_adjust_groups_token(
                handle,
                0,
                request.as_ptr().cast(),
                64,
                previous.as_mut_ptr().cast(),
                &mut length,
            )
        },
        STATUS_ACCESS_DENIED
    );
    assert_eq!(
        unsafe {
            nt_adjust_groups_token(handle, 0, ptr::null(), 0, ptr::null_mut(), ptr::null_mut())
        },
        STATUS_INVALID_PARAMETER
    );
    assert_eq!(
        unsafe {
            nt_adjust_groups_token(
                handle,
                0,
                (request.as_ptr() as *const u8).add(1).cast(),
                0,
                ptr::null_mut(),
                ptr::null_mut(),
            )
        },
        0x8000_0002u32 as i32
    );
    let mut invalid = SID_USERS;
    invalid.revision = 2;
    assert_eq!(
        adjust(handle, &packet(&[(&invalid, 0)])),
        STATUS_INVALID_SID
    );
    let mut fault = request.clone();
    fault[1] = 0;
    assert_eq!(adjust(handle, &fault), STATUS_ACCESS_VIOLATION);
    assert!(free_handle(handle));
    assert_eq!(adjust(handle, &request), STATUS_INVALID_HANDLE);
    let wrong_type = alloc_access_handle(HandleKind::Event, 0, 0x40, false).unwrap();
    assert_eq!(adjust(wrong_type, &request), STATUS_OBJECT_TYPE_MISMATCH);
    assert!(free_handle(wrong_type));
}

#[test]
fn valid_fifteen_subauthority_sid_is_unknown_rather_than_truncated_to_existing_group() {
    let _guard = state_guard();
    let mut token = token();
    let handle = source(&mut token, 0x40);
    let mut long_sid = [0u32; 17];
    long_sid[0] = 0x0f01;
    long_sid[1] = 0x0500_0000;
    let request = [1u64, long_sid.as_ptr() as u64, SE_GROUP_ENABLED as u64];
    assert_eq!(adjust(handle, &request), STATUS_NOT_ALL_ASSIGNED);
    assert_eq!(token.group_count, 2);
    assert!(free_handle(handle));
}

#[test]
fn win32_group_adjustment_normalizes_bool_and_translates_native_failures() {
    let _guard = state_guard();
    let mut token = token();
    let handle = source(&mut token, 0x40);
    let unknown = sid::well_known(WellKnownSid::Administrators);
    let request = packet(&[(&unknown, SE_GROUP_ENABLED)]);
    assert_eq!(
        unsafe {
            AdjustTokenGroups(
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
    token.groups[0].attributes |= SE_GROUP_MANDATORY;
    let request = packet(&[(&SID_USERS, 0)]);
    assert_ne!(token.groups[0].attributes & SE_GROUP_MANDATORY, 0);
    assert_eq!(
        unsafe {
            AdjustTokenGroups(
                handle,
                0,
                request.as_ptr().cast(),
                0,
                ptr::null_mut(),
                ptr::null_mut(),
            )
        },
        0
    );
    assert_eq!(error::get_last_error(), 1310);
    assert_eq!(
        unsafe {
            AdjustTokenGroups(
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
    assert!(free_handle(handle));
}
