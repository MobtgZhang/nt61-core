//! Exercise captured production descriptors and token policy, not a test model.
use super::*;
use crate::se::{
    sid::{sid, WellKnownSid},
    token::{self, TokenGroup},
};
use alloc::{vec, vec::Vec};

fn identity(value: sid) -> Vec<u8> {
    let mut output = vec![value.revision, value.sub_authority_count];
    output.extend(value.identifier_authority);
    for part in &value.sub_authority[..value.sub_authority_count as usize] {
        output.extend(part.to_le_bytes());
    }
    output
}

fn token() -> Token {
    Token::new_impersonation(
        sid::well_known(WellKnownSid::LocalSystem),
        ImpersonationLevel::SecurityImpersonation,
    )
}

fn ace(
    kind: u8,
    flags: u8,
    mask: u32,
    object: Option<[u8; 16]>,
    inherited: Option<[u8; 16]>,
    sid: &[u8],
) -> Vec<u8> {
    let mut bytes = vec![kind, flags, 0, 0];
    bytes.extend(mask.to_le_bytes());
    if matches!(kind, 5 | 6) {
        bytes.extend(
            ((object.is_some() as u32) | ((inherited.is_some() as u32) << 1)).to_le_bytes(),
        );
        if let Some(guid) = object {
            bytes.extend(guid);
        }
        if let Some(guid) = inherited {
            bytes.extend(guid);
        }
    }
    bytes.extend(sid);
    let length = bytes.len() as u16;
    bytes[2..4].copy_from_slice(&length.to_le_bytes());
    bytes
}

fn descriptor(aces: &[Vec<u8>]) -> Vec<u8> {
    let owner = identity(sid::well_known(WellKnownSid::World));
    let mut bytes = vec![1, 0, 4, 0x80];
    bytes.extend(20u32.to_le_bytes());
    bytes.extend(20u32.to_le_bytes());
    bytes.extend(0u32.to_le_bytes());
    bytes.extend(((20 + owner.len()) as u32).to_le_bytes());
    bytes.extend(owner);
    let length = 8 + aces.iter().map(Vec::len).sum::<usize>();
    bytes.extend([4, 0]);
    bytes.extend((length as u16).to_le_bytes());
    bytes.extend((aces.len() as u16).to_le_bytes());
    bytes.extend([0, 0]);
    for ace in aces {
        bytes.extend(ace);
    }
    bytes
}

fn mapping() -> GenericMapping {
    GenericMapping {
        generic_read: 1,
        generic_write: 2,
        generic_execute: 4,
        generic_all: 7,
    }
}

fn types(levels: &[u16]) -> Vec<object_types::ObjectType> {
    let guids: Vec<_> = (0..levels.len()).map(|index| [index as u8; 16]).collect();
    let records: Vec<_> = levels
        .iter()
        .enumerate()
        .map(|(index, &level)| ObjectTypeList {
            level,
            sbz: 0,
            object_type: &guids[index],
        })
        .collect();
    object_types::capture(records.as_ptr() as u64, records.len() as u32).unwrap()
}

fn check(
    aces: &[Vec<u8>],
    token: &Token,
    desired: u32,
    types: &[object_types::ObjectType],
    principal: Option<&[u8]>,
    list: bool,
) -> Vec<(u32, NTSTATUS)> {
    let bytes = descriptor(aces);
    let descriptor = capture::descriptor(capture::Memory::Trusted(&bytes)).unwrap();
    evaluate::evaluate_types(
        &descriptor,
        token,
        desired,
        &mapping(),
        types,
        principal,
        list,
    )
    .unwrap()
    .iter()
    .map(|decision| (decision.granted, decision.status))
    .collect()
}

