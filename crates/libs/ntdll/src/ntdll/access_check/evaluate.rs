//! Ordered allow/deny ACE evaluation, separated from caller-memory capture.

use super::object_types::{ObjectType, Operation, Tree};
use super::*;
use crate::se::{sid::sid, token};
use alloc::vec::Vec;

const OWNER_RIGHTS_SID: [u8; 12] = [1, 1, 0, 0, 0, 0, 0, 3, 4, 0, 0, 0];
const INHERIT_ONLY_ACE: u8 = 8;

fn same_sid(bytes: &[u8], value: &sid) -> bool {
    value.is_valid()
        && bytes.len() == value.size()
        && bytes[0] == value.revision
        && bytes[1] == value.sub_authority_count
        && bytes[2..8] == value.identifier_authority
        && value.sub_authority[..value.sub_authority_count as usize]
            .iter()
            .enumerate()
            .all(|(index, part)| bytes[8 + index * 4..12 + index * 4] == part.to_le_bytes())
}

fn matches_token(bytes: &[u8], token: &Token, deny: bool, restricting: bool) -> bool {
    if restricting {
        return token.restricted_groups[..token.restricted_count]
            .iter()
            .any(|group| same_sid(bytes, &group.sid));
    }
    (same_sid(bytes, &token.user)
        && (deny || token.user_attributes & token::SE_GROUP_USE_FOR_DENY_ONLY == 0))
        || token.groups[..token.group_count].iter().any(|group| {
            let deny_only = group.attributes & token::SE_GROUP_USE_FOR_DENY_ONLY != 0;
            (if deny {
                group.is_enabled() || deny_only
            } else {
                group.is_enabled() && !deny_only
            }) && same_sid(bytes, &group.sid)
        })
}

fn owner(bytes: &[u8], token: &Token) -> bool {
    (same_sid(bytes, &token.user) && token.user_attributes & token::SE_GROUP_USE_FOR_DENY_ONLY == 0)
        || token.groups[..token.group_count].iter().any(|group| {
            group.attributes & token::SE_GROUP_OWNER != 0
                && group.attributes & token::SE_GROUP_USE_FOR_DENY_ONLY == 0
                && same_sid(bytes, &group.sid)
        })
}

