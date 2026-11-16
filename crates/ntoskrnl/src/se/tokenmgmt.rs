//! Token Management Operations
//!
//! Implements Windows token management APIs:
//!   - Token creation (NT_CREATE_TOKEN)
//!   - Token duplication (NT_DUPLICATE_TOKEN)
//!   - Privilege adjustment (NT_ADJUST_PRIVILEGES_TOKEN)
//!   - group adjustment (NT_ADJUST_GROUPS_TOKEN)
//!   - Token filtering (restricted tokens)
//!   - Token query/set operations
//!
//! References: Windows SDK, WRK

use core::sync::atomic::{AtomicU64, Ordering};

use super::sid::sid;
use super::token::{
    luid, ImpersonationLevel, Token, TokenGroup, TokenPrivilege, TokenTypeEnum, TOKEN_MAX_GROUPS,
    TOKEN_MAX_PRIVILEGES,
};

pub const DUPLICATE_SAME_ACCESS: u32 = 0x00000002;
pub const DUPLICATE_SAME_ATTRIBUTES: u32 = 0x00000004;

pub const TOKEN_ASSIGN_PRIMARY: u32 = 0x0001;
pub const TOKEN_DUPLICATE: u32 = 0x0002;
pub const TOKEN_IMPERSONATE: u32 = 0x0004;
pub const TOKEN_QUERY: u32 = 0x0008;
pub const TOKEN_QUERY_SOURCE: u32 = 0x0010;
pub const TOKEN_ADJUST_PRIVILEGES: u32 = 0x0020;
pub const TOKEN_ADJUST_GROUPS: u32 = 0x0040;
pub const TOKEN_ADJUST_DEFAULT: u32 = 0x0080;
pub const TOKEN_ADJUST_SESSIONID: u32 = 0x0100;

