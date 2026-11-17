//! Built-in user database (SAM).
//!
//! A minimal Security Account Manager (SAM). Built-in accounts are an
//! immutable boot baseline. Runtime accounts are kept under a spinlock and
//! store salted password hashes. The production persistence backend is the
//! `SAM\Domains\Account\Users\<RID>` registry hive; this module provides the
//! allocation and verification semantics used by that backend.
//!
//!   SYSTEM              — S-1-5-18
//!   Administrators      — S-1-5-32-544
//!   nt61test            — S-1-5-21-0-0-0-1001
//!
//! Lookup is linear in the table (≤ a handful of entries), which is
//! fine for kernel boot-time decisions.
//!
//! Authorization decisions (token creation, privilege set, etc.)
//! continue to live in `se::token`. This module is purely the
//! "who exists" lookup.

use super::sid::{sid, WellKnownSid};
use crate::crypto::csprng;
use crate::crypto::hash::sha256;
use crate::ke::sync::Spinlock;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountKind {
    System,
    Administrators,
    User,
    Guest,
}

#[derive(Debug, Clone, Copy)]
pub struct AccountEntry {
    /// Stable relative identifier (RID). Built-in accounts use
    /// the well-known RIDs; `User` accounts use the placeholder
    /// `0x3E8 + slot`.
    pub rid: u32,
    pub sid: sid,
    pub kind: AccountKind,
    /// Whether the account is currently enabled. OpenSSH skips
    /// disabled accounts at logon.
    pub enabled: bool,
}

/// Runtime-only credentials for a dynamically created account.
#[derive(Clone)]
struct UserRecord {
    name: String,
    entry: AccountEntry,
    salt: [u8; 16],
    password_hash: [u8; 32],
    picture: u32,
}

const SYSTEM: AccountEntry = AccountEntry {
    rid: 18,
    sid: sid::well_known(WellKnownSid::LocalSystem),
    kind: AccountKind::System,
    enabled: true,
};

/// Built-in local Administrator, RID 500. The requested default is enabled
/// with an empty interactive password; groups such as Administrators are not users.
const ADMINISTRATOR: AccountEntry = AccountEntry {
    rid: 500,
    sid: sid::with_authority_and_subs_arr([0,0,0,0,0,5],5,21,0,0,0,500,0,0,0),
    kind: AccountKind::Administrators,
    enabled: true,
};

const ADMINS: AccountEntry = AccountEntry {
    rid: 544,
    sid: sid::well_known(WellKnownSid::Administrators),
    kind: AccountKind::Administrators,
    enabled: true,
};

const GUESTS: AccountEntry = AccountEntry {
    rid: 546,
    sid: sid::well_known(WellKnownSid::Guests),
    kind: AccountKind::Guest,
    enabled: false,
};

const NT61TEST: AccountEntry = AccountEntry {
    rid: 1001,
    sid: sid::with_2subs([0, 0, 0, 0, 0, 5], 21, 1001),
    kind: AccountKind::User,
    enabled: true,
};

const GUEST: AccountEntry = AccountEntry {
    rid: 501, sid: sid::with_2subs([0,0,0,0,0,5],21,501), kind: AccountKind::Guest, enabled: false,
};
const BUILTIN: &[AccountEntry] = &[SYSTEM, ADMINISTRATOR, GUEST, ADMINS, GUESTS, NT61TEST];

/// Runtime-created accounts. `Vec` is intentionally small and bounded by the
/// caller; a real registry-backed SAM can replace this storage later.
static RUNTIME_ACCOUNTS: Spinlock<Vec<UserRecord>> = Spinlock::new(Vec::new());
static SAM_MUTATION:Spinlock<()>=Spinlock::new(());
static NEXT_DYNAMIC_RID: AtomicU32 = AtomicU32::new(0x1002);

const MAX_NAME_LENGTH: usize = 64;

/// Errors returned when creating a runtime account.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateUserError {
    NameTooLong,
    EmptyName,
    DuplicateName,
    RidExhausted,
    InvalidName,
    OutOfMemory,
    PersistenceFailure,
}

fn password_hash(password: &[u8], salt: &[u8; 16]) -> [u8; 32] {
    let mut salted = alloc::vec::Vec::with_capacity(salt.len() + password.len());
    salted.extend_from_slice(salt);
    salted.extend_from_slice(password);
    sha256(&salted)
}

/// Look up an account by SID.
pub fn lookup_by_sid(sid: &sid) -> Option<AccountEntry> {
    if let Some(entry)=RUNTIME_ACCOUNTS.lock().iter().find(|record|record.entry.sid.equals(sid)).map(|r|r.entry) { return Some(entry); }
    BUILTIN.iter().find(|a|a.sid.equals(sid)).copied()
}

