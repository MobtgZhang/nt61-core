use super::{advapi_token_calls, kernel32::error, state_guard};
use crate::ntdll::access_check::*;
use crate::ntdll::file::{alloc_access_handle, free_handle, HandleKind};
use crate::ntdll::status::*;
use crate::ntdll::types::HANDLE;
use crate::se::{
    sid::{sid, WellKnownSid},
    token::*,
};
use core::ptr;

const MAXIMUM: u32 = 0x0200_0000;
const SECURITY: u32 = 0x0100_0000;
const OWNER: u32 = 0x0008_0000;
const READ_CONTROL: u32 = 0x0002_0000;
const WRITE_DAC: u32 = 0x0004_0000;

fn mapping() -> GenericMapping {
    GenericMapping {
        generic_read: 1,
        generic_write: 2,
        generic_execute: 4,
        generic_all: 7,
    }
}

fn token() -> Token {
    let mut token = Token::new();
    token.user = sid::well_known(WellKnownSid::LocalSystem);
    token.token_type = TokenTypeEnum::TokenImpersonation;
    token.impersonation_level = ImpersonationLevel::SecurityImpersonation;
    token
}

fn handle(token: &mut Token, access: u32) -> HANDLE {
    alloc_access_handle(HandleKind::Token, token as *mut _ as u64, access, false).unwrap()
}

fn sid_bytes(value: sid) -> Vec<u8> {
    let mut bytes = vec![value.revision, value.sub_authority_count];
    bytes.extend(value.identifier_authority);
    for number in &value.sub_authority[..value.sub_authority_count as usize] {
        bytes.extend(number.to_le_bytes());
    }
    bytes
}

fn acl(entries: &[(u8, u8, u32, sid)]) -> Vec<u8> {
    let mut bytes = vec![2, 0, 8, 0, entries.len() as u8, 0, 0, 0];
    for (kind, flags, mask, identity) in entries {
        let identity = sid_bytes(*identity);
        bytes.extend([*kind, *flags]);
        bytes.extend(((8 + identity.len()) as u16).to_le_bytes());
        bytes.extend(mask.to_le_bytes());
        bytes.extend(identity);
    }
    let length = bytes.len() as u16;
    bytes[2..4].copy_from_slice(&length.to_le_bytes());
    bytes
}

fn descriptor(owner: &sid, dacl: Option<&[u8]>, present: bool) -> SecurityDescriptor {
    SecurityDescriptor {
        revision: 1,
        sbz1: 0,
        control: if present { 4 } else { 0 },
        owner: (owner as *const sid).cast_mut().cast(),
        group: (owner as *const sid).cast_mut().cast(),
        sacl: ptr::null_mut(),
        dacl: dacl.map_or(ptr::null_mut(), |bytes| bytes.as_ptr().cast_mut().cast()),
    }
}

fn call(
    descriptor: *const SecurityDescriptor,
    handle: HANDLE,
    desired: u32,
) -> (i32, u32, i32, [u32; 8]) {
    let mut privileges = [0xccu32; 8];
    let mut length = 32;
    let mut granted = 0xcccccccc;
    let mut status = 0xccccccccu32 as i32;
    let result = unsafe {
        nt_access_check(
            descriptor,
            handle,
            desired,
            &mapping(),
            privileges.as_mut_ptr().cast(),
            &mut length,
            &mut granted,
            &mut status,
        )
    };
    assert_eq!(length, 32);
    (result, granted, status, privileges)
}

#[test]
fn absent_and_null_dacl_grant_but_empty_dacl_denies() {
    let _guard = state_guard();
    let mut token = token();
    let handle = handle(&mut token, 8);
    let other = sid::well_known(WellKnownSid::World);
    for present in [false, true] {
        let sd = descriptor(&other, None, present);
        let (result, granted, status, privileges) = call(&sd, handle, 3);
        assert_eq!(
            (result, granted, status),
            (STATUS_SUCCESS, 3, STATUS_SUCCESS)
        );
        assert_eq!(privileges[..2], [0, 0]);
        let (_, granted, status, _) = call(&sd, handle, MAXIMUM);
        assert_eq!((granted, status), (7, STATUS_SUCCESS));
    }
    let empty = acl(&[]);
    let sd = descriptor(&other, Some(&empty), true);
    let (result, granted, status, _) = call(&sd, handle, 1);
    assert_eq!(
        (result, granted, status),
        (STATUS_SUCCESS, 0, STATUS_ACCESS_DENIED)
    );
    assert!(free_handle(handle));
}

