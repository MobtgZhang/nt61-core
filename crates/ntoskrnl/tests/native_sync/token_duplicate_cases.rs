use super::*;
use crate::mm::pool;
use crate::ntdll::file::*;
use crate::ntdll::token_duplicate::*;
use crate::se::{acl::Acl, sid::*, token::*};
use core::sync::atomic::Ordering;

const TOKEN_ALL_ACCESS: u32 = 0x000f_01ff;

fn source(token: &mut Token, access: u32) -> HANDLE {
    alloc_access_handle(HandleKind::Token, token as *mut Token as u64, access, false).unwrap()
}

fn inspect<T>(handle: HANDLE, operation: impl FnOnce(&Token) -> T) -> T {
    with_sync_access(handle, 8, |entries| {
        Ok(operation(unsafe { &*(entries[0].target as *const Token) }))
    })
    .unwrap()
}

#[test]
fn duplicate_has_independent_identity_groups_privileges_and_default_dacl() {
    let _guard = state_guard();
    let baseline = pool::LIVE.load(Ordering::Relaxed);
    let mut token = Token::new_primary(crate::se::sid::sid::well_known(WellKnownSid::LocalSystem));
    let mut acl = Acl::new();
    token.default_dacl = &mut acl;
    token.authentication_id = luid::from_u64(0x1234);
    token.add_group(TokenGroup::new(SID_USERS, SE_GROUP_ENABLED));
    token.add_privilege(TokenPrivilege::new(
        luid::from_u64(23),
        SE_PRIVILEGE_ENABLED,
    ));
    let original = source(&mut token, TOKEN_ALL_ACCESS);
    let first = duplicate_token(original, 0, 0, 2, true, Some(1)).unwrap();
    let second = duplicate_token(original, 8, 0, 1, false, None).unwrap();
    assert_eq!(handle_flags(first).unwrap(), 1);
    let id = inspect(first, |copy| {
        assert_eq!(copy.authentication_id, token.authentication_id);
        assert_eq!(copy.token_type, TokenTypeEnum::TokenImpersonation);
        assert_eq!(
            copy.impersonation_level,
            ImpersonationLevel::SecurityIdentification
        );
        assert_ne!(copy.default_dacl, token.default_dacl);
        assert_eq!(copy.refcount, 1);
        copy.token_id
    });
    inspect(second, |copy| {
        assert_ne!(copy.token_id, id);
        assert_eq!(
            copy.impersonation_level,
            ImpersonationLevel::SecurityImpersonation
        );
    });
    with_sync_access(first, 0x20, |entries| {
        let copy = unsafe { &mut *(entries[0].target as *mut Token) };
        copy.privileges[0].attributes = 0;
        unsafe {
            (*copy.default_dacl).revision = 4;
        }
        Ok(())
    })
    .unwrap();
    assert!(token.privileges[0].is_enabled());
    assert_eq!(acl.revision, 2);
    inspect(second, |copy| assert!(copy.privileges[0].is_enabled()));
    assert!(free_handle(original));
    assert!(free_handle(first));
    assert!(free_handle(second));
    assert_eq!(pool::LIVE.load(Ordering::Relaxed), baseline);
}

#[test]
fn effective_only_keeps_deny_and_integrity_groups_and_enabled_privileges() {
    let _guard = state_guard();
    let mut token = Token::new_primary(SID_USERS);
    for attributes in [
        0,
        SE_GROUP_ENABLED_BY_DEFAULT,
        SE_GROUP_ENABLED,
        SE_GROUP_USE_FOR_DENY_ONLY,
        SE_GROUP_INTEGRITY,
    ] {
        token.add_group(TokenGroup::new(SID_USERS, attributes));
    }
    for attributes in [0, SE_PRIVILEGE_ENABLED_BY_DEFAULT, SE_PRIVILEGE_ENABLED] {
        token.add_privilege(TokenPrivilege::new(luid::from_u64(23), attributes));
    }
    let original = source(&mut token, TOKEN_ALL_ACCESS);
    let copy = duplicate_token(original, 8, 1, 1, false, None).unwrap();
    inspect(copy, |value| {
        assert_eq!(value.group_count, 3);
        assert_eq!(value.privilege_count, 1);
        assert_eq!(value.groups[0].attributes, SE_GROUP_ENABLED);
        assert_eq!(value.groups[1].attributes, SE_GROUP_USE_FOR_DENY_ONLY);
        assert_eq!(value.groups[2].attributes, SE_GROUP_INTEGRITY);
        assert!(value.privileges[0].is_enabled());
    });
    assert_eq!(token.group_count, 5);
    assert_eq!(token.privilege_count, 3);
    assert!(free_handle(copy));
    assert!(free_handle(original));
}

