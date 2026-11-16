//! Security Token
//
//! A TOKEN is attached to every process and thread. It carries
//! the security identity of the subject: the user SID, group SIDs,
//! privileges, and default DACL.
//
//! Token types:
//!   TokenPrimary    — attached to a process, used for all actions
//!   TokenImpersonation — attached to a thread, used to impersonate
//
//! Token groups contain:
//!   SID
//!   attributes (SE_GROUP_ENABLED, SE_GROUP_LOGON_ID, ...)
//
//! Token privileges contain:
//!   LUID (Locally Unique ID)
//!   attributes (SE_PRIVILEGE_ENABLED, ...)
//
//! References: Windows SDK winnt.h, WRK secobj.c

// Security subsystem uses NT naming (`TOKEN_USER`, `SID_*`,
// `SE_GROUP_ENABLED`, etc.). These names ARE the kernel
// security ABI and cannot be renamed.

use super::sid::{sid, WellKnownSid, SID_USERS};

/// Token information class values.
pub const TOKEN_USER: u32 = 1;
pub const TOKEN_GROUPS: u32 = 2;
pub const TOKEN_PRIVILEGES: u32 = 3;
pub const TOKEN_OWNER: u32 = 4;
pub const TOKEN_PRIMARY_GROUP: u32 = 5;
pub const TOKEN_DEFAULT_DACL: u32 = 6;
pub const TOKEN_SOURCE: u32 = 7;
pub const TOKEN_TYPE: u32 = 8;
pub const TOKEN_IMPERSONATION_LEVEL: u32 = 9;
pub const TOKEN_STATISTICS: u32 = 10;
pub const TOKEN_RESTRICTED_SIDS: u32 = 11;
pub const TOKEN_SESSION_ID: u32 = 12;
pub const TOKEN_GROUPS_AND_PRIVILEGES: u32 = 13;
pub const TOKEN_SESSION_REFERENCE: u32 = 14;
pub const TOKEN_SAND_BOX_INERT: u32 = 15;
pub const TOKEN_AUDIT_POLICY: u32 = 16;
pub const TOKEN_ORIGIN: u32 = 17;
pub const TOKEN_ELEVATION_TYPE: u32 = 18;
pub const TOKEN_LINKED_TOKEN: u32 = 19;
pub const TOKEN_ELEVATION: u32 = 20;
pub const TOKEN_HAS_RESTRICTIONS: u32 = 21;
pub const TOKEN_ACCESS_INFORMATION: u32 = 22;
pub const TOKEN_VIRTUALIZATION_ALLOWED: u32 = 23;
pub const TOKEN_VIRTUALIZATION_ENABLED: u32 = 24;
pub const TOKEN_INTEGRITY_LEVEL: u32 = 25;
pub const TOKEN_UIACCESS: u32 = 26;
pub const TOKEN_MANDATORY_POLICY: u32 = 27;
pub const TOKEN_LOGON_SID: u32 = 28;
pub const TOKEN_IS_APP_CONTAINER: u32 = 29;

/// Token types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
#[derive(Default)]
pub enum TokenTypeEnum {
    #[default]
    TokenPrimary = 1,
    TokenImpersonation = 2,
}

/// Token group attributes.
pub const SE_GROUP_MANDATORY: u32 = 0x00000001;
pub const SE_GROUP_ENABLED_BY_DEFAULT: u32 = 0x00000002;
pub const SE_GROUP_ENABLED: u32 = 0x00000004;
pub const SE_GROUP_OWNER: u32 = 0x00000008;
pub const SE_GROUP_USE_FOR_DENY_ONLY: u32 = 0x00000010;
pub const SE_GROUP_INTEGRITY: u32 = 0x00000020;
pub const SE_GROUP_INTEGRITY_ENABLED: u32 = 0x00000040;
pub const SE_GROUP_LOGON_ID: u32 = 0xC0000000;

/// Privilege attributes.
pub const SE_PRIVILEGE_ENABLED_BY_DEFAULT: u32 = 0x00000001;
pub const SE_PRIVILEGE_ENABLED: u32 = 0x00000002;
pub const SE_PRIVILEGE_REMOVED: u32 = 0x00000004;
pub const SE_PRIVILEGE_USED_FOR_ACCESS: u32 = 0x80000000;

/// LUID (Locally Unique ID) — a 64-bit identifier.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub struct luid {
    pub low_part: u32,
    pub high_part: i32,
}