#[test]
fn ordered_aces_do_not_revoke_rights_already_granted() {
    let _guard = state_guard();
    let mut token = token();
    let user = token.user;
    let handle = handle(&mut token, 8);
    let other = sid::well_known(WellKnownSid::World);
    for (entries, expected) in [
        (vec![(0, 0, 3, user), (1, 0, 1, user)], (3, STATUS_SUCCESS)),
        (
            vec![(1, 0, 1, user), (0, 0, 3, user)],
            (0, STATUS_ACCESS_DENIED),
        ),
    ] {
        let acl = acl(&entries);
        let sd = descriptor(&other, Some(&acl), true);
        let (_, granted, status, _) = call(&sd, handle, 3);
        assert_eq!((granted, status), expected);
    }
    assert!(free_handle(handle));
}

#[test]
fn maximum_allowed_respects_order_and_requires_explicit_extra_rights() {
    let _guard = state_guard();
    let mut token = token();
    let user = token.user;
    let handle = handle(&mut token, 8);
    let other = sid::well_known(WellKnownSid::World);
    let entries = acl(&[(1, 0, 1, user), (0, 0, 7, user)]);
    let sd = descriptor(&other, Some(&entries), true);
    assert_eq!(&call(&sd, handle, MAXIMUM).1, &6);
    let (_, granted, status, _) = call(&sd, handle, MAXIMUM | 1);
    assert_eq!((granted, status), (0, STATUS_ACCESS_DENIED));
    let entries = acl(&[(0, 0, 1, user), (1, 0, 1, user)]);
    let sd = descriptor(&other, Some(&entries), true);
    let (_, granted, status, _) = call(&sd, handle, MAXIMUM);
    assert_eq!((granted, status), (1, STATUS_SUCCESS));
    assert!(free_handle(handle));
}

#[test]
fn group_enablement_deny_only_and_inherit_only_are_honored() {
    let _guard = state_guard();
    let group = sid::well_known(WellKnownSid::World);
    for (attributes, allow, deny) in [
        (SE_GROUP_ENABLED, true, true),
        (0, false, false),
        (SE_GROUP_USE_FOR_DENY_ONLY, false, true),
    ] {
        let mut token = token();
        token.add_group(TokenGroup::new(group, attributes));
        let user = token.user;
        let handle = handle(&mut token, 8);
        let entries = acl(&[(0, 0, 1, group)]);
        let sd = descriptor(&group, Some(&entries), true);
        let (_, granted, _, _) = call(&sd, handle, 1);
        assert_eq!(granted == 1, allow);
        let entries = acl(&[(1, 0, 1, group), (0, 0, 1, user)]);
        let sd = descriptor(&group, Some(&entries), true);
        let (_, granted, _, _) = call(&sd, handle, 1);
        assert_eq!(granted == 0, deny);
        let entries = acl(&[(1, 8, 1, group), (0, 8, 2, user), (0, 0, 1, user)]);
        let sd = descriptor(&group, Some(&entries), true);
        let (_, granted, status, _) = call(&sd, handle, 1);
        assert_eq!((granted, status), (1, STATUS_SUCCESS));
        assert!(free_handle(handle));
    }
}

#[test]
fn owners_get_read_control_and_write_dac_without_acl_grants() {
    let _guard = state_guard();
    let mut token = token();
    let user = token.user;
    let handle = handle(&mut token, 8);
    let empty = acl(&[]);
    let sd = descriptor(&user, Some(&empty), true);
    let (_, granted, status, _) = call(&sd, handle, READ_CONTROL | WRITE_DAC);
    assert_eq!(
        (granted, status),
        (READ_CONTROL | WRITE_DAC, STATUS_SUCCESS)
    );
    assert!(free_handle(handle));
}

