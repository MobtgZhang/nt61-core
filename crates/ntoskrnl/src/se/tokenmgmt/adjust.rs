//! Trusted token adjustment. Callers serialize mutation and own the token.
use crate::se::token::{Token, TokenGroup, TokenPrivilege, TOKEN_MAX_GROUPS, TOKEN_MAX_PRIVILEGES};
use alloc::vec::Vec;

/// Adjust existing privileges, never adding privileges absent at token creation.
/// Removed privileges are compacted out and cannot be returned for restoration.
///
/// # Safety
/// `token` must be live, uniquely borrowed, and protected against concurrent access.
pub unsafe fn se_adjust_privileges_token(
    token: *mut Token,
    disable_all: bool,
    requested: &[TokenPrivilege],
    mut previous: Option<&mut Vec<TokenPrivilege>>,
) -> bool {
    // SAFETY: required by the caller contract above.
    let Some(token) = (unsafe { token.as_mut() }) else {
        return false;
    };
    let mut privileges = [TokenPrivilege::default(); TOKEN_MAX_PRIVILEGES];
    let mut old = [TokenPrivilege::default(); TOKEN_MAX_PRIVILEGES];
    let mut count = 0;
    let mut changed = 0;
    for privilege in &token.privileges[..token.privilege_count] {
        let request = requested.iter().find(|entry| entry.luid == privilege.luid);
        let Some(attributes) =
            privilege.adjusted_attributes(disable_all, request.map(|entry| entry.attributes))
        else {
            continue;
        };
        if attributes != privilege.attributes {
            old[changed] = *privilege;
            changed += 1;
        }
        privileges[count] = TokenPrivilege {
            luid: privilege.luid,
            attributes,
        };
        count += 1;
    }
    // Reserve output before any token mutation, including privilege removal.
    if let Some(previous) = previous.as_mut() {
        if previous.try_reserve(changed).is_err() {
            return false;
        }
        previous.extend_from_slice(&old[..changed]);
    }
    let all_found = disable_all
        || requested.iter().all(|request| {
            token.privileges[..token.privilege_count]
                .iter()
                .any(|entry| entry.luid == request.luid)
        });
    token.privileges = privileges;
    token.privilege_count = count;
    token.refresh_privilege_flags();
    all_found
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
pub unsafe fn se_adjust_groups_token(
    token: *mut Token,
    reset_to_default: bool,
    new_state: &[TokenGroup],
    mut previous_state: Option<&mut Vec<TokenGroup>>,
) -> bool {
    // SAFETY: the caller supplies a live token and serializes token mutation.
    let Some(token) = (unsafe { token.as_mut() }) else {
        return false;
    };
    let mut attributes = [0; TOKEN_MAX_GROUPS];
    let mut changed = 0;
    for (index, group) in token.groups[..token.group_count].iter().enumerate() {
        let request = new_state.iter().find(|entry| entry.sid.equals(&group.sid));
        let Ok(value) =
            group.adjusted_attributes(reset_to_default, request.map(|entry| entry.attributes))
        else {
            return false;
        };
        attributes[index] = value;
        changed += usize::from(value != group.attributes);
    }
    if let Some(previous) = previous_state.as_mut() {
        if previous.try_reserve(changed).is_err() {
            return false;
        }
    }
    let all_found = reset_to_default
        || new_state.iter().all(|request| {
            token.groups[..token.group_count]
                .iter()
                .any(|group| group.sid.equals(&request.sid))
        });
    for (group, attributes) in token.groups[..token.group_count].iter_mut().zip(attributes) {
        if group.attributes != attributes {
            if let Some(previous) = previous_state.as_mut() {
                previous.push(*group);
            }
            group.attributes = attributes;
        }
    }
    all_found
}