impl luid {
    pub const fn new() -> Self {
        Self {
            low_part: 0,
            high_part: 0,
        }
    }

    pub const fn from_u64(v: u64) -> Self {
        Self {
            low_part: (v & 0xFFFFFFFF) as u32,
            high_part: (v >> 32) as i32,
        }
    }

    pub fn to_u64(&self) -> u64 {
        ((self.high_part as u64) << 32) | (self.low_part as u64)
    }

    pub fn equals(&self, other: &luid) -> bool {
        self.low_part == other.low_part && self.high_part == other.high_part
    }
}

impl core::fmt::Debug for luid {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Luid(0x{:08x}:{:08x})", self.high_part, self.low_part)
    }
}

/// Token group entry.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct TokenGroup {
    pub sid: sid,
    pub attributes: u32,
}

/// Group membership is fixed when a token is created. Adjustment can only
/// change the enabled bit, subject to mandatory and deny-only restrictions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupAdjustmentError {
    CannotDisableMandatory,
    CannotEnableDenyOnly,
}

impl TokenGroup {
    pub fn new(sid: sid, attributes: u32) -> Self {
        Self { sid, attributes }
    }

    pub fn is_enabled(&self) -> bool {
        (self.attributes & SE_GROUP_ENABLED) != 0
    }

    pub fn is_mandatory(&self) -> bool {
        (self.attributes & SE_GROUP_MANDATORY) != 0
    }

    pub fn is_logon_id(&self) -> bool {
        (self.attributes & SE_GROUP_LOGON_ID) != 0
    }

    pub fn adjusted_attributes(
        &self,
        reset_to_default: bool,
        requested: Option<u32>,
    ) -> Result<u32, GroupAdjustmentError> {
        let enabled = if reset_to_default {
            // Restricted groups remain deny-only, even if inherited defaults
            // originally enabled them before the token was restricted.
            if self.attributes & SE_GROUP_USE_FOR_DENY_ONLY != 0 {
                return Ok(self.attributes & !SE_GROUP_ENABLED);
            }
            self.attributes & (SE_GROUP_ENABLED_BY_DEFAULT | SE_GROUP_MANDATORY) != 0
        } else if let Some(attributes) = requested {
            attributes & SE_GROUP_ENABLED != 0
        } else {
            return Ok(self.attributes);
        };
        if !enabled && self.attributes & SE_GROUP_MANDATORY != 0 {
            return Err(GroupAdjustmentError::CannotDisableMandatory);
        }
        if enabled && self.attributes & SE_GROUP_USE_FOR_DENY_ONLY != 0 {
            return Err(GroupAdjustmentError::CannotEnableDenyOnly);
        }
        Ok((self.attributes & !SE_GROUP_ENABLED) | if enabled { SE_GROUP_ENABLED } else { 0 })
    }
}

/// Token privilege entry.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct TokenPrivilege {
    pub luid: luid,
    pub attributes: u32,
}

impl TokenPrivilege {
    pub fn new(luid: luid, attributes: u32) -> Self {
        Self { luid, attributes }
    }

    pub fn is_enabled(&self) -> bool {
        (self.attributes & SE_PRIVILEGE_ENABLED) != 0
    }

    pub fn enable(&mut self) {
        self.attributes |= SE_PRIVILEGE_ENABLED;
    }

    /// Only the enabled bit is adjustable. `None` means irreversible removal;
    /// removed privileges must not appear in a restorable previous-state list.
    pub fn adjusted_attributes(&self, disable_all: bool, requested: Option<u32>) -> Option<u32> {
        if disable_all {
            return Some(self.attributes & !SE_PRIVILEGE_ENABLED);
        }
        let Some(attributes) = requested else {
            return Some(self.attributes);
        };
        if attributes & SE_PRIVILEGE_REMOVED != 0 {
            return None;
        }
        Some((self.attributes & !SE_PRIVILEGE_ENABLED) | (attributes & SE_PRIVILEGE_ENABLED))
    }
}

/// Maximum groups per token.
pub const TOKEN_MAX_GROUPS: usize = 16;
/// Maximum privileges per token.
pub const TOKEN_MAX_PRIVILEGES: usize = 16;