/// Look up an account by case-insensitive name. Names use the
/// Windows convention: `SYSTEM`, `Administrators`, `Guest`,
/// `nt61test`. The match is byte-wise for ASCII; Unicode
/// matches are not yet supported because we only have ASCII
/// account names.
pub fn lookup_by_name(name: &str) -> Option<AccountEntry> {
    let normalized = name.trim().trim_end_matches('\0');
    if let Some(entry)=RUNTIME_ACCOUNTS.lock().iter().find(|record|record.name.eq_ignore_ascii_case(normalized)).map(|r|r.entry) {return Some(entry);}
    BUILTIN.iter().find(|a|normalized.eq_ignore_ascii_case(builtin_name(a))).copied()
}

fn builtin_name(entry: &AccountEntry) -> &'static str {
    if entry.rid == 500 { return "Administrator"; }
    if entry.rid == 546 { return "Guests"; }
    match entry.kind {
        AccountKind::System => "SYSTEM",
        AccountKind::Administrators => "Administrators",
        AccountKind::User => "nt61test",
        AccountKind::Guest => "Guest",
    }
}

/// Return an iterator over the account table. Used by
/// `NetUserEnum` and similar enumeration APIs.
pub fn all_accounts() -> Vec<AccountEntry> {
    let mut accounts = Vec::new();
    let records=RUNTIME_ACCOUNTS.lock();
    accounts.extend(BUILTIN.iter().filter(|a|!records.iter().any(|r|r.entry.rid==a.rid)).copied());
    for record in records.iter() {
        accounts.push(record.entry);
    }
    accounts
}

pub fn init() {
    RUNTIME_ACCOUNTS.lock().clear();
    NEXT_DYNAMIC_RID.store(0x1002, Ordering::SeqCst);
    for user in crate::registry::cm::sam_runtime_users() {
        NEXT_DYNAMIC_RID.fetch_max(user.rid.saturating_add(1), Ordering::SeqCst);
        let mut account = BUILTIN.iter().find(|a|a.rid==user.rid).copied().unwrap_or(AccountEntry {
            rid: user.rid,
            sid: sid::with_2subs([0, 0, 0, 0, 0, 5], 21, user.rid),
            kind: AccountKind::User,
            enabled: user.enabled,
        });
        account.enabled=user.enabled;
        if account.rid!=18 && account.rid!=544 && account.rid!=546 {account.kind=if user.administrator {AccountKind::Administrators} else if account.rid==501 {AccountKind::Guest} else {AccountKind::User};}
        RUNTIME_ACCOUNTS.lock().push(UserRecord {
            picture:user.picture,
            name: user.name,
            entry: account,
            salt: user.salt,
            password_hash: user.password_hash,
        });
    }
}

/// Create a runtime account with a salted password hash and a fresh RID.
pub fn create_user(
    name: &str,
    password: &[u8],
    enabled: bool,
) -> Result<AccountEntry, CreateUserError> {
    let _mutation=SAM_MUTATION.lock();
    let name = name.trim().trim_end_matches('\0');
    if name.is_empty() {
        return Err(CreateUserError::EmptyName);
    }
    if !valid_account_name(name) {return Err(CreateUserError::InvalidName);}
    if name.len() > MAX_NAME_LENGTH {
        return Err(CreateUserError::NameTooLong);
    }

    let mut accounts = RUNTIME_ACCOUNTS.lock();
    if accounts
        .iter()
        .any(|record| record.name.as_str().eq_ignore_ascii_case(name))
        || BUILTIN
            .iter()
            .any(|entry| builtin_name(entry).eq_ignore_ascii_case(name))
    {
        return Err(CreateUserError::DuplicateName);
    }

    let rid = loop {
        let current = NEXT_DYNAMIC_RID.load(Ordering::SeqCst);
        let next = current
            .checked_add(1)
            .ok_or(CreateUserError::RidExhausted)?;
        if NEXT_DYNAMIC_RID
            .compare_exchange_weak(current, next, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            break current;
        }
    };

    let mut salt = [0u8; 16];
    if !csprng::fill(&mut salt) {
        return Err(CreateUserError::OutOfMemory);
    }
    let entry = AccountEntry {
        rid,
        sid: sid::with_2subs([0, 0, 0, 0, 0, 5], 21, rid),
        kind: AccountKind::User,
        enabled,
    };
    accounts.push(UserRecord {
        name: String::from(name),
        entry,
        salt,
        password_hash: password_hash(password, &salt),
        picture: 0,
    });
    drop(accounts);

    crate::registry::cm::add_sam_runtime_user(crate::registry::cm::SamRuntimeUser {
        administrator:false, picture:0,
        rid,
        name: String::from(name),
        enabled,
        salt,
        password_hash: password_hash(password, &salt),
    });
    if !crate::registry::flush::flush_sam_hive(){
        RUNTIME_ACCOUNTS.lock().retain(|r|r.entry.rid!=rid);
        crate::registry::cm::restore_sam_runtime_user(rid,None);
        return Err(CreateUserError::PersistenceFailure);
    }
    Ok(entry)
}