#[test]
fn security_and_take_ownership_privileges_are_reported_with_nt_layout() {
    let _guard = state_guard();
    let other = sid::well_known(WellKnownSid::World);
    let empty = acl(&[]);
    let sd = descriptor(&other, Some(&empty), true);
    let mut token = token();
    let no_privileges = handle(&mut token, 8);
    let (_, granted, status, _) = call(&sd, no_privileges, SECURITY);
    assert_eq!((granted, status), (0, STATUS_PRIVILEGE_NOT_HELD));
    assert!(free_handle(no_privileges));
    token.add_privilege(TokenPrivilege::new(luid::from_u64(8), SE_PRIVILEGE_ENABLED));
    token.add_privilege(TokenPrivilege::new(luid::from_u64(9), SE_PRIVILEGE_ENABLED));
    let handle = handle(&mut token, 8);
    let (_, granted, status, privileges) = call(&sd, handle, SECURITY | OWNER);
    assert_eq!((granted, status), (SECURITY | OWNER, STATUS_SUCCESS));
    assert_eq!(
        privileges,
        [
            2,
            0,
            8,
            0,
            SE_PRIVILEGE_USED_FOR_ACCESS,
            9,
            0,
            SE_PRIVILEGE_USED_FOR_ACCESS
        ]
    );
    assert!(free_handle(handle));
}

#[test]
fn generic_desired_rights_are_rejected_but_ace_masks_are_mapped() {
    let _guard = state_guard();
    let mut token = token();
    let user = token.user;
    let handle = handle(&mut token, 8);
    let other = sid::well_known(WellKnownSid::World);
    let entries = acl(&[(0, 0, 0x8000_0000, user)]);
    let sd = descriptor(&other, Some(&entries), true);
    let (_, granted, status, _) = call(&sd, handle, 1);
    assert_eq!((granted, status), (1, STATUS_SUCCESS));
    let (result, granted, status, _) = call(&sd, handle, 0x8000_0000);
    assert_eq!(result as u32, 0xc00000e6);
    assert_eq!(granted, 0xcccccccc);
    assert_eq!(status as u32, 0xcccccccc);
    assert!(free_handle(handle));
}

#[test]
fn small_privilege_buffers_report_required_size_without_changing_results() {
    let _guard = state_guard();
    let mut token = token();
    let handle = handle(&mut token, 8);
    let other = sid::well_known(WellKnownSid::World);
    let sd = descriptor(&other, None, true);
    let mut privileges = [0xccccccccu32; 8];
    for available in [0, 19] {
        let mut length = available;
        let mut granted = 99;
        let mut status = 99;
        assert_eq!(
            unsafe {
                nt_access_check(
                    &sd,
                    handle,
                    1,
                    &mapping(),
                    privileges.as_mut_ptr().cast(),
                    &mut length,
                    &mut granted,
                    &mut status,
                )
            },
            STATUS_BUFFER_TOO_SMALL
        );
        assert_eq!(length, 20);
        assert_eq!((granted, status, privileges[0]), (99, 99, 0xcccccccc));
    }
    assert!(free_handle(handle));
}

#[test]
fn access_check_rejects_primary_anonymous_and_handles_without_query_rights() {
    let _guard = state_guard();
    let other = sid::well_known(WellKnownSid::World);
    let sd = descriptor(&other, None, true);
    for (kind, level, access, expected) in [
        (
            TokenTypeEnum::TokenPrimary,
            ImpersonationLevel::SecurityImpersonation,
            8,
            0xc000005c,
        ),
        (
            TokenTypeEnum::TokenImpersonation,
            ImpersonationLevel::SecurityAnonymous,
            8,
            0xc00000a5,
        ),
        (
            TokenTypeEnum::TokenImpersonation,
            ImpersonationLevel::SecurityIdentification,
            2,
            0xc0000022,
        ),
    ] {
        let mut token = token();
        token.token_type = kind;
        token.impersonation_level = level;
        let handle = handle(&mut token, access);
        assert_eq!(call(&sd, handle, 1).0 as u32, expected);
        assert!(free_handle(handle));
    }
}