/// Token flags.
pub const TOKEN_HAS_TRAVERSE_PRIVILEGE: u32 = 0x0001;
pub const TOKEN_HAS_BACKUP_PRIVILEGE: u32 = 0x0002;
pub const TOKEN_HAS_RESTORE_PRIVILEGE: u32 = 0x0004;
pub const TOKEN_WRITE_RESTRICTED: u32 = 0x0008;
pub const TOKEN_HAS_IMPERSONATE_PRIVILEGE: u32 = 0x0010;
// Internal state flags, independent of the public NtFilterToken option bits.
pub const TOKEN_IS_RESTRICTED: u32 = 0x0020;
pub const TOKEN_IS_FILTERED: u32 = 0x0040;
pub const TOKEN_SANDBOX_INERT: u32 = 0x0080;

/// Integrity level values.
pub const SECURITY_MANDATORY_UNTRUSTED_RID: u32 = 0x0000;
pub const SECURITY_MANDATORY_LOW_RID: u32 = 0x1000;
pub const SECURITY_MANDATORY_MEDIUM_RID: u32 = 0x2000;
pub const SECURITY_MANDATORY_HIGH_RID: u32 = 0x3000;
pub const SECURITY_MANDATORY_SYSTEM_RID: u32 = 0x4000;
pub const SECURITY_MANDATORY_PROTECTED_PROCESS_RID: u32 = 0x5000;

/// TOKEN_INFORMATION_CLASS: TOKEN_INTEGRITY_LEVEL
#[repr(C)]
pub struct TOKEN_INTEGRITY_LEVEL {
    pub label: sid,
    pub sub_auth_count: u32,
}

/// SECURITY_MANDATORY_LABEL_RID (in sub_auth[0] of a SID).
pub const SECURITY_MANDATORY_UNTRUSTED: u32 = 0x0000;
pub const SECURITY_MANDATORY_LOW: u32 = 0x1000;
pub const SECURITY_MANDATORY_MEDIUM: u32 = 0x2000;
pub const SECURITY_MANDATORY_HIGH: u32 = 0x3000;
pub const SECURITY_MANDATORY_SYSTEM: u32 = 0x4000;

/// TOKEN_SOURCE — identifies who created the token.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct TOKEN_SOURCE {
    pub source_name: [u8; 8],
    pub source_identifier: luid,
}

impl TOKEN_SOURCE {
    pub const fn from_name(name: &[u8; 8]) -> Self {
        Self {
            source_name: *name,
            source_identifier: luid::new(),
        }
    }
}

/// SECURITY_IMPERSONATION_LEVEL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
#[derive(Default)]
pub enum ImpersonationLevel {
    SecurityAnonymous = 0,
    #[default]
    SecurityIdentification = 1,
    SecurityImpersonation = 2,
    SecurityDelegation = 3,
}

/// TOKEN_CONTROL — unique identifier for a token.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct TokenId {
    pub token_id: luid,
    pub authentication_id: luid,
    pub token_type: TokenTypeEnum,
    pub impersonation_level: ImpersonationLevel,
    pub modified_id: luid,
}

/// A security token. This is the core security object attached
/// to processes and threads.
#[repr(C)]
pub struct Token {
    /// Token ID
    pub token_id: luid,
    /// Authentication ID (logon session)
    pub authentication_id: luid,
    /// User SID
    pub user: sid,
    /// A disabled user SID participates only in deny ACEs.
    pub user_attributes: u32,
    /// Primary group SID
    pub primary_group: sid,
    /// Default DACL
    pub default_dacl: *mut super::acl::Acl,
    /// Session ID
    pub session_id: u32,
    /// Token flags
    pub flags: u32,
    /// Integrity level RID
    pub integrity_level_rid: u32,
    /// Token type
    pub token_type: TokenTypeEnum,
    /// Impersonation level (for impersonation tokens)
    pub impersonation_level: ImpersonationLevel,
    /// Groups
    pub groups: [TokenGroup; TOKEN_MAX_GROUPS],
    /// Number of groups
    pub group_count: usize,
    /// Restricting SIDs are evaluated in a separate access-check pass.
    pub restricted_groups: [TokenGroup; TOKEN_MAX_GROUPS],
    pub restricted_count: usize,
    /// Privileges
    pub privileges: [TokenPrivilege; TOKEN_MAX_PRIVILEGES],
    /// Number of privileges
    pub privilege_count: usize,
    /// Token reference count (for reference counting)
    pub refcount: u32,
    _padding: [u8; 4],
}

