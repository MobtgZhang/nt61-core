use crate::adjust::{se_adjust_groups_token, se_adjust_privileges_token};
use crate::sid::{sid, WellKnownSid};
use crate::token::*;

fn privilege(number: u64, attributes: u32) -> TokenPrivilege {
    TokenPrivilege::new(luid::from_u64(number), attributes)
}

#[test]
fn trusted_adjustment_never_adds_privileges_even_with_spare_capacity() {
    let mut token = Token::new();
    token.add_privilege(privilege(17, SE_PRIVILEGE_ENABLED));
    let requests = [privilege(17, 0), privilege(20, SE_PRIVILEGE_ENABLED)];
    let mut previous = Vec::new();
    // SAFETY: stack-owned token is exclusively accessed by this test.
    assert!(!unsafe {
        se_adjust_privileges_token(&mut token, false, &requests, Some(&mut previous))
    });
    assert_eq!(token.privilege_count, 1);
    assert!(!token.has_privilege(&luid::from_u64(17)));
    assert!(!token.has_privilege(&luid::from_u64(20)));
    assert_eq!(previous.len(), 1);
    assert_eq!(previous[0].luid.to_u64(), 17);
    assert_eq!(previous[0].attributes, SE_PRIVILEGE_ENABLED);
}

#[test]
fn removed_privileges_cannot_be_restored_or_leak_into_previous_state() {
    let mut token = Token::new();
    token.add_privilege(privilege(17, SE_PRIVILEGE_ENABLED));
    token.add_privilege(privilege(18, SE_PRIVILEGE_ENABLED_BY_DEFAULT));
    token.add_privilege(privilege(29, SE_PRIVILEGE_ENABLED));
    token.refresh_privilege_flags();
    token.flags |= TOKEN_WRITE_RESTRICTED;
    let mut previous = vec![privilege(99, 0)];
    let requests = [
        privilege(17, SE_PRIVILEGE_REMOVED | SE_PRIVILEGE_ENABLED),
        privilege(18, SE_PRIVILEGE_ENABLED | SE_PRIVILEGE_USED_FOR_ACCESS),
    ];
    // SAFETY: stack-owned token is exclusively accessed by this test.
    assert!(unsafe {
        se_adjust_privileges_token(&mut token, false, &requests, Some(&mut previous))
    });
    assert_eq!(token.privilege_count, 2);
    assert_eq!(token.privileges[0].luid.to_u64(), 18);
    assert_eq!(token.privileges[0].attributes, 3);
    assert_eq!(token.privileges[1].luid.to_u64(), 29);
    assert_eq!(previous.len(), 2);
    assert_eq!(previous[1].luid.to_u64(), 18);
    assert_eq!(previous[1].attributes, SE_PRIVILEGE_ENABLED_BY_DEFAULT);
    assert_eq!(token.flags & TOKEN_HAS_BACKUP_PRIVILEGE, 0);
    assert_ne!(token.flags & TOKEN_HAS_RESTORE_PRIVILEGE, 0);
    assert_ne!(token.flags & TOKEN_HAS_IMPERSONATE_PRIVILEGE, 0);
    assert_ne!(token.flags & TOKEN_WRITE_RESTRICTED, 0);
    // SAFETY: same exclusive access as above.
    assert!(!unsafe { se_adjust_privileges_token(&mut token, false, &[privilege(17, 2)], None) });
    assert_eq!(token.privilege_count, 2);
}

#[test]
fn disabling_all_ignores_requests_and_returns_only_changed_privileges() {
    let mut token = Token::new();
    token.add_privilege(privilege(
        17,
        SE_PRIVILEGE_ENABLED | SE_PRIVILEGE_USED_FOR_ACCESS,
    ));
    token.add_privilege(privilege(18, SE_PRIVILEGE_ENABLED_BY_DEFAULT));
    token.refresh_privilege_flags();
    let mut previous = Vec::new();
    // SAFETY: stack-owned token is exclusively accessed by this test.
    assert!(unsafe {
        se_adjust_privileges_token(
            &mut token,
            true,
            &[privilege(20, SE_PRIVILEGE_REMOVED)],
            Some(&mut previous),
        )
    });
    assert_eq!(token.privilege_count, 2);
    assert_eq!(token.privileges[0].attributes, SE_PRIVILEGE_USED_FOR_ACCESS);
    assert_eq!(
        token.privileges[1].attributes,
        SE_PRIVILEGE_ENABLED_BY_DEFAULT
    );
    assert_eq!(previous.len(), 1);
    assert_eq!(previous[0].luid.to_u64(), 17);
    assert_eq!(token.flags & TOKEN_HAS_BACKUP_PRIVILEGE, 0);
    // Restoring previous state only re-enables the privilege that changed.
    assert!(unsafe { se_adjust_privileges_token(&mut token, false, &previous, None) });
    assert!(token.has_privilege(&luid::from_u64(17)));
    assert!(!token.has_privilege(&luid::from_u64(18)));
}

#[test]
fn duplicate_requests_do_not_duplicate_previous_entries() {
    let mut token = Token::new();
    token.add_privilege(privilege(17, 0));
    let mut previous = Vec::new();
    // SAFETY: stack-owned token is exclusively accessed by this test.
    assert!(unsafe {
        se_adjust_privileges_token(
            &mut token,
            false,
            &[privilege(17, 2), privilege(17, 2)],
            Some(&mut previous),
        )
    });
    assert_eq!(previous.len(), 1);
    assert_eq!(token.privilege_count, 1);
    assert!(token.has_privilege(&luid::from_u64(17)));
    assert!(!unsafe { se_adjust_privileges_token(core::ptr::null_mut(), false, &[], None) });
}

#[test]
fn trusted_groups_reject_new_membership_and_enforce_group_restrictions() {
    let users = sid::well_known(WellKnownSid::Users);
    let admins = sid::well_known(WellKnownSid::Administrators);
    let mut token = Token::new();
    token.add_group(TokenGroup::new(users, SE_GROUP_OWNER));
    let mut previous = Vec::new();
    // SAFETY: stack-owned token is exclusively accessed by this test.
    assert!(!unsafe {
        se_adjust_groups_token(
            &mut token,
            false,
            &[
                TokenGroup::new(users, SE_GROUP_ENABLED),
                TokenGroup::new(admins, SE_GROUP_ENABLED),
            ],
            Some(&mut previous),
        )
    });
    assert_eq!(token.group_count, 1);
    assert_eq!(
        token.groups[0].attributes,
        SE_GROUP_OWNER | SE_GROUP_ENABLED
    );
    assert_eq!(previous.len(), 1);
    assert_eq!(previous[0].attributes, SE_GROUP_OWNER);

    token.groups[0].attributes |= SE_GROUP_MANDATORY;
    assert!(!unsafe {
        se_adjust_groups_token(&mut token, false, &[TokenGroup::new(users, 0)], None)
    });
    assert!(token.groups[0].is_enabled());
    token.groups[0].attributes = SE_GROUP_USE_FOR_DENY_ONLY;
    assert!(!unsafe {
        se_adjust_groups_token(
            &mut token,
            false,
            &[TokenGroup::new(users, SE_GROUP_ENABLED)],
            None,
        )
    });
    assert_eq!(token.groups[0].attributes, SE_GROUP_USE_FOR_DENY_ONLY);
    assert!(!unsafe { se_adjust_groups_token(core::ptr::null_mut(), true, &[], None) });
}
