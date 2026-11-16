//! Effective-token reads retain the installed impersonation snapshot.
use super::impersonation::ImpersonationReference;
use crate::se::token::Token;

pub struct EffectiveTokenReference {
    impersonation: Option<ImpersonationReference>,
    primary: *const Token,
}

impl EffectiveTokenReference {
    pub fn as_ref(&self) -> Option<&Token> {
        if let Some(reference) = &self.impersonation {
            Some(reference.token.token())
        } else {
            // SAFETY: the current process owns its primary token. This preserves
            // the existing primary-token lifetime contract; process primary
            // token replacement still needs its own synchronized owner.
            unsafe { self.primary.as_ref() }
        }
    }

    pub fn is_null(&self) -> bool {
        self.as_ref().is_none()
    }
}

pub fn get_current_thread_token() -> EffectiveTokenReference {
    let mut reference = EffectiveTokenReference {
        impersonation: None,
        primary: core::ptr::null(),
    };
    let thread = crate::ps::thread::get_current_ethread();
    if thread.is_null() {
        return reference;
    }
    // SAFETY: the scheduler retains the current ETHREAD during the call. Pin
    // acquisition serializes with token replacement/revert/termination.
    unsafe {
        reference.impersonation = (*thread).impersonation.reference();
        let process = (*thread).threads_process;
        if !process.is_null() {
            reference.primary = (*process).token.get_object() as *const Token;
        }
    }
    reference
}