impl Token {
    /// create a new primary token for a user.
    pub fn new_primary(user_sid: sid) -> Self {
        let mut t = Self::new();
        t.token_id = luid::new();
        t.authentication_id = luid::new();
        t.user = user_sid;
        t.primary_group = SID_USERS;
        t.token_type = TokenTypeEnum::TokenPrimary;
        t.default_dacl = core::ptr::null_mut();
        t.session_id = 0;
        t.flags = 0;
        t.integrity_level_rid = SECURITY_MANDATORY_MEDIUM;
        t.impersonation_level = ImpersonationLevel::SecurityImpersonation;
        t.refcount = 1;
        t
    }

    /// create a new impersonation token.
    pub fn new_impersonation(user_sid: sid, level: ImpersonationLevel) -> Self {
        let mut t = Self::new();
        t.token_id = luid::new();
        t.authentication_id = luid::new();
        t.user = user_sid;
        t.primary_group = SID_USERS;
        t.token_type = TokenTypeEnum::TokenImpersonation;
        t.impersonation_level = level;
        t.default_dacl = core::ptr::null_mut();
        t.refcount = 1;
        t
    }

    pub fn new() -> Self {
        Self {
            token_id: luid::new(),
            authentication_id: luid::new(),
            user: sid::well_known(WellKnownSid::Null),
            user_attributes: 0,
            primary_group: sid::well_known(WellKnownSid::Null),
            default_dacl: core::ptr::null_mut(),
            session_id: 0,
            flags: 0,
            integrity_level_rid: SECURITY_MANDATORY_MEDIUM,
            token_type: TokenTypeEnum::TokenPrimary,
            impersonation_level: ImpersonationLevel::SecurityImpersonation,
            groups: [TokenGroup {
                sid: sid {
                    revision: 1,
                    sub_authority_count: 0,
                    identifier_authority: [0; 6],
                    sub_authority: [0; 8],
                },
                attributes: 0,
            }; TOKEN_MAX_GROUPS],
            group_count: 0,
            restricted_groups: [TokenGroup::default(); TOKEN_MAX_GROUPS],
            restricted_count: 0,
            privileges: [TokenPrivilege {
                luid: luid::new(),
                attributes: 0,
            }; TOKEN_MAX_PRIVILEGES],
            privilege_count: 0,
            refcount: 0,
            _padding: [0; 4],
        }
    }

    /// Add a group to the token.
    pub fn add_group(&mut self, group: TokenGroup) {
        if self.group_count < TOKEN_MAX_GROUPS {
            self.groups[self.group_count] = group;
            self.group_count += 1;
        }
    }

    /// Add a privilege to the token.
    pub fn add_privilege(&mut self, privilege: TokenPrivilege) {
        if self.privilege_count < TOKEN_MAX_PRIVILEGES {
            self.privileges[self.privilege_count] = privilege;
            self.privilege_count += 1;
        }
    }

    /// Apply captured restrictions to an independently owned duplicate.
    /// Unsupported token-policy options fail before any mutation.
    pub fn filter(
        &mut self,
        flags: u32,
        disable: &[TokenGroup],
        delete: &[luid],
        restrict: &[TokenGroup],
    ) -> Result<(), i32> {
        if flags & !0xf != 0 || restrict.iter().any(|entry| entry.attributes != 0) {
            return Err(0xc000_000du32 as i32); // STATUS_INVALID_PARAMETER
        }
        // LUA elevation policy and write-only restriction need object-specific
        // policy support. Do not weaken either request into ordinary filtering.
        if flags & 0xc != 0 {
            return Err(0xc000_00bbu32 as i32); // STATUS_NOT_SUPPORTED
        }
        if restrict.len() > TOKEN_MAX_GROUPS {
            return Err(0xc000_009au32 as i32); // STATUS_INSUFFICIENT_RESOURCES
        }
        for entry in disable {
            if entry.sid.equals(&self.user) {
                self.user_attributes = (self.user_attributes
                    & !(SE_GROUP_ENABLED | SE_GROUP_ENABLED_BY_DEFAULT))
                    | SE_GROUP_USE_FOR_DENY_ONLY;
            }
            for group in &mut self.groups[..self.group_count] {
                if group.sid.equals(&entry.sid) {
                    group.attributes = (group.attributes
                        & !(SE_GROUP_ENABLED | SE_GROUP_ENABLED_BY_DEFAULT))
                        | SE_GROUP_USE_FOR_DENY_ONLY;
                }
            }
        }
        let mut count = 0;
        for index in 0..self.privilege_count {
            let privilege = self.privileges[index];
            let remove = if flags & 1 != 0 {
                privilege.luid != luid::from_u64(23) // SeChangeNotifyPrivilege
            } else {
                delete.contains(&privilege.luid)
            };
            if !remove {
                self.privileges[count] = privilege;
                count += 1;
            }
        }
        self.privileges[count..].fill(TokenPrivilege::default());
        self.privilege_count = count;
        if !restrict.is_empty() {
            let was_restricted = self.flags & TOKEN_IS_RESTRICTED != 0;
            let mut groups = [TokenGroup::default(); TOKEN_MAX_GROUPS];
            count = 0;
            for entry in restrict {
                if (!was_restricted
                    || self.restricted_groups[..self.restricted_count]
                        .iter()
                        .any(|old| old.sid.equals(&entry.sid)))
                    && !groups[..count].iter().any(|old| old.sid.equals(&entry.sid))
                {
                    groups[count] = TokenGroup::new(
                        entry.sid,
                        SE_GROUP_MANDATORY | SE_GROUP_ENABLED_BY_DEFAULT | SE_GROUP_ENABLED,
                    );
                    count += 1;
                }
            }
            self.restricted_groups = groups;
            self.restricted_count = count;
            // Keep this flag even when intersection is empty: an empty
            // restricting set must not turn a restricted token unrestricted.
            self.flags |= TOKEN_IS_RESTRICTED;
        }
        if flags & 2 != 0 {
            self.flags |= TOKEN_SANDBOX_INERT;
        }
        self.flags |= TOKEN_IS_FILTERED;
        self.refresh_privilege_flags();
        Ok(())
    }