#[test]
fn impersonation_type_and_level_constraints_follow_source_level() {
    let _guard = state_guard();
    for source_level in [
        ImpersonationLevel::SecurityIdentification,
        ImpersonationLevel::SecurityImpersonation,
    ] {
        let mut token = Token::new_impersonation(SID_USERS, source_level);
        let original = source(&mut token, TOKEN_ALL_ACCESS);
        assert_eq!(
            duplicate_token(original, 8, 0, 2, false, Some(3)),
            Err(STATUS_BAD_IMPERSONATION_LEVEL)
        );
        let copy = duplicate_token(original, 8, 0, 2, false, None).unwrap();
        inspect(copy, |copy| {
            assert_eq!(copy.impersonation_level, source_level)
        });
        assert!(free_handle(copy));
        let primary = duplicate_token(original, 8, 0, 1, false, Some(3));
        if source_level == ImpersonationLevel::SecurityIdentification {
            assert_eq!(primary, Err(STATUS_BAD_IMPERSONATION_LEVEL));
        } else {
            assert!(free_handle(primary.unwrap()));
        }
        assert_eq!(
            duplicate_token(original, 8, 0, 0, false, None),
            Err(STATUS_INVALID_PARAMETER)
        );
        assert_eq!(
            duplicate_token(original, 8, 0, 2, false, Some(4)),
            Err(STATUS_BAD_IMPERSONATION_LEVEL)
        );
        assert!(free_handle(original));
    }
}

#[test]
fn duplicate_requires_rights_and_maps_generic_access() {
    let _guard = state_guard();
    let mut token = Token::new_primary(SID_USERS);
    let limited = source(&mut token, 8);
    assert_eq!(
        duplicate_token(limited, 8, 0, 1, false, None),
        Err(STATUS_ACCESS_DENIED)
    );
    assert!(free_handle(limited));
    let original = source(&mut token, 0x2000a);
    let copy = duplicate_token(original, 0x80000000, 0, 1, false, None).unwrap();
    assert!(with_sync_access(copy, 8, |_| Ok(())).is_ok());
    assert_eq!(
        with_sync_access(copy, 2, |_| Ok(())),
        Err(STATUS_ACCESS_DENIED)
    );
    assert_eq!(
        duplicate_token(original, 0x20, 0, 1, false, None),
        Err(STATUS_ACCESS_DENIED)
    );
    assert_eq!(map_token_access(0x20000000), Ok(0x20004));
    assert_eq!(map_token_access(0x02000000), Err(STATUS_NOT_SUPPORTED));
    let event = alloc_dispatcher_handle(HandleKind::Event, 0, DispatcherState::None);
    assert_eq!(
        duplicate_token(event, 8, 0, 1, false, None),
        Err(STATUS_OBJECT_TYPE_MISMATCH)
    );
    for handle in [copy, original, event] {
        assert!(free_handle(handle));
    }
}