#[test]
fn relative_descriptors_use_bounded_offsets_in_trusted_helper() {
    let _guard = state_guard();
    let mut token = token();
    let handle = handle(&mut token, 8);
    let identity = sid_bytes(sid::well_known(WellKnownSid::World));
    let mut sd = vec![0u8; 20];
    sd[0] = 1;
    sd[2..4].copy_from_slice(&0x8004u16.to_le_bytes());
    sd[4..8].copy_from_slice(&20u32.to_le_bytes());
    sd[8..12].copy_from_slice(&(20u32 + identity.len() as u32).to_le_bytes());
    sd.extend(&identity);
    sd.extend(&identity);
    let mut granted = 99;
    let mut status = 99;
    assert_eq!(
        nt_access_check_kernel(
            Some(&sd),
            handle,
            3,
            Some(&mapping()),
            &mut granted,
            &mut status
        ),
        STATUS_SUCCESS
    );
    assert_eq!((granted, status), (3, STATUS_SUCCESS));
    assert_eq!(call(sd.as_ptr().cast(), handle, 3).1, 3);
    sd[4..8].copy_from_slice(&0xfffffffcu32.to_le_bytes());
    assert_eq!(
        nt_access_check_kernel(
            Some(&sd),
            handle,
            3,
            Some(&mapping()),
            &mut granted,
            &mut status
        ) as u32,
        0xc0000079
    );
    assert!(free_handle(handle));
}

#[test]
fn malformed_descriptors_aces_and_output_pointers_never_grant() {
    let _guard = state_guard();
    let mut token = token();
    let user = token.user;
    let handle = handle(&mut token, 8);
    let other = sid::well_known(WellKnownSid::World);
    let mut sd = descriptor(&other, None, true);
    sd.owner = ptr::null_mut();
    assert_eq!(call(&sd, handle, 1).0 as u32, 0xc0000079);
    let mut entries = acl(&[(0, 0, 1, user)]);
    entries[10..12].copy_from_slice(&0xfffcu16.to_le_bytes());
    let sd = descriptor(&other, Some(&entries), true);
    assert_eq!(call(&sd, handle, 1).0 as u32, 0xc0000077);
    let entries = acl(&[(9, 0, 1, user)]);
    let sd = descriptor(&other, Some(&entries), true);
    assert_eq!(call(&sd, handle, 1).0, STATUS_NOT_SUPPORTED);
    let sd = descriptor(&other, None, true);
    let mut length = 20;
    let mut granted = 99;
    let mut status = 99;
    assert_eq!(
        unsafe {
            nt_access_check(
                &sd,
                handle,
                1,
                &mapping(),
                ptr::null_mut(),
                &mut length,
                &mut granted,
                &mut status,
            )
        },
        STATUS_ACCESS_VIOLATION
    );
    assert_eq!((granted, status), (99, 99));
    assert!(free_handle(handle));
}

#[test]
fn win32_access_check_separates_api_failure_from_denied_access_and_last_error() {
    let _guard = state_guard();
    let mut token = token();
    let handle = handle(&mut token, 8);
    let other = sid::well_known(WellKnownSid::World);
    let empty = acl(&[]);
    let mut sd = descriptor(&other, Some(&empty), true);
    let mut privileges = [0u32; 5];
    let mut length = 20;
    let mut mapping = mapping();
    for (desired, expected, expected_granted, expected_status, expected_error) in [
        (1, 1, 0, 0, 5),
        (SECURITY, 1, 0, 0, 1314),
        (0x8000_0000, 0, 99, 99, 1360),
    ] {
        let mut granted = 99;
        let mut status = 99;
        error::set_last_error(1234);
        assert_eq!(
            unsafe {
                advapi_token_calls::AccessCheck(
                    &mut sd,
                    handle,
                    desired,
                    (&mut mapping as *mut GenericMapping).cast(),
                    privileges.as_mut_ptr().cast(),
                    &mut length,
                    &mut granted,
                    &mut status,
                )
            },
            expected
        );
        assert_eq!(
            (granted, status, error::get_last_error()),
            (expected_granted, expected_status, expected_error)
        );
    }
    let mut granted = 99;
    let mut status = 99;
    sd.dacl = ptr::null_mut();
    error::set_last_error(1234);
    assert_eq!(
        unsafe {
            advapi_token_calls::AccessCheck(
                &mut sd,
                handle,
                3,
                (&mut mapping as *mut GenericMapping).cast(),
                privileges.as_mut_ptr().cast(),
                &mut length,
                &mut granted,
                &mut status,
            )
        },
        1
    );
    assert_eq!((granted, status, error::get_last_error()), (3, 1, 1234));
    assert!(free_handle(handle));
}