    /// Check if the token has a specific privilege enabled.
    pub fn has_privilege(&self, luid: &luid) -> bool {
        for i in 0..self.privilege_count {
            if self.privileges[i].luid.equals(luid) && self.privileges[i].is_enabled() {
                return true;
            }
        }
        false
    }

    /// Keep the fast privilege checks consistent after adjustment or removal.
    pub fn refresh_privilege_flags(&mut self) {
        for (number, flag) in [
            (23, TOKEN_HAS_TRAVERSE_PRIVILEGE),
            (17, TOKEN_HAS_BACKUP_PRIVILEGE),
            (18, TOKEN_HAS_RESTORE_PRIVILEGE),
            (29, TOKEN_HAS_IMPERSONATE_PRIVILEGE),
        ] {
            self.flags &= !flag;
            if self.has_privilege(&luid::from_u64(number)) {
                self.flags |= flag;
            }
        }
    }

    /// Check if the token belongs to the administrators group.
    pub fn is_admin(&self) -> bool {
        self.is_local_system()
            || self.groups.iter().take(self.group_count).any(|group| {
                group.attributes & SE_GROUP_USE_FOR_DENY_ONLY == 0
                    && group
                        .sid
                        .equals(&sid::well_known(WellKnownSid::Administrators))
            })
    }

    /// Check if the token belongs to the LocalSystem account.
    pub fn is_local_system(&self) -> bool {
        self.user_attributes & SE_GROUP_USE_FOR_DENY_ONLY == 0
            && self
                .user
                .equals(&sid::well_known(WellKnownSid::LocalSystem))
    }

    /// Reference counting.
    pub fn add_ref(&mut self) {
        self.refcount += 1;
    }

    pub fn release(&mut self) -> u32 {
        if self.refcount > 0 {
            self.refcount -= 1;
        }
        self.refcount
    }
}

impl Default for Token {
    fn default() -> Self {
        Self::new()
    }
}

/// Well-known privilege LUIDs.
pub mod privileges {
    use super::luid;

