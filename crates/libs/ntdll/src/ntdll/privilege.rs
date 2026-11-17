//! Effective-token privilege checks shared by native system services.

pub(crate) fn enabled(number: u32) -> bool {
    let token = crate::ps::process::get_current_thread_token();
    // The scheduler owns the effective token for the running thread. An absent
    // token during early boot never grants a user-mode privilege.
    token.as_ref().is_some_and(|token| {
        token.has_privilege(&crate::se::token::luid {
            low_part: number,
            high_part: 0,
        })
    })
}

/// SYSTEM_LUID identifies the logon session, independently of enabled privileges.
pub(crate) fn system_logon() -> bool {
    let token = crate::ps::process::get_current_thread_token();
    // The scheduler retains the effective token of the current thread.
    token.as_ref().is_some_and(|token| {
        token.authentication_id.low_part == 0x3e7 && token.authentication_id.high_part == 0
    })
}