#[test]
fn object_ace_capture_handles_both_optional_guid_fields_and_rejects_bad_sid() {
    let user = identity(token().user);
    for kind in [5, 6] {
        for object in [None, Some([1; 16])] {
            for inherited in [None, Some([2; 16])] {
                let bytes = descriptor(&[ace(kind, 0, 3, object, inherited, &user)]);
                let sd = capture::descriptor(capture::Memory::Trusted(&bytes)).unwrap();
                let captured = &sd.dacl.as_ref().unwrap()[0];
                assert_eq!(captured.object_type, object);
                assert_eq!(captured.sid, user);
            }
        }
    }
    let mut bad = ace(5, 0, 3, Some([1; 16]), Some([2; 16]), &user);
    bad[45] = 15; // SID claims more subauthorities than the captured ACE holds.
    assert!(matches!(
        capture::descriptor(capture::Memory::Trusted(&descriptor(&[bad]))),
        Err(STATUS_INVALID_ACL)
    ));
    let mut bad = ace(5, 0, 3, Some([1; 16]), None, &user);
    bad.truncate(24);
    bad[2..4].copy_from_slice(&24u16.to_le_bytes());
    assert!(matches!(
        capture::descriptor(capture::Memory::Trusted(&descriptor(&[bad]))),
        Err(STATUS_INVALID_ACL)
    ));
}

#[test]
fn object_ace_without_type_list_ignores_specific_allow_but_enforces_deny() {
    let token = token();
    let user = identity(token.user);
    let allow = ace(0, 0, 3, None, None, &user);
    assert_eq!(
        check(
            &[ace(5, 0, 3, Some([1; 16]), None, &user)],
            &token,
            1,
            &[],
            None,
            false
        ),
        [(0, STATUS_ACCESS_DENIED)]
    );
    assert_eq!(
        check(
            &[ace(6, 0, 1, Some([1; 16]), None, &user), allow],
            &token,
            1,
            &[],
            None,
            false
        ),
        [(0, STATUS_ACCESS_DENIED)]
    );
    for inherited in [None, Some([99; 16])] {
        assert_eq!(
            check(
                &[ace(5, 0, 3, None, inherited, &user)],
                &token,
                3,
                &[],
                None,
                false
            ),
            [(3, STATUS_SUCCESS)]
        );
    }
}

#[test]
fn object_access_tree_distinguishes_whole_tree_from_each_property() {
    let token = token();
    let user = identity(token.user);
    let types = types(&[0, 1, 2, 1]);
    let aces = [
        ace(5, 0, 3, Some([1; 16]), None, &user),
        ace(5, 0, 1, Some([3; 16]), None, &user),
    ];
    assert_eq!(
        check(&aces, &token, MAXIMUM_ALLOWED, &types, None, true),
        [(1, 0), (3, 0), (3, 0), (1, 0)]
    );
    assert_eq!(
        check(&aces, &token, 3, &types, None, true),
        [
            (1, STATUS_ACCESS_DENIED),
            (3, 0),
            (3, 0),
            (1, STATUS_ACCESS_DENIED)
        ]
    );
    assert_eq!(
        check(&aces, &token, 3, &types, None, false),
        [(0, STATUS_ACCESS_DENIED)]
    );
    assert_eq!(
        check(&aces, &token, 1, &types, None, false),
        [(1, STATUS_SUCCESS)]
    );
}

#[test]
fn object_access_principal_self_and_inherit_only_do_not_grant_accidentally() {
    let token = token();
    let user = identity(token.user);
    let principal_self = [1, 1, 0, 0, 0, 0, 0, 5, 10, 0, 0, 0];
    let types = types(&[0, 1]);
    let aces = [ace(5, 0, 3, Some([0; 16]), None, &principal_self)];
    assert_eq!(
        check(&aces, &token, 3, &types, Some(&user), true),
        [(3, 0), (3, 0)]
    );
    assert_eq!(
        check(&aces, &token, 3, &types, None, true),
        [(0, STATUS_ACCESS_DENIED); 2]
    );
    let inherited_only = [ace(5, 8, 7, Some([0; 16]), None, &user)];
    assert_eq!(
        check(&inherited_only, &token, 1, &types, None, true),
        [(0, STATUS_ACCESS_DENIED); 2]
    );
}

#[test]
fn object_access_restricted_tokens_intersect_per_property() {
    let mut token = token();
    let user = identity(token.user);
    let restricted = sid::well_known(WellKnownSid::World);
    token.restricted_groups[0] = TokenGroup::new(restricted, token::SE_GROUP_ENABLED);
    token.restricted_count = 1;
    token.flags |= token::TOKEN_IS_RESTRICTED;
    let types = types(&[0, 1, 1]);
    let aces = [
        ace(5, 0, 7, Some([0; 16]), None, &user),
        ace(5, 0, 3, Some([1; 16]), None, &identity(restricted)),
    ];
    assert_eq!(
        check(&aces, &token, MAXIMUM_ALLOWED, &types, None, true),
        [(0, STATUS_ACCESS_DENIED), (3, 0), (0, STATUS_ACCESS_DENIED)]
    );
}