/// Verify a runtime account password. Unknown accounts always fail.
pub fn verify_runtime_password(name: &str, password: &[u8]) -> bool {
    let normalized = name.trim().trim_end_matches('\0').to_ascii_lowercase();
    let accounts = RUNTIME_ACCOUNTS.lock();
    accounts
        .iter()
        .find(|record| record.name.as_str().eq_ignore_ascii_case(&normalized))
        .map(|record| {
            constant_time_eq(
                &record.password_hash,
                &password_hash(password, &record.salt),
            )
        })
        .unwrap_or(false)
}

/// Interactive credential verification; never accepts a built-in group as a
/// login account. Blank Administrator passwords are local-interactive only.
pub fn verify_interactive_password(name: &str, password: &[u8]) -> bool {
    let Some(account) = lookup_by_name(name).filter(|entry|entry.enabled) else {return false;};
    if account.rid == 500 && !RUNTIME_ACCOUNTS.lock().iter().any(|r|r.entry.rid==500) {return password.is_empty();}
    verify_runtime_password(name,password)
}

fn constant_time_eq(a: &[u8; 32], b: &[u8; 32]) -> bool {
    let mut diff = 0u8;
    for (left, right) in a.iter().zip(b.iter()) {
        diff |= left ^ right;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::se::sid::WellKnownSid;

    #[test]
    fn administrator_is_an_enabled_rid_500_user_with_empty_interactive_password() {
        let user=lookup_by_name("administrator").unwrap();
        assert_eq!(user.rid,500);assert!(user.enabled);
        assert_eq!(user.sid.sub_authority_count,5);
        assert!(verify_interactive_password("Administrator",b""));
        assert!(!verify_interactive_password("Administrator",b"wrong"));
        assert!(!verify_interactive_password("Administrators",b""));
        assert!(!verify_runtime_password("Administrator",b""));
    }

    #[test]
    fn lookup_system_by_name() {
        let sys = lookup_by_name("SYSTEM").unwrap();
        assert_eq!(sys.kind, AccountKind::System);
    }

    #[test]
    fn lookup_system_by_name_lowercase() {
        let sys = lookup_by_name("system").unwrap();
        assert_eq!(sys.kind, AccountKind::System);
    }

    #[test]
    fn lookup_system_by_sid() {
        let sid = sid::well_known(WellKnownSid::LocalSystem);
        let sys = lookup_by_sid(&sid).unwrap();
        assert_eq!(sys.rid, 18);
    }

    #[test]
    fn lookup_nt61test() {
        let u = lookup_by_name("nt61test").unwrap();
        assert_eq!(u.kind, AccountKind::User);
        assert_eq!(u.rid, 1001);
        assert!(u.enabled);
    }

    #[test]
    fn lookup_unknown_returns_none() {
        assert!(lookup_by_name("nobody").is_none());
    }

    #[test]
    fn creates_and_verifies_runtime_user() {
        init();
        let account = create_user("runtime-user", b"secret", true).unwrap();
        assert_eq!(account.rid, 0x1002);
        assert_eq!(account.kind, AccountKind::User);
        assert!(lookup_by_name("RUNTIME-USER").is_some());
        assert!(verify_runtime_password("runtime-user", b"secret"));
        assert!(!verify_runtime_password("runtime-user", b"wrong"));
        assert!(create_user("Runtime-User", b"secret", true).is_err());
    }
}

/// Owned account name, including dynamically created users.
pub fn account_name(identity:&sid)->Option<String>{
    if let Some(name)=RUNTIME_ACCOUNTS.lock().iter().find(|r|r.entry.sid.equals(identity)).map(|r|r.name.clone()){return Some(name);}
    BUILTIN.iter().find(|a|a.sid.equals(identity)).map(|a|String::from(builtin_name(a)))
}
/// Verify and replace a local interactive credential atomically. SAM stores
/// only the salted digest; callers own and erase the plaintext input.
pub fn change_interactive_password(name:&str,old:&[u8],new:&[u8])->bool{
    let _mutation=SAM_MUTATION.lock();
    if new.len()>32{return false;}
    let Some(entry)=lookup_by_name(name).filter(|a|a.enabled && a.kind!=AccountKind::System && a.rid!=544)else{return false;};
    let mut salt=[0u8;16];if !csprng::fill(&mut salt){return false;}
    let digest=password_hash(new,&salt);
    let previous=crate::registry::cm::sam_runtime_users().into_iter().find(|r|r.rid==entry.rid);
    let mut records=RUNTIME_ACCOUNTS.lock();
    let old_record=records.iter().find(|r|r.entry.rid==entry.rid).cloned();
    if let Some(record)=records.iter_mut().find(|r|r.name.eq_ignore_ascii_case(name)){
        if !constant_time_eq(&record.password_hash,&password_hash(old,&record.salt)){return false;}
        record.salt=salt;record.password_hash=digest;
    }else{
        if entry.rid!=500 || !old.is_empty(){return false;}
        records.push(UserRecord{name:String::from(name),entry,salt,password_hash:digest,picture:0});
    }
    let picture=records.iter().find(|r|r.entry.rid==entry.rid).map(|r|r.picture).unwrap_or(0);
    drop(records);
    crate::registry::cm::add_sam_runtime_user(crate::registry::cm::SamRuntimeUser{administrator:entry.kind==AccountKind::Administrators,picture,rid:entry.rid,name:String::from(name),enabled:entry.enabled,salt,password_hash:digest});
    if crate::registry::flush::flush_sam_hive(){return true;}
    let mut records=RUNTIME_ACCOUNTS.lock();records.retain(|r|r.entry.rid!=entry.rid);if let Some(old)=old_record{records.push(old);}
    crate::registry::cm::restore_sam_runtime_user(entry.rid,previous);false
}

/// Local interactive users only: groups, SYSTEM and credential-less test identities
/// never appear as login tiles.
pub fn interactive_accounts()->Vec<(u32,String,bool,bool,u32)> {
    let records=RUNTIME_ACCOUNTS.lock();
    let mut out=Vec::new();
    for entry in BUILTIN.iter().filter(|a|matches!(a.rid,500|501)) {
        if !records.iter().any(|r|r.entry.rid==entry.rid) {out.push((entry.rid,String::from(builtin_name(entry)),entry.enabled,entry.kind==AccountKind::Administrators,0));}
    }
    out.extend(records.iter().filter(|r|r.entry.rid==500||r.entry.rid==501||r.entry.rid>=1000).map(|r|(r.entry.rid,r.name.clone(),r.entry.enabled,r.entry.kind==AccountKind::Administrators,r.picture)));
    out.sort_by_key(|r|r.0);out
}
pub fn valid_account_name(name:&str)->bool {
    !name.is_empty() && name.len()<=64 && name.bytes().all(|b|b.is_ascii_alphanumeric()||matches!(b,b' '|b'_'|b'-'|b'.')) && name.trim()==name && !name.ends_with('.') && name!="." && name!=".."
}
/// Change account type, enabled state or bundled portrait without replacing credentials.
pub fn configure_account(name:&str,administrator:Option<bool>,enabled:Option<bool>,picture:Option<u32>)->bool {
    let _mutation=SAM_MUTATION.lock();
    if picture.is_some_and(|p|p>35) {return false;}
    let Some(entry)=lookup_by_name(name).filter(|a|a.rid==500||a.rid==501||a.rid>=1000) else{return false;};
    if entry.rid==500 && (administrator==Some(false)||enabled==Some(false)){return false;}
    let previous=crate::registry::cm::sam_runtime_users().into_iter().find(|r|r.rid==entry.rid);
    let mut records=RUNTIME_ACCOUNTS.lock();
    let old_record=records.iter().find(|r|r.entry.rid==entry.rid).cloned();
    if !records.iter().any(|r|r.entry.rid==entry.rid) {
        // Built-in credentials begin empty. Preserve the built-in SID.
        let mut salt=[0;16];if !csprng::fill(&mut salt){return false;}
        records.push(UserRecord{name:String::from(name),entry,salt,password_hash:password_hash(b"",&salt),picture:0});
    }
    let r=records.iter_mut().find(|r|r.entry.rid==entry.rid).unwrap();
    if let Some(admin)=administrator {r.entry.kind=if admin {AccountKind::Administrators}else if entry.rid==501 {AccountKind::Guest}else{AccountKind::User};}
    if let Some(enabled)=enabled {r.entry.enabled=enabled;}
    if let Some(picture)=picture {r.picture=picture;}
    let persisted=crate::registry::cm::SamRuntimeUser{rid:r.entry.rid,name:r.name.clone(),enabled:r.entry.enabled,administrator:r.entry.kind==AccountKind::Administrators,picture:r.picture,salt:r.salt,password_hash:r.password_hash};
    drop(records);
    crate::registry::cm::add_sam_runtime_user(persisted);
    if crate::registry::flush::flush_sam_hive(){return true;}
    let mut records=RUNTIME_ACCOUNTS.lock();records.retain(|r|r.entry.rid!=entry.rid);if let Some(old)=old_record{records.push(old);}
    crate::registry::cm::restore_sam_runtime_user(entry.rid,previous);false
}