pub fn se_create_token(
    user: sid,
    groups: &[TokenGroup],
    privileges: &[TokenPrivilege],
    token_type: TokenTypeEnum,
    impersonation_level: ImpersonationLevel,
) -> *mut Token {
    // Reject lists that exceed the fixed-capacity token storage instead
    // of silently dropping entries: a token that quietly lost a group
    // would grant MORE access than the caller asked for (deny-only and
    // restrictive entries disappear), and a lost privilege breaks
    // privilege bookkeeping later. Fail loudly and let the caller see
    // the allocation failure.
    if groups.len() > TOKEN_MAX_GROUPS || privileges.len() > TOKEN_MAX_PRIVILEGES {
        crate::hal::serial::write_string(
            "[SE] se_create_token: group/privilege list exceeds token capacity\r\n",
        );
        return core::ptr::null_mut();
    }

    let token_ptr = crate::mm::pool::allocate(
        crate::mm::pool::PoolType::NonPaged,
        core::mem::size_of::<Token>(),
    ) as *mut Token;

    if token_ptr.is_null() {
        return core::ptr::null_mut();
    }

    unsafe {
        let token = &mut *token_ptr;
        *token = Token::new();

        token.user = user;
        token.token_type = token_type;
        token.impersonation_level = impersonation_level;
        token.refcount = 1;

        // Capacity was verified above, so every entry fits.
        token.groups[..groups.len()].copy_from_slice(groups);
        token.group_count = groups.len();

        token.privileges[..privileges.len()].copy_from_slice(privileges);
        token.privilege_count = privileges.len();

        token.primary_group = super::sid::SID_USERS;

        static NEXT_TOKEN_ID: AtomicU64 = AtomicU64::new(1000);
        token.token_id = luid::from_u64(NEXT_TOKEN_ID.fetch_add(1, Ordering::Relaxed));

        static NEXT_AUTH_ID: AtomicU64 = AtomicU64::new(2000);
        token.authentication_id = luid::from_u64(NEXT_AUTH_ID.fetch_add(1, Ordering::Relaxed));
    }

    token_ptr
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
pub unsafe fn se_duplicate_token(
    existing_token: *const Token,
    impersonation_level: ImpersonationLevel,
    token_type: TokenTypeEnum,
) -> *mut Token {
    if existing_token.is_null() {
        return core::ptr::null_mut();
    }

    // SAFETY: the caller pins existing_token for this operation.
    let src = unsafe { &*existing_token };
    let new_token_ptr = super::token::duplicate_for_process(src);
    if new_token_ptr.is_null() {
        return core::ptr::null_mut();
    }
    // SAFETY: the duplicate owns new token and DACL allocations.
    unsafe {
        (*new_token_ptr).token_type = token_type;
        (*new_token_ptr).impersonation_level = impersonation_level;
        static NEXT_TOKEN_ID: AtomicU64 = AtomicU64::new(1000);
        (*new_token_ptr).token_id = luid::from_u64(NEXT_TOKEN_ID.fetch_add(1, Ordering::Relaxed));
        (*new_token_ptr).refcount = 1;
    }

    new_token_ptr
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
pub unsafe fn se_filter_token(
    existing_token: *const Token,
    flags: u32,
    sids_to_disable: &[sid],
    privileges_to_delete: &[luid],
    restricted_sids: &[sid],
) -> *mut Token {
    if existing_token.is_null() {
        return core::ptr::null_mut();
    }

    if restricted_sids.len() > TOKEN_MAX_GROUPS {
        return core::ptr::null_mut();
    }
    // SAFETY: existing_token was checked above and is pinned by the caller.
    let source = unsafe { &*existing_token };
    let mut disabled = [TokenGroup::default(); TOKEN_MAX_GROUPS + 1];
    let mut count = 0;
    if sids_to_disable.iter().any(|sid| sid.equals(&source.user)) {
        disabled[count] = TokenGroup::new(source.user, 0);
        count += 1;
    }
    for group in &source.groups[..source.group_count] {
        if sids_to_disable.iter().any(|sid| sid.equals(&group.sid)) {
            disabled[count] = *group;
            count += 1;
        }
    }
    let mut restricted = [TokenGroup::default(); TOKEN_MAX_GROUPS];
    for (entry, sid) in restricted.iter_mut().zip(restricted_sids) {
        *entry = TokenGroup::new(*sid, 0);
    }
    // SAFETY: source remains pinned; se_duplicate_token copies its DACL too.
    let new_token = unsafe {
        se_duplicate_token(
            existing_token,
            source.impersonation_level,
            source.token_type,
        )
    };
    if new_token.is_null() {
        return new_token;
    }
    // SAFETY: this unpublished duplicate is exclusively owned here.
    let value = unsafe { &mut *new_token };
    if value
        .filter(
            flags,
            &disabled[..count],
            privileges_to_delete,
            &restricted[..restricted_sids.len()],
        )
        .is_err()
    {
        if !value.default_dacl.is_null() {
            crate::mm::pool::free(value.default_dacl.cast());
        }
        crate::mm::pool::free(new_token.cast());
        return core::ptr::null_mut();
    }

    new_token
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
pub unsafe fn se_token_is_restricted(token: *const Token) -> bool {
    if token.is_null() {
        return false;
    }
    unsafe { ((*token).flags & super::token::TOKEN_IS_RESTRICTED) != 0 }
}

pub use self::adjust::{se_adjust_groups_token, se_adjust_privileges_token};

#[path = "tokenmgmt/adjust.rs"]
mod adjust;

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
pub unsafe fn se_query_information_token(
    token: *const Token,
    info_class: u32,
    buffer: *mut u8,
    buffer_length: u32,
    return_length: *mut u32,
) -> bool {
    if token.is_null() || return_length.is_null() {
        return false;
    }

    unsafe {
        let tok = &*token;

        match info_class {
            super::token::TOKEN_USER => {
                let required = core::mem::size_of::<sid>();
                *return_length = required as u32;
                if buffer.is_null() || buffer_length < required as u32 {
                    return false;
                }
                core::ptr::write(buffer as *mut sid, tok.user);
                true
            }
            super::token::TOKEN_GROUPS => {
                let required = 4 + tok.group_count * core::mem::size_of::<TokenGroup>();
                *return_length = required as u32;
                if buffer.is_null() || buffer_length < required as u32 {
                    return false;
                }
                core::ptr::write(buffer as *mut u32, tok.group_count as u32);
                let groups_ptr = buffer.add(4) as *mut TokenGroup;
                for i in 0..tok.group_count {
                    core::ptr::write(groups_ptr.add(i), tok.groups[i]);
                }
                true
            }
            super::token::TOKEN_PRIVILEGES => {
                let required = 4 + tok.privilege_count * core::mem::size_of::<TokenPrivilege>();
                *return_length = required as u32;
                if buffer.is_null() || buffer_length < required as u32 {
                    return false;
                }
                core::ptr::write(buffer as *mut u32, tok.privilege_count as u32);
                let privs_ptr = buffer.add(4) as *mut TokenPrivilege;
                for i in 0..tok.privilege_count {
                    core::ptr::write(privs_ptr.add(i), tok.privileges[i]);
                }
                true
            }
            super::token::TOKEN_TYPE => {
                let required = core::mem::size_of::<TokenTypeEnum>();
                *return_length = required as u32;
                if buffer.is_null() || buffer_length < required as u32 {
                    return false;
                }
                core::ptr::write(buffer as *mut TokenTypeEnum, tok.token_type);
                true
            }
            super::token::TOKEN_IMPERSONATION_LEVEL => {
                let required = core::mem::size_of::<ImpersonationLevel>();
                *return_length = required as u32;
                if buffer.is_null() || buffer_length < required as u32 {
                    return false;
                }
                core::ptr::write(buffer as *mut ImpersonationLevel, tok.impersonation_level);
                true
            }
            super::token::TOKEN_SESSION_ID => {
                let required = core::mem::size_of::<u32>();
                *return_length = required as u32;
                if buffer.is_null() || buffer_length < required as u32 {
                    return false;
                }
                core::ptr::write(buffer as *mut u32, tok.session_id);
                true
            }
            super::token::TOKEN_INTEGRITY_LEVEL => {
                let required = core::mem::size_of::<u32>();
                *return_length = required as u32;
                if buffer.is_null() || buffer_length < required as u32 {
                    return false;
                }
                core::ptr::write(buffer as *mut u32, tok.integrity_level_rid);
                true
            }
            _ => {
                *return_length = 0;
                false
            }
        }
    }
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
pub unsafe fn se_set_information_token(
    token: *mut Token,
    info_class: u32,
    buffer: *const u8,
    buffer_length: u32,
) -> bool {
    if token.is_null() || buffer.is_null() {
        return false;
    }

    unsafe {
        let tok = &mut *token;

        match info_class {
            super::token::TOKEN_SESSION_ID => {
                if buffer_length < core::mem::size_of::<u32>() as u32 {
                    return false;
                }
                tok.session_id = *(buffer as *const u32);
                true
            }
            super::token::TOKEN_INTEGRITY_LEVEL => {
                if buffer_length < core::mem::size_of::<u32>() as u32 {
                    return false;
                }
                tok.integrity_level_rid = *(buffer as *const u32);
                true
            }
            _ => false,
        }
    }
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
pub unsafe fn se_token_type(token: *const Token) -> TokenTypeEnum {
    if token.is_null() {
        return TokenTypeEnum::TokenPrimary;
    }
    unsafe { (*token).token_type }
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
pub unsafe fn se_token_impersonation_level(token: *const Token) -> ImpersonationLevel {
    if token.is_null() {
        return ImpersonationLevel::SecurityAnonymous;
    }
    unsafe { (*token).impersonation_level }
}

pub fn init() {}

#[cfg(test)]
mod token_capacity_tests {
    use super::*;
    use crate::se::sid::{sid, WellKnownSid};
    use crate::se::token::{Token, SE_PRIVILEGE_ENABLED};

    // Under libtest the kernel heap is never initialised, so the pool
    // allocation inside se_create_token would fail (same pattern as
    // seaccess tests and io::tests).
    fn ensure_test_heap() {
        if crate::mm::heap::KERNEL_HEAP.configured_region().is_some() {
            return;
        }
        const TEST_HEAP_LEN: usize = 4 * 1024 * 1024;
        static mut TEST_HEAP: [u8; TEST_HEAP_LEN] = [0u8; TEST_HEAP_LEN];
        let base = core::ptr::addr_of_mut!(TEST_HEAP) as *mut u8;
        crate::mm::heap::KERNEL_HEAP.configure(base, TEST_HEAP_LEN);
    }

    fn make_token(groups: &[TokenGroup], privileges: &[TokenPrivilege]) -> *mut Token {
        se_create_token(
            sid::well_known(WellKnownSid::Users),
            groups,
            privileges,
            TokenTypeEnum::TokenPrimary,
            ImpersonationLevel::SecurityImpersonation,
        )
    }

    #[test]
    fn create_token_rejects_oversized_group_list() {
        ensure_test_heap();
        let groups: alloc::vec::Vec<TokenGroup> = (0..TOKEN_MAX_GROUPS + 1)
            .map(|i| TokenGroup {
                sid: sid::well_known(WellKnownSid::Users),
                attributes: i as u32,
            })
            .collect();
        assert!(make_token(&groups, &[]).is_null());
    }

    #[test]
    fn create_token_copies_every_group_and_privilege() {
        ensure_test_heap();
        let groups: alloc::vec::Vec<TokenGroup> = (0..TOKEN_MAX_GROUPS)
            .map(|i| TokenGroup {
                sid: sid::well_known(WellKnownSid::Users),
                attributes: i as u32,
            })
            .collect();
        let privileges: alloc::vec::Vec<TokenPrivilege> = (0..TOKEN_MAX_PRIVILEGES)
            .map(|i| TokenPrivilege {
                luid: luid::from_u64(i as u64 + 1),
                attributes: SE_PRIVILEGE_ENABLED,
            })
            .collect();
        let token = make_token(&groups, &privileges);
        assert!(!token.is_null());
        unsafe {
            assert_eq!((*token).group_count, TOKEN_MAX_GROUPS);
            assert_eq!((*token).privilege_count, TOKEN_MAX_PRIVILEGES);
            for i in 0..TOKEN_MAX_GROUPS {
                assert!((*token).groups[i].sid.equals(&groups[i].sid));
            }
            for i in 0..TOKEN_MAX_PRIVILEGES {
                assert!((*token).privileges[i].luid.equals(&privileges[i].luid));
            }
        }
    }

    #[test]
    fn adjust_privileges_rejects_unknown_privilege() {
        ensure_test_heap();
        let existing = [TokenPrivilege {
            luid: luid::from_u64(1),
            attributes: SE_PRIVILEGE_ENABLED,
        }];
        let token = make_token(&[], &existing);
        assert!(!token.is_null());

        let added = [TokenPrivilege {
            luid: luid::from_u64(2),
            attributes: SE_PRIVILEGE_ENABLED,
        }];
        assert!(!unsafe { se_adjust_privileges_token(token, false, &added, None) });
        unsafe {
            assert_eq!((*token).privilege_count, 1);
            assert!((*token).privileges[0].luid.equals(&existing[0].luid));
        }
    }

    #[test]
    fn adjust_privileges_overflow_fails_explicitly() {
        ensure_test_heap();
        let privileges: alloc::vec::Vec<TokenPrivilege> = (0..TOKEN_MAX_PRIVILEGES)
            .map(|i| TokenPrivilege {
                luid: luid::from_u64(i as u64 + 1),
                attributes: SE_PRIVILEGE_ENABLED,
            })
            .collect();
        let token = make_token(&[], &privileges);
        assert!(!token.is_null());

        let overflow = [TokenPrivilege {
            luid: luid::from_u64(999),
            attributes: SE_PRIVILEGE_ENABLED,
        }];
        assert!(!unsafe { se_adjust_privileges_token(token, false, &overflow, None) });
        unsafe {
            assert_eq!((*token).privilege_count, TOKEN_MAX_PRIVILEGES);
        }
    }

    #[test]
    fn adjust_groups_rejects_unknown_group() {
        ensure_test_heap();
        let token = make_token(&[], &[]);
        assert!(!token.is_null());

        let added = [TokenGroup {
            sid: sid::well_known(WellKnownSid::Administrators),
            attributes: super::super::token::SE_GROUP_ENABLED,
        }];
        assert!(!unsafe { se_adjust_groups_token(token, false, &added, None) });
        unsafe {
            assert_eq!((*token).group_count, 0);
        }
    }
}