#[test]
fn object_native_interfaces_write_exact_result_count_and_privilege_size() {
    let _guard = super::super::file::HANDLE_TEST_LOCK.lock();
    use super::super::file::{alloc_access_handle, free_handle};
    let mut token = token();
    let handle =
        alloc_access_handle(HandleKind::Token, &mut token as *mut Token as u64, 8, false).unwrap();
    let user = identity(token.user);
    let bytes = descriptor(&[
        ace(5, 0, 3, Some([1; 16]), None, &user),
        ace(5, 0, 1, Some([3; 16]), None, &user),
    ]);
    let guids = [[0; 16], [1; 16], [2; 16], [3; 16]];
    let mut records: Vec<_> = [0, 1, 2, 1]
        .iter()
        .enumerate()
        .map(|(index, &level)| ObjectTypeList {
            level,
            sbz: 0xffff,
            object_type: &guids[index],
        })
        .collect();
    let mut privileges = [0xccccccccu32; 8];
    let mut length = 32;
    let mut granted = [0xcccccccc; 5];
    let mut status = [0xccccccccu32 as NTSTATUS; 5];
    let invoke = |list: bool,
                  count: u32,
                  types: *const ObjectTypeList,
                  length: &mut u32,
                  granted: &mut [u32; 5],
                  status: &mut [NTSTATUS; 5],
                  privileges: &mut [u32; 8]| unsafe {
        let function = if list {
            nt_access_check_by_type_result_list
        } else {
            nt_access_check_by_type
        };
        function(
            bytes.as_ptr().cast(),
            core::ptr::null_mut(),
            handle,
            3,
            types,
            count,
            &mapping(),
            privileges.as_mut_ptr().cast(),
            length,
            granted.as_mut_ptr(),
            status.as_mut_ptr(),
        )
    };
    assert_eq!(
        invoke(
            true,
            4,
            records.as_ptr(),
            &mut length,
            &mut granted,
            &mut status,
            &mut privileges
        ),
        STATUS_SUCCESS
    );
    assert_eq!(granted, [1, 3, 3, 1, 0xcccccccc]);
    assert_eq!(
        status,
        [
            STATUS_ACCESS_DENIED,
            0,
            0,
            STATUS_ACCESS_DENIED,
            0xccccccccu32 as NTSTATUS
        ]
    );
    assert_eq!(
        (length, privileges[0], privileges[1], privileges[2]),
        (32, 0, 0, 0xcccccccc)
    );
    granted.fill(0xcccccccc);
    status.fill(0xccccccccu32 as NTSTATUS);
    assert_eq!(
        invoke(
            false,
            4,
            records.as_ptr(),
            &mut length,
            &mut granted,
            &mut status,
            &mut privileges
        ),
        STATUS_SUCCESS
    );
    assert_eq!((granted[0], status[0]), (0, STATUS_ACCESS_DENIED));
    assert_eq!(granted[1], 0xcccccccc);
    length = 0;
    granted.fill(0xcccccccc);
    assert_eq!(
        invoke(
            true,
            4,
            records.as_ptr(),
            &mut length,
            &mut granted,
            &mut status,
            &mut privileges
        ),
        STATUS_BUFFER_TOO_SMALL
    );
    assert_eq!(length, 20);
    assert_eq!(granted, [0xcccccccc; 5]);
    length = 32;
    assert_eq!(
        invoke(
            true,
            0,
            records.as_ptr(),
            &mut length,
            &mut granted,
            &mut status,
            &mut privileges
        ),
        STATUS_INVALID_PARAMETER
    );
    records[1].level = 2;
    assert_eq!(
        invoke(
            true,
            4,
            records.as_ptr(),
            &mut length,
            &mut granted,
            &mut status,
            &mut privileges
        ),
        STATUS_INVALID_PARAMETER
    );
    assert_eq!(granted, [0xcccccccc; 5]);
    assert!(free_handle(handle));
}