    /// SeCreateTokenPrivilege
    pub const SE_CREATE_TOKEN_PRIVILEGE: luid = luid {
        low_part: 0x00000002,
        high_part: 0,
    };
    /// SeAssignPrimaryTokenPrivilege
    pub const SE_ASSIGN_PRIMARY_TOKEN_PRIVILEGE: luid = luid {
        low_part: 0x00000003,
        high_part: 0,
    };
    /// SeLockMemoryPrivilege
    pub const SE_LOCK_MEMORY_PRIVILEGE: luid = luid {
        low_part: 0x00000004,
        high_part: 0,
    };
    /// SeIncreaseQuotaPrivilege
    pub const SE_INCREASE_QUOTA_PRIVILEGE: luid = luid {
        low_part: 0x00000005,
        high_part: 0,
    };
    /// SeMachineAccountPrivilege
    pub const SE_MACHINE_ACCOUNT_PRIVILEGE: luid = luid {
        low_part: 0x00000006,
        high_part: 0,
    };
    /// SeTcbPrivilege
    pub const SE_TCB_PRIVILEGE: luid = luid {
        low_part: 0x00000007,
        high_part: 0,
    };
    /// SeSecurityPrivilege
    pub const SE_SECURITY_PRIVILEGE: luid = luid {
        low_part: 0x00000008,
        high_part: 0,
    };
    /// SeTakeOwnershipPrivilege
    pub const SE_TAKE_OWNERSHIP_PRIVILEGE: luid = luid {
        low_part: 0x00000009,
        high_part: 0,
    };
    /// SeLoadDriverPrivilege
    pub const SE_LOAD_DRIVER_PRIVILEGE: luid = luid {
        low_part: 0x0000000A,
        high_part: 0,
    };
    /// SeSystemProfilePrivilege
    pub const SE_SYSTEM_PROFILE_PRIVILEGE: luid = luid {
        low_part: 0x0000000B,
        high_part: 0,
    };
    /// SeSystemtimePrivilege
    pub const SE_SYSTEMTIME_PRIVILEGE: luid = luid {
        low_part: 0x0000000C,
        high_part: 0,
    };
    /// SeProfileSingleProcessPrivilege
    pub const SE_PROFILE_SINGLE_PROCESS_PRIVILEGE: luid = luid {
        low_part: 0x0000000D,
        high_part: 0,
    };
    /// SeIncreaseBasePriorityPrivilege
    pub const SE_INCREASE_BASE_PRIORITY_PRIVILEGE: luid = luid {
        low_part: 0x0000000E,
        high_part: 0,
    };
    /// SeCreatePagefilePrivilege
    pub const SE_CREATE_PAGEFILE_PRIVILEGE: luid = luid {
        low_part: 0x0000000F,
        high_part: 0,
    };
    /// SeCreatePermanentPrivilege
    pub const SE_CREATE_PERMANENT_PRIVILEGE: luid = luid {
        low_part: 0x00000010,
        high_part: 0,
    };
    /// SeBackupPrivilege
    pub const SE_BACKUP_PRIVILEGE: luid = luid {
        low_part: 0x00000011,
        high_part: 0,
    };
    /// SeRestorePrivilege
    pub const SE_RESTORE_PRIVILEGE: luid = luid {
        low_part: 0x00000012,
        high_part: 0,
    };
    /// SeShutdownPrivilege
    pub const SE_SHUTDOWN_PRIVILEGE: luid = luid {
        low_part: 0x00000013,
        high_part: 0,
    };
    /// SeDebugPrivilege
    pub const SE_DEBUG_PRIVILEGE: luid = luid {
        low_part: 0x00000014,
        high_part: 0,
    };
    /// SeAuditPrivilege
    pub const SE_AUDIT_PRIVILEGE: luid = luid {
        low_part: 0x00000015,
        high_part: 0,
    };
    /// SeSystemEnvironmentPrivilege
    pub const SE_SYSTEM_ENVIRONMENT_PRIVILEGE: luid = luid {
        low_part: 0x00000016,
        high_part: 0,
    };
    /// SeChangeNotifyPrivilege
    pub const SE_CHANGE_NOTIFY_PRIVILEGE: luid = luid {
        low_part: 0x00000017,
        high_part: 0,
    };
    /// SeRemoteShutdownPrivilege
    pub const SE_REMOTE_SHUTDOWN_PRIVILEGE: luid = luid {
        low_part: 0x00000018,
        high_part: 0,
    };
    /// SeUndockPrivilege
    pub const SE_UNDOCK_PRIVILEGE: luid = luid {
        low_part: 0x00000019,
        high_part: 0,
    };
    /// SeSyncAgentPrivilege
    pub const SE_SYNC_AGENT_PRIVILEGE: luid = luid {
        low_part: 0x0000001A,
        high_part: 0,
    };
    /// SeEnableDelegationPrivilege
    pub const SE_ENABLE_DELEGATION_PRIVILEGE: luid = luid {
        low_part: 0x0000001B,
        high_part: 0,
    };
    /// SeManageVolumePrivilege
    pub const SE_MANAGE_VOLUME_PRIVILEGE: luid = luid {
        low_part: 0x0000001C,
        high_part: 0,
    };
    /// SeImpersonatePrivilege
    pub const SE_IMPERSONATE_PRIVILEGE: luid = luid {
        low_part: 0x0000001D,
        high_part: 0,
    };
    /// SeCreateGlobalPrivilege
    pub const SE_CREATE_GLOBAL_PRIVILEGE: luid = luid {
        low_part: 0x0000001E,
        high_part: 0,
    };
    /// SeTrustedCredManAccessPrivilege
    pub const SE_TRUSTED_CREDMAN_ACCESS_PRIVILEGE: luid = luid {
        low_part: 0x0000001F,
        high_part: 0,
    };
    /// SeRelabelPrivilege
    pub const SE_RELABEL_PRIVILEGE: luid = luid {
        low_part: 0x00000020,
        high_part: 0,
    };
    /// SeTimeZonePrivilege
    pub const SE_TIME_ZONE_PRIVILEGE: luid = luid {
        low_part: 0x00000021,
        high_part: 0,
    };
    /// SeCreateSymbolicLinkPrivilege
    pub const SE_CREATE_SYMBOLIC_LINK_PRIVILEGE: luid = luid {
        low_part: 0x00000022,
        high_part: 0,
    };
}

