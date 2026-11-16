use super::{state_guard, ntdll::{file::*, status::*, types::*, token_filter::nt_filter_token,
    token_groups::{SidAndAttributes, TOKEN_GROUPS}, token_privileges::{LuidAndAttributes, TOKEN_PRIVILEGES}}};
use crate::se::{acl::Acl, sid::{sid, WellKnownSid, SID_USERS}, token::*};
use crate::mm::pool;
use core::{ptr, sync::atomic::Ordering};

fn source(token: &mut Token, access: u32) -> HANDLE {
    alloc_access_handle(HandleKind::Token, token as *mut _ as u64, access, false).unwrap()
}
fn inspect<T>(handle: HANDLE, f: impl FnOnce(&Token) -> T) -> T {
    with_sync_access(handle, 8, |entries| Ok(f(unsafe { &*(entries[0].target as *const Token) }))).unwrap()
}
fn groups(sid: &sid, attributes: u32) -> TOKEN_GROUPS {
    TOKEN_GROUPS { group_count: 1, groups: [SidAndAttributes { sid: (sid as *const sid).cast(), attributes }] }
}
fn filter(source: HANDLE, flags: u32, disable: Option<&TOKEN_GROUPS>,
    delete: Option<&TOKEN_PRIVILEGES>, restrict: Option<&TOKEN_GROUPS>) -> Result<HANDLE, i32>
{
    let mut output = ptr::null_mut();
    let status = unsafe { nt_filter_token(source, flags, disable.map_or(ptr::null(), |p| p),
        delete.map_or(ptr::null(), |p| p), restrict.map_or(ptr::null(), |p| p), &mut output) };
    if status == STATUS_SUCCESS { Ok(output) } else { Err(status) }
}

#[test]
fn filtered_token_owns_dacl_and_preserves_source_rights_type_and_identity() {
    let _guard = state_guard();
    let baseline = pool::LIVE.load(Ordering::Relaxed);
    let mut token = Token::new_impersonation(SID_USERS, ImpersonationLevel::SecurityIdentification);
    let mut acl = Acl::new();
    token.default_dacl = &mut acl;
    token.token_id = luid::from_u64(17);
    token.authentication_id = luid::from_u64(31);
    token.add_group(TokenGroup::new(SID_USERS, SE_GROUP_ENABLED | SE_GROUP_ENABLED_BY_DEFAULT));
    for number in [17, 23, 29] { token.add_privilege(TokenPrivilege::new(luid::from_u64(number), SE_PRIVILEGE_ENABLED)); }
    token.refresh_privilege_flags();
    let original = source(&mut token, 0x2a);
    let disable = groups(&SID_USERS, 0xffffffff); // disabling attributes are ignored
    let restricted_sid = sid::well_known(WellKnownSid::World);
    let copy = filter(original, 3, Some(&disable), None, Some(&groups(&restricted_sid, 0))).unwrap();
    inspect(copy, |value| {
        assert_ne!(value.token_id, token.token_id);
        assert_eq!(value.authentication_id, token.authentication_id);
        assert_eq!(value.token_type, token.token_type);
        assert_eq!(value.impersonation_level, token.impersonation_level);
        assert_ne!(value.default_dacl, token.default_dacl);
        assert_eq!(value.user_attributes, SE_GROUP_USE_FOR_DENY_ONLY);
        assert_eq!(value.groups[0].attributes, SE_GROUP_USE_FOR_DENY_ONLY);
        assert_eq!(value.group_count, 1);
        assert_eq!(value.restricted_count, 1);
        assert!(value.restricted_groups[0].sid.equals(&restricted_sid));
        assert_eq!(value.restricted_groups[0].attributes, 7);
        assert_eq!(value.privilege_count, 1);
        assert_eq!(value.privileges[0].luid.to_u64(), 23);
        assert_eq!(value.flags & (TOKEN_HAS_BACKUP_PRIVILEGE | TOKEN_HAS_IMPERSONATE_PRIVILEGE), 0);
        assert_ne!(value.flags & TOKEN_IS_RESTRICTED, 0);
        assert_ne!(value.flags & TOKEN_SANDBOX_INERT, 0);
    });
    assert_eq!(with_sync_access(copy, 0x40, |_| Ok(())), Err(STATUS_ACCESS_DENIED));
    assert_eq!(token.user_attributes, 0);
    assert_eq!(token.privilege_count, 3);
    assert_eq!(token.restricted_count, 0);
    assert!(free_handle(original));
    assert!(free_handle(copy));
    assert_eq!(pool::LIVE.load(Ordering::Relaxed), baseline);
}

#[test]
fn deleted_privileges_are_removed_and_missing_requests_do_not_add_membership() {
    let _guard = state_guard();
    let mut token = Token::new_primary(SID_USERS);
    token.add_privilege(TokenPrivilege::new(luid::from_u64(17), SE_PRIVILEGE_ENABLED));
    let original = source(&mut token, 0x2a);
    let delete = TOKEN_PRIVILEGES { privilege_count: 1, privileges: [LuidAndAttributes { luid: 17, attributes: 0xffffffff }] };
    let missing = sid::well_known(WellKnownSid::World);
    let copy = filter(original, 0, Some(&groups(&missing, 0)), Some(&delete), None).unwrap();
    inspect(copy, |value| {
        assert_eq!(value.privilege_count, 0);
        assert_eq!(value.group_count, 0);
        assert_eq!(value.user_attributes, 0);
        assert_eq!(value.flags & TOKEN_IS_RESTRICTED, 0);
    });
    for handle in [copy, original] { assert!(free_handle(handle)); }
}

