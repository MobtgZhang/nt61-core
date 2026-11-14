use crate::window::DesktopSecurity;
use alloc::string::String;
use alloc::vec::Vec;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogonState {
    Winlogon,
    Authenticated,
    Locked,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Credential {
    pub user: String,
    password_hash: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Authentication {
    pub user: String,
    pub session_id: u32,
    pub desktop: DesktopSecurity,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthenticationProvider {
    pub credentials: Vec<Credential>,
    pub next_session: u32,
    pub state: LogonState,
    pub failed_attempts: u32,
}

impl AuthenticationProvider {
    pub fn new() -> Self {
        let mut provider = Self {
            credentials: Vec::new(),
            next_session: 1,
            state: LogonState::Winlogon,
            failed_attempts: 0,
        };
        provider.register("Administrator", "");
        provider
    }

    pub fn register(&mut self, user: &str, password: &str) {
        self.credentials.push(Credential {
            user: String::from(user),
            password_hash: hash_password(password),
        });
    }

    pub fn press_sas(&mut self) -> DesktopSecurity {
        self.state = match self.state {
            LogonState::Authenticated | LogonState::Locked => LogonState::Locked,
            LogonState::Winlogon => LogonState::Winlogon,
        };
        DesktopSecurity::Winlogon
    }

    pub fn authenticate(&mut self, user: &str, password: &str) -> Option<Authentication> {
        let matches = self.credentials.iter().any(|credential| {
            credential.user.eq_ignore_ascii_case(user) && credential.password_hash == hash_password(password)
        });
        if matches {
            let session_id = self.next_session;
            self.next_session += 1;
            self.state = LogonState::Authenticated;
            self.failed_attempts = 0;
            Some(Authentication {
                user: String::from(user),
                session_id,
                desktop: DesktopSecurity::Default,
            })
        } else {
            self.failed_attempts += 1;
            None
        }
    }

    pub fn lock(&mut self) -> bool {
        if self.state == LogonState::Authenticated {
            self.state = LogonState::Locked;
            true
        } else {
            false
        }
    }

    pub fn logout(&mut self) {
        self.state = LogonState::Winlogon;
        self.failed_attempts = 0;
    }

    pub fn unlock(&mut self, user: &str, password: &str) -> bool {
        if self.state != LogonState::Locked {
            return false;
        }
        let authenticated = self.authenticate(user, password);
        if authenticated.is_some() {
            self.state = LogonState::Authenticated;
            true
        } else {
            self.state = LogonState::Locked;
            false
        }
    }

    pub fn is_locked_out(&self) -> bool {
        self.failed_attempts >= 5
    }
}

impl Default for AuthenticationProvider {
    fn default() -> Self {
        Self::new()
    }
}

fn hash_password(password: &str) -> u32 {
    password.bytes().fold(0x811C_9DC5, |hash, byte| {
        (hash ^ byte as u32).wrapping_mul(0x0100_0193)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secure_logon_creates_new_session_and_wrong_password_fails() {
        let mut provider = AuthenticationProvider::new();
        let good = provider.authenticate("Administrator", "");
        assert!(good.is_some());
        assert!(provider.authenticate("Administrator", "bad").is_none());
    }

    #[test]
    fn sas_lock_and_unlock_use_secure_desktop() {
        let mut provider = AuthenticationProvider::new();
        assert_eq!(provider.press_sas(), DesktopSecurity::Winlogon);
        assert!(provider.authenticate("Administrator", "").is_some());
        assert!(provider.lock());
        assert_eq!(provider.press_sas(), DesktopSecurity::Winlogon);
        assert!(!provider.unlock("Administrator", "bad"));
        assert!(provider.unlock("Administrator", ""));
        assert_eq!(provider.state, LogonState::Authenticated);
    }

    #[test]
    fn repeated_failures_are_reported_without_implicit_bypass() {
        let mut provider = AuthenticationProvider::new();
        for _ in 0..4 {
            assert!(provider.authenticate("Administrator", "bad").is_none());
        }
        assert!(!provider.is_locked_out());
        assert!(provider.authenticate("Administrator", "bad").is_none());
        assert!(provider.is_locked_out());
    }

    #[test]
    fn logout_clears_sensitive_session_state_without_implicit_bypass() {
        let mut provider = AuthenticationProvider::new();
        assert!(provider.authenticate("Administrator", "").is_some());
        provider.authenticate("Administrator", "bad");
        provider.logout();
        assert_eq!(provider.state, LogonState::Winlogon);
        assert_eq!(provider.failed_attempts, 0);
        assert!(!provider.is_locked_out());
    }
}

/// Password entry shared by LogonUI and host checks. Never formats or clones
/// credentials; erasure uses volatile stores so dead-store elimination cannot
/// retain the previous password after a failed or successful logon.
pub struct PasswordEntry {
    bytes: [u8; 32],
    length: usize,
}
impl PasswordEntry {
    pub fn new() -> Self { Self { bytes: [0; 32], length: 0 } }
    pub fn len(&self) -> usize { self.length }
    pub fn is_empty(&self) -> bool { self.length == 0 }
    pub fn append(&mut self, byte: u8) {
        if self.length < self.bytes.len() {
            self.bytes[self.length] = byte; self.length += 1;
        }
    }
    pub fn backspace(&mut self) {
        if self.length != 0 {
            self.length -= 1;
            // SAFETY: length is a valid index into our exclusively owned array.
            unsafe { core::ptr::write_volatile(&mut self.bytes[self.length], 0); }
        }
    }
    pub fn clear(&mut self) {
        for byte in &mut self.bytes {
            // SAFETY: each byte is an exclusive reference to live array storage.
            unsafe { core::ptr::write_volatile(byte, 0); }
        }
        self.length = 0;
    }
    pub fn as_str(&self) -> Option<&str> {
        core::str::from_utf8(&self.bytes[..self.length]).ok()
    }
}
impl Default for PasswordEntry { fn default() -> Self { Self::new() } }
impl Drop for PasswordEntry { fn drop(&mut self) { self.clear(); } }
#[cfg(test)] mod password_tests {
    use super::PasswordEntry;
    #[test] fn deleting_and_clearing_erase_password_bytes() {
        let mut entry=PasswordEntry::new();
        for byte in b"secret" { entry.append(*byte); }
        entry.backspace(); assert_eq!(entry.as_str(),Some("secre"));
        assert_eq!(entry.bytes[5],0);
        entry.clear(); assert!(entry.is_empty()); assert_eq!(entry.bytes,[0;32]);
        entry.backspace(); assert_eq!(entry.len(),0);
    }
    #[test] fn password_capacity_is_bounded_and_a_fresh_entry_is_empty() {
        let mut entry=PasswordEntry::new();
        assert_eq!(entry.as_str(),Some(""));
        for _ in 0..100 { entry.append(b'x'); }
        assert_eq!(entry.len(),32); entry.clear(); entry.append(b'y');
        assert_eq!(entry.as_str(),Some("y")); assert!(entry.bytes[1..].iter().all(|b|*b==0));
    }
}
