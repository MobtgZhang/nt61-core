//! Security subject capture with a lifetime-pinned client token.
use crate::se::token::{luid, ImpersonationLevel, Token};

#[repr(C)]
pub struct SecuritySubjectContext {
    pub client_token: *mut Token,
    pub impersonation_level: ImpersonationLevel,
    pub primary_token: *mut Token,
    pub process_audit_id: luid,
    // Project extension after the NT context prefix: retain the client token.
    client_reference: Option<crate::ps::impersonation::ImpersonationReference>,
}

impl SecuritySubjectContext {
    pub fn new() -> Self {
        Self {
            client_token: core::ptr::null_mut(),
            impersonation_level: ImpersonationLevel::SecurityAnonymous,
            primary_token: core::ptr::null_mut(),
            process_audit_id: luid::new(),
            client_reference: None,
        }
    }

    pub fn capture() -> Self {
        let mut ctx = Self::new();

        // Windows NT semantics for SeCaptureSecuritySubjectContext:
        //   - Client token: the thread's impersonation token
        //     (ETHREAD -> ClientSecurity -> ClientToken).
        //   - Primary token: the process's primary token
        //     (EPROCESS -> Token).
        //   - Impersonation level: from the client token's
        //     impersonation level, or SecurityAnonymous if no client token.

        // 1. Client (impersonation) token from the calling ETHREAD.
        let thread = crate::ps::thread::get_current_ethread();
        if !thread.is_null() {
            if let Some(reference) = unsafe { (*thread).impersonation.reference() } {
                ctx.client_token = reference.token.token() as *const Token as *mut Token;
                ctx.impersonation_level = reference.level;
                ctx.client_reference = Some(reference);
            }

            // 2. Primary token from the owning EPROCESS.
            let process = unsafe { (*thread).threads_process };
            if !process.is_null() {
                let proc_token = unsafe { (*process).token.get_object() };
                if proc_token != 0 {
                    ctx.primary_token = proc_token as *mut Token;
                }
            }
        }

        ctx
    }

    pub fn release(&mut self) {
        self.client_reference = None;
        if !self.client_token.is_null() {
            self.client_token = core::ptr::null_mut();
        }
        if !self.primary_token.is_null() {
            self.primary_token = core::ptr::null_mut();
        }
    }

    pub fn effective_token(&self) -> *mut Token {
        if !self.client_token.is_null() {
            self.client_token
        } else {
            self.primary_token
        }
    }
}

impl Default for SecuritySubjectContext {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for SecuritySubjectContext {
    fn drop(&mut self) {
        self.release();
    }
}
