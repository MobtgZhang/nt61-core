//! Executive locale defaults. UI language changes are staged until committed.
//! The current kernel has one interactive session; all ISAs share this state.
use super::{status::*, types::NTSTATUS};
use crate::{ke::sync::Spinlock, mm::user_copy::write_user_object};

struct Defaults {
    system: u32,
    user: u32,
    language: u16,
    pending: Option<u16>,
}

static DEFAULTS: Spinlock<Defaults> = Spinlock::new(Defaults {
    system: 0x409,
    user: 0x409,
    language: 0x409,
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
    write_user_object(output as u64, &0x409u16)
        .err()
        .unwrap_or(STATUS_SUCCESS)
}

pub extern "C" fn nt_query_default_ui_language(output: *mut u16) -> NTSTATUS {
    let language = DEFAULTS.lock().language;
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
        state.language = pending;
    } else {
        state.pending = Some(language);
    }
    STATUS_SUCCESS
}