#[test]
fn refilter_intersects_restricting_sids_and_does_not_clear_an_empty_intersection() {
    let _guard = state_guard();
    let mut token = Token::new_primary(SID_USERS);
    let original = source(&mut token, 0xa);
    let first = filter(original, 0, None, None, Some(&groups(&SID_USERS, 0))).unwrap();
    let preserved = filter(first, 0, None, None, None).unwrap();
    inspect(preserved, |value| assert_eq!(value.restricted_count, 1));
    let other = sid::well_known(WellKnownSid::World);
    let second = filter(first, 0, None, None, Some(&groups(&other, 0))).unwrap();
    inspect(second, |value| {
        assert_eq!(value.restricted_count, 0);
        assert_ne!(value.flags & TOKEN_IS_RESTRICTED, 0);
    });
    for handle in [original, first, second, preserved] { assert!(free_handle(handle)); }
}

#[test]
fn filter_rejects_bad_attributes_rights_and_policy_without_publishing_a_handle() {
    let _guard = state_guard();
    let mut token = Token::new_primary(SID_USERS);
    let original = source(&mut token, 0xa);
    assert_eq!(filter(original, 0x10, None, None, None), Err(STATUS_INVALID_PARAMETER));
    for flag in [4, 8] { assert_eq!(filter(original, flag, None, None, None), Err(STATUS_NOT_SUPPORTED)); }
    assert_eq!(filter(original, 0, None, None, Some(&groups(&SID_USERS, 1))), Err(STATUS_INVALID_PARAMETER));
    let limited = source(&mut token, 8);
    assert_eq!(filter(limited, 0, None, None, None), Err(STATUS_ACCESS_DENIED));
    let event = alloc_handle(HandleKind::Event, 0);
    assert_eq!(filter(event, 0, None, None, None), Err(STATUS_OBJECT_TYPE_MISMATCH));
    for handle in [original, limited, event] { assert!(free_handle(handle)); }
}

#[test]
fn filter_copy_faults_and_allocation_failures_rollback_all_owned_memory() {
    let _guard = state_guard();
    let baseline = pool::LIVE.load(Ordering::Relaxed);
    let mut token = Token::new_primary(SID_USERS);
    let mut acl = Acl::new();
    token.default_dacl = &mut acl;
    let original = source(&mut token, 0xa);
    let handles = handle_count();
    let mut output = ptr::null_mut();
    crate::mm::user_copy::FAIL_WRITE.with(|v| v.set(&mut output as *mut HANDLE as u64));
    assert_eq!(unsafe { nt_filter_token(original, 0, ptr::null(), ptr::null(), ptr::null(), &mut output) }, STATUS_ACCESS_VIOLATION);
    crate::mm::user_copy::FAIL_WRITE.with(|v| v.set(0));
    assert_eq!(handle_count(), handles);
    for allocations in [0, 1] {
        pool::FAIL_AFTER.with(|v| v.set(Some(allocations)));
        assert_eq!(filter(original, 0, None, None, None), Err(STATUS_NO_MEMORY));
        pool::FAIL_AFTER.with(|v| v.set(None));
        assert_eq!(pool::LIVE.load(Ordering::Relaxed), baseline);
    }
    assert_eq!(unsafe { nt_filter_token(original, 0, ptr::null(), ptr::null(), ptr::null(), ptr::null_mut()) }, STATUS_ACCESS_VIOLATION);
    assert!(free_handle(original));
}

#[test]
fn queries_encode_distinct_restricting_sids_and_filter_flags() {
    let _guard = state_guard();
    let mut token = Token::new_primary(SID_USERS);
    let original = source(&mut token, 0xa);
    let copy = filter(original, 2, None, None, Some(&groups(&SID_USERS, 0))).unwrap();
    let mut bytes = [0u8; 128];
    let mut length = 0;
    use crate::ntdll::token_information::nt_query_information_token_kernel as query;
    assert_eq!(query(copy, 11, Some(&mut bytes), &mut length), STATUS_SUCCESS);
    assert_eq!(u32::from_le_bytes(bytes[..4].try_into().unwrap()), 1);
    assert_eq!(u64::from_le_bytes(bytes[8..16].try_into().unwrap()), bytes.as_ptr() as u64 + 24);
    assert_eq!(u32::from_le_bytes(bytes[16..20].try_into().unwrap()), 7);
    for class in [15, 21] {
        assert_eq!(query(copy, class, Some(&mut bytes), &mut length), STATUS_SUCCESS);
        assert_eq!(u32::from_le_bytes(bytes[..4].try_into().unwrap()), 1);
    }
    for handle in [copy, original] { assert!(free_handle(handle)); }
}
