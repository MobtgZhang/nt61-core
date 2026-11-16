//! Ordered allow/deny ACE evaluation, separated from caller-memory capture.

use super::*;
use crate::se::{sid::sid, token};

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

fn matches_token(bytes: &[u8], token: &Token, deny: bool) -> bool {
    same_sid(bytes, &token.user)
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
    same_sid(bytes, &token.user)
        || token.groups[..token.group_count].iter().any(|group| {
            group.attributes & token::SE_GROUP_OWNER != 0
                && group.attributes & token::SE_GROUP_USE_FOR_DENY_ONLY == 0
                && same_sid(bytes, &group.sid)
        })
}

pub(super) fn evaluate(
    descriptor: &capture::Descriptor,
    token: &Token,
    desired: u32,
    mapping: &GenericMapping,
) -> Result<AccessDecision, NTSTATUS> {
    let maximum = desired & MAXIMUM_ALLOWED != 0;
    let explicit = desired & !MAXIMUM_ALLOWED;
    let mut result = AccessDecision::new(0, STATUS_SUCCESS);
    if explicit & ACCESS_SYSTEM_SECURITY != 0 {
        if !token.has_privilege(&luid::from_u64(8)) {
            result.status = STATUS_PRIVILEGE_NOT_HELD;
            return Ok(result);
        }
        result.granted |= ACCESS_SYSTEM_SECURITY;
        result.use_privilege(8);
    }
    if explicit & WRITE_OWNER != 0 && token.has_privilege(&luid::from_u64(9)) {
        result.granted |= WRITE_OWNER;
        result.use_privilege(9);
    }
    if owner(&descriptor.owner, token) {
        result.granted |= (if maximum { u32::MAX } else { explicit }) & (READ_CONTROL | WRITE_DAC);
    }
    let Some(aces) = &descriptor.dacl else {
        result.granted |= explicit;
        if maximum {
            result.granted |= mapping.generic_all & !(ACCESS_SYSTEM_SECURITY | MAXIMUM_ALLOWED);
        }
        return Ok(result);
    };
    let mut remaining = explicit & !result.granted;
    let mut denied = 0;
    let mut allowed = 0;
    for ace in aces {
        if ace.flags & 8 != 0 {
            continue;
        } // INHERIT_ONLY_ACE
        let mask = mapping.map(ace.mask) & !(ACCESS_SYSTEM_SECURITY | MAXIMUM_ALLOWED);
        if ace.kind == 1 && matches_token(&ace.sid, token, true) {
            if maximum {
                denied |= mask & !allowed;
            }
            if remaining & mask != 0 {
                result.status = STATUS_ACCESS_DENIED;
                result.granted = 0;
                return Ok(result);
            }
        } else if ace.kind == 0 && matches_token(&ace.sid, token, false) {
            allowed |= mask & !denied;
            remaining &= !mask;
        }
        // Once explicit rights are granted, later deny ACEs do not revoke
        // them. Maximum-access evaluation must continue through the ACL.
        if !maximum && remaining == 0 {
            break;
        }
    }
    if remaining != 0 {
        result.status = STATUS_ACCESS_DENIED;
        result.granted = 0;
    } else {
        result.granted |= explicit;
        if maximum {
            result.granted |= allowed;
        }
        if maximum && result.granted == 0 {
            result.status = STATUS_ACCESS_DENIED;
        }
    }
    Ok(result)
}