#[test]
fn capture_qos_output_faults_and_allocation_failures_do_not_leak() {
    let _guard = state_guard();
    let baseline = pool::LIVE.load(Ordering::Relaxed);
    let mut token = Token::new_primary(SID_USERS);
    let mut acl = Acl::new();
    token.default_dacl = &mut acl;
    let original = source(&mut token, TOKEN_ALL_ACCESS);
    let mut qos = SecurityQualityOfService {
        length: 12,
        impersonation_level: 1,
        context_tracking_mode: 1,
        effective_only: 0,
    };
    let mut attributes = ObjectAttributes::new();
    attributes.attributes = 2;
    attributes.security_quality_of_service = (&mut qos as *mut SecurityQualityOfService).cast();
    let mut output = core::ptr::null_mut();
    let call = |attributes: *mut ObjectAttributes, output: *mut HANDLE| unsafe {
        nt_duplicate_token(original, 8, attributes, 0, 2, output)
    };
    assert_eq!(call(&mut attributes, &mut output), STATUS_SUCCESS);
    assert_eq!(handle_flags(output).unwrap(), 1);
    inspect(output, |copy| {
        assert_eq!(
            copy.impersonation_level,
            ImpersonationLevel::SecurityIdentification
        )
    });
    assert!(free_handle(output));
    qos.context_tracking_mode = 2;
    attributes.security_quality_of_service = (&mut qos as *mut SecurityQualityOfService).cast();
    assert_eq!(call(&mut attributes, &mut output), STATUS_INVALID_PARAMETER);
    qos.context_tracking_mode = 0;
    qos.impersonation_level = 4;
    attributes.security_quality_of_service = (&mut qos as *mut SecurityQualityOfService).cast();
    assert_eq!(
        call(&mut attributes, &mut output),
        STATUS_BAD_IMPERSONATION_LEVEL
    );
    qos.impersonation_level = 2;
    qos.length = 8;
    attributes.security_quality_of_service = (&mut qos as *mut SecurityQualityOfService).cast();
    assert_eq!(call(&mut attributes, &mut output), STATUS_INVALID_PARAMETER);
    assert_eq!(call(1usize as *mut _, &mut output), 0x80000002u32 as i32);
    assert_eq!(
        call(core::ptr::null_mut(), core::ptr::null_mut()),
        STATUS_ACCESS_VIOLATION
    );
    let handles = handle_count();
    crate::mm::user_copy::FAIL_WRITE.with(|value| value.set(&mut output as *mut HANDLE as u64));
    assert_eq!(
        call(core::ptr::null_mut(), &mut output),
        STATUS_ACCESS_VIOLATION
    );
    crate::mm::user_copy::FAIL_WRITE.with(|value| value.set(0));
    assert_eq!(handle_count(), handles);
    for allocations in [0, 1] {
        pool::FAIL_AFTER.with(|limit| limit.set(Some(allocations)));
        assert_eq!(
            duplicate_token(original, 8, 0, 1, false, None),
            Err(STATUS_NO_MEMORY)
        );
        pool::FAIL_AFTER.with(|limit| limit.set(None));
        assert_eq!(pool::LIVE.load(Ordering::Relaxed), baseline);
    }
    assert!(free_handle(original));
}

#[test]
fn owned_token_alias_keeps_payload_until_the_final_close() {
    let _guard = state_guard();
    let baseline = pool::LIVE.load(Ordering::Relaxed);
    let mut token = Token::new_primary(SID_USERS);
    let original = source(&mut token, TOKEN_ALL_ACCESS);
    let copy = duplicate_token(original, 0, 0, 1, false, None).unwrap();
    let alias = duplicate_sync_handle_with_access(copy, 8, 0, 0).unwrap();
    assert!(free_handle(copy));
    assert_eq!(pool::LIVE.load(Ordering::Relaxed), baseline + 1);
    inspect(alias, |token| {
        assert_eq!(token.token_type, TokenTypeEnum::TokenPrimary)
    });
    close_sync_handle(alias).unwrap();
    assert_eq!(pool::LIVE.load(Ordering::Relaxed), baseline);
    assert!(free_handle(original));
}

#[test]
fn pinned_token_reader_serializes_with_final_close() {
    let _guard = state_guard();
    let baseline = pool::LIVE.load(Ordering::Relaxed);
    let mut token = Token::new_primary(SID_USERS);
    let original = source(&mut token, TOKEN_ALL_ACCESS);
    let copy = duplicate_token(original, 8, 0, 1, false, None).unwrap();
    let raw = copy as usize;
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        inspect(raw as HANDLE, |token| {
            entered_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            assert_eq!(token.token_type, TokenTypeEnum::TokenPrimary);
        });
    });
    entered_rx.recv().unwrap();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let closer = std::thread::spawn(move || {
        assert!(free_handle(raw as HANDLE));
        done_tx.send(()).unwrap();
    });
    assert!(done_rx
        .recv_timeout(std::time::Duration::from_millis(20))
        .is_err());
    assert_eq!(pool::LIVE.load(Ordering::Relaxed), baseline + 1);
    release_tx.send(()).unwrap();
    reader.join().unwrap();
    closer.join().unwrap();
    assert_eq!(pool::LIVE.load(Ordering::Relaxed), baseline);
    assert!(free_handle(original));
}
