//! Executive locale defaults. UI language changes are staged until committed.
//! The current kernel has one interactive session; all ISAs share this state.
use super::{status::*, types::NTSTATUS};
use crate::{ke::sync::Spinlock, mm::user_copy::write_user_object};

#[path = "locale/install_language.rs"]
pub mod install_language;
use install_language::INSTALL_LANGUAGE;

struct Defaults {
    system: u32,
    user: u32,
    language: Option<u16>,
    pending: Option<u16>,
}

static DEFAULTS: Spinlock<Defaults> = Spinlock::new(Defaults {
    system: 0x409,
    user: 0x409,
    language: None,
    pending: None,
});

pub extern "C" fn nt_query_default_locale(user: u8, output: *mut u32) -> NTSTATUS {
    let value = {
        let state = DEFAULTS.lock();
        if user != 0 {
            state.user
        } else {
            state.system
        }
    };
    write_user_object(output as u64, &value)
        .err()
        .unwrap_or(STATUS_SUCCESS)
}

pub extern "C" fn nt_set_default_locale(user: u8, locale: u32) -> NTSTATUS {
    if locale & 0xffff0000 != 0 {
        return STATUS_INVALID_PARAMETER;
    }
    if user == 0 && !super::privilege::enabled(7) {
        return STATUS_PRIVILEGE_NOT_HELD;
    }
    // Zero asks for a registry reload. Do not silently substitute en-US when
    // that persistence path is unavailable.
    if locale == 0 {
        return STATUS_NOT_SUPPORTED;
    }
    let mut state = DEFAULTS.lock();
    if user != 0 {
        state.user = locale;
    } else {
        state.system = locale;
    }
    STATUS_SUCCESS
}

pub extern "C" fn nt_query_install_ui_language(output: *mut u16) -> NTSTATUS {
    write_user_object(output as u64, &INSTALL_LANGUAGE.language())
        .err()
        .unwrap_or(STATUS_SUCCESS)
}

pub extern "C" fn nt_query_default_ui_language(output: *mut u16) -> NTSTATUS {
    let language = DEFAULTS
        .lock()
        .language
        .unwrap_or_else(|| INSTALL_LANGUAGE.language());
    write_user_object(output as u64, &language)
        .err()
        .unwrap_or(STATUS_SUCCESS)
}

pub extern "C" fn nt_set_default_ui_language(language: u16) -> NTSTATUS {
    let mut state = DEFAULTS.lock();
    if language == 0 {
        let Some(pending) = state.pending else {
            return STATUS_OBJECT_NAME_NOT_FOUND;
        };
        state.language = Some(pending);
    } else {
        state.pending = Some(language);
    }
    STATUS_SUCCESS
}

/// The caller mode comes from the trusted dispatch boundary, never a service
/// argument. Kernel callers of the original Nt entry are rejected.
pub fn flush_install_ui_language(user_mode: bool, language: u32, commit: u32) -> NTSTATUS {
    INSTALL_LANGUAGE.flush(
        user_mode,
        language,
        commit,
        super::registry::persist_install_ui_language,
    )
}

/// User SSDT adapter. The project's system-call entry captures user arguments;
/// kernel code must use `flush_install_ui_language` with its actual caller mode.
pub extern "C" fn nt_flush_install_ui_language_from_user(language: u32, commit: u32) -> NTSTATUS {
    flush_install_ui_language(true, language, commit)
}

pub extern "C" fn nt_is_ui_language_comitted() -> NTSTATUS {
    INSTALL_LANGUAGE.committed_status()
}