pub fn init() {
    // crate::kprintln!("    SE/TOKEN: initialized (Token, group, Privilege, LUID, well-known SIDs)");
}

/// create a system token for kernel processes.
/// Returns a pointer to the allocated token.
pub fn create_system_token() -> *mut Token {
    use super::sid::sid;
    use super::sid::WellKnownSid;

    let token = crate::mm::pool::allocate(
        crate::mm::pool::PoolType::NonPaged,
        core::mem::size_of::<Token>(),
    ) as *mut Token;

    if !token.is_null() {
        // SYSTEM_LUID identifies the built-in LocalSystem logon session.
        // A SYSTEM SID alone must not be used to infer this identity later.
        let mut system_token = Token::new_primary(sid::well_known(WellKnownSid::LocalSystem));
        system_token.authentication_id = luid::from_u64(0x3e7);
        // SAFETY: allocation returned suitably aligned, exclusive storage for
        // one Token. It is initialized before its pointer is published.
        unsafe {
            core::ptr::write(token, system_token);
        }
        // crate::kprintln!("[SEC] Created system token at 0x{:016x}", token as u64);
    }
    token
}

/// create a user token for user-mode processes.
/// Returns a pointer to the allocated token.
pub fn create_user_token(user_sid: sid) -> *mut Token {
    let token = crate::mm::pool::allocate(
        crate::mm::pool::PoolType::NonPaged,
        core::mem::size_of::<Token>(),
    ) as *mut Token;

    if !token.is_null() {
        unsafe {
            core::ptr::write(token, Token::new_primary(user_sid));
        }
        // crate::kprintln!("[SEC] Created user token at 0x{:016x}", token as u64);
    }
    token
}

/// Copy a fully-configured Token (with privileges, groups, default
/// DACL) into a fresh pool allocation. Used by logon paths that
/// build the token in stack memory first.
pub fn persist_token(template: &Token) -> *mut Token {
    let token = crate::mm::pool::allocate(
        crate::mm::pool::PoolType::NonPaged,
        core::mem::size_of::<Token>(),
    ) as *mut Token;

    if !token.is_null() {
        unsafe {
            let src = template as *const Token as *const u8;
            core::ptr::copy_nonoverlapping(src, token as *mut u8, core::mem::size_of::<Token>());
        }
    }
    token
}

/// Duplicate a token for a new process, including the default DACL.
pub fn duplicate_for_process(template: &Token) -> *mut Token {
    let token = persist_token(template);
    if token.is_null() {
        return core::ptr::null_mut();
    }

    unsafe {
        if !(*token).default_dacl.is_null() {
            let acl = crate::mm::pool::allocate(
                crate::mm::pool::PoolType::NonPaged,
                core::mem::size_of::<super::acl::Acl>(),
            ) as *mut super::acl::Acl;
            if acl.is_null() {
                crate::mm::pool::free(token as *mut u8);
                return core::ptr::null_mut();
            }
            core::ptr::copy_nonoverlapping(
                (*token).default_dacl as *const u8,
                acl as *mut u8,
                core::mem::size_of::<super::acl::Acl>(),
            );
            (*token).default_dacl = acl;
        }
    }
    token
}