fn pass(
    descriptor: &capture::Descriptor,
    token: &Token,
    desired: u32,
    mapping: &GenericMapping,
    restricting: bool,
    types: &[ObjectType],
    principal: Option<&[u8]>,
    result_list: bool,
) -> Result<Vec<AccessDecision>, NTSTATUS> {
    let maximum = desired & MAXIMUM_ALLOWED != 0;
    let explicit = desired & !MAXIMUM_ALLOWED;
    let mut result = AccessDecision::new(0, STATUS_SUCCESS);
    if explicit & ACCESS_SYSTEM_SECURITY != 0 {
        if !token.has_privilege(&luid::from_u64(8)) {
            result.status = STATUS_PRIVILEGE_NOT_HELD;
            return decisions(result, types, result_list);
        }
        result.granted |= ACCESS_SYSTEM_SECURITY;
        result.use_privilege(8);
    }
    if explicit & WRITE_OWNER != 0 && token.has_privilege(&luid::from_u64(9)) {
        result.granted |= WRITE_OWNER;
        result.use_privilege(9);
    }
    let is_owner = !restricting && owner(&descriptor.owner, token);
    let has_owner_rights_ace = descriptor.dacl.as_ref().is_some_and(|aces| {
        aces.iter().any(|ace| {
            ace.flags & INHERIT_ONLY_ACE == 0 && ace.sid.as_slice() == OWNER_RIGHTS_SID
        })
    });
    if is_owner && !has_owner_rights_ace {
        result.granted |= (if maximum { u32::MAX } else { explicit }) & (READ_CONTROL | WRITE_DAC);
    }
    let Some(aces) = &descriptor.dacl else {
        result.granted |= explicit;
        if maximum {
            result.granted |= mapping.generic_all & !(ACCESS_SYSTEM_SECURITY | MAXIMUM_ALLOWED);
        }
        return decisions(result, types, result_list);
    };
    let mut tree = Tree::new(types, explicit & !result.granted)?;
    let collect_maximum = maximum || result_list;
    let mut stopped_on_deny = false;
    for ace in aces {
        if ace.flags & INHERIT_ONLY_ACE != 0 {
            continue;
        }
        let mask = mapping.map(ace.mask) & !(ACCESS_SYSTEM_SECURITY | MAXIMUM_ALLOWED);
        let deny = matches!(ace.kind, 1 | 6);
        let index = if let Some(guid) = &ace.object_type {
            if types.is_empty() {
                // NT ignores object-specific allow ACEs without a type list.
                // Object-specific deny ACEs still protect the whole object.
                if !deny {
                    continue;
                }
                0
            } else if let Some(index) = tree.first(guid) {
                index
            } else {
                continue;
            }
        } else {
            0
        };
        // S-1-5-10 (PRINCIPAL_SELF) names the supplied principal, rather than
        // the user whose token happens to be checked.
        let identity = if ace.sid.as_slice() == [1, 1, 0, 0, 0, 0, 0, 5, 10, 0, 0, 0] {
            principal.unwrap_or(&ace.sid)
        } else {
            &ace.sid
        };
        let matches = if ace.sid.as_slice() == OWNER_RIGHTS_SID {
            is_owner
        } else {
            matches_token(identity, token, deny, restricting)
        };
        if !matches {
            continue;
        }
        if collect_maximum {
            tree.apply(
                index,
                mask,
                if deny {
                    Operation::DenyMaximum
                } else {
                    Operation::AllowMaximum
                },
            );
        } else if deny {
            if tree.masks[index].remaining & mask != 0 {
                stopped_on_deny = true;
                break;
            }
        } else {
            tree.apply(index, mask, Operation::Grant);
        }
        // Once explicit rights are granted, later deny ACEs do not revoke
        // them. Maximum-access evaluation must continue through the ACL.
        if !collect_maximum && tree.masks[0].remaining == 0 {
            break;
        }
    }
    let mut output = decisions(result, types, result_list)?;
    for (result, masks) in output.iter_mut().zip(&tree.masks) {
        if collect_maximum {
            result.granted |= masks.allowed & if maximum { u32::MAX } else { explicit };
            if explicit & !result.granted != 0 || result.granted == 0 {
                result.status = STATUS_ACCESS_DENIED;
                // Scalar checks report no granted access on denial. The
                // result-list API retains the independently granted subset.
                if !result_list {
                    result.granted = 0;
                }
            }
        } else if stopped_on_deny || masks.remaining != 0 {
            result.status = STATUS_ACCESS_DENIED;
            result.granted = 0;
        } else {
            result.granted |= explicit;
        }
    }
    Ok(output)
}

fn decisions(
    result: AccessDecision,
    types: &[ObjectType],
    list: bool,
) -> Result<Vec<AccessDecision>, NTSTATUS> {
    let count = if list { types.len().max(1) } else { 1 };
    let mut output = Vec::new();
    output
        .try_reserve_exact(count)
        .map_err(|_| STATUS_NO_MEMORY)?;
    output.resize(count, result);
    Ok(output)
}

/// Both SID sets must grant the requested access. Evaluate maximum access
/// independently in each pass, then intersect; combining SID lists would grant
/// rights that neither security identity is allowed to exercise on its own.
pub(super) fn evaluate(
    descriptor: &capture::Descriptor,
    token: &Token,
    desired: u32,
    mapping: &GenericMapping,
) -> Result<AccessDecision, NTSTATUS> {
    Ok(evaluate_types(descriptor, token, desired, mapping, &[], None, false)?[0])
}

pub(super) fn evaluate_types(
    descriptor: &capture::Descriptor,
    token: &Token,
    desired: u32,
    mapping: &GenericMapping,
    types: &[ObjectType],
    principal: Option<&[u8]>,
    result_list: bool,
) -> Result<Vec<AccessDecision>, NTSTATUS> {
    let mut ordinary = pass(
        descriptor,
        token,
        desired,
        mapping,
        false,
        types,
        principal,
        result_list,
    )?;
    if token.flags & token::TOKEN_IS_RESTRICTED != 0 {
        let restricted = pass(
            descriptor,
            token,
            desired,
            mapping,
            true,
            types,
            principal,
            result_list,
        )?;
        for (ordinary, restricted) in ordinary.iter_mut().zip(restricted) {
            ordinary.granted &= restricted.granted;
            if restricted.status != STATUS_SUCCESS
                || desired & !MAXIMUM_ALLOWED & !ordinary.granted != 0
                || (desired & MAXIMUM_ALLOWED != 0 && ordinary.granted == 0)
            {
                ordinary.status = STATUS_ACCESS_DENIED;
                if !result_list {
                    ordinary.granted = 0;
                }
            }
        }
    }
    Ok(ordinary)
}
