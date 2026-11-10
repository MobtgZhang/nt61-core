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
        provider.register("User", "");
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
            credential.user == user && credential.password_hash == hash_password(password)
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
        let good = provider.authenticate("User", "");
        assert!(good.is_some());
        assert!(provider.authenticate("User", "bad").is_none());
    }

    #[test]
    fn sas_lock_and_unlock_use_secure_desktop() {
        let mut provider = AuthenticationProvider::new();
        assert_eq!(provider.press_sas(), DesktopSecurity::Winlogon);
        assert!(provider.authenticate("User", "").is_some());
        assert!(provider.lock());
        assert_eq!(provider.press_sas(), DesktopSecurity::Winlogon);
        assert!(!provider.unlock("User", "bad"));
        assert!(provider.unlock("User", ""));
        assert_eq!(provider.state, LogonState::Authenticated);
    }

    #[test]
    fn repeated_failures_are_reported_without_implicit_bypass() {
        let mut provider = AuthenticationProvider::new();
        for _ in 0..4 {
            assert!(provider.authenticate("User", "bad").is_none());
        }
        assert!(!provider.is_locked_out());
        assert!(provider.authenticate("User", "bad").is_none());
        assert!(provider.is_locked_out());
    }

    #[test]
    fn logout_clears_sensitive_session_state_without_implicit_bypass() {
        let mut provider = AuthenticationProvider::new();
        assert!(provider.authenticate("User", "").is_some());
        provider.authenticate("User", "bad");
        provider.logout();
        assert_eq!(provider.state, LogonState::Winlogon);
        assert_eq!(provider.failed_attempts, 0);
        assert!(!provider.is_locked_out());
    }
}
