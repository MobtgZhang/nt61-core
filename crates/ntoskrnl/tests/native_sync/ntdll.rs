pub mod file;
#[path = "../../../libs/ntdll/src/ntdll/legacy_services.rs"]
pub mod legacy_services;
#[path = "../../../libs/ntdll/src/ntdll/uuid.rs"]
pub mod uuid;
#[path = "../../../libs/ntdll/src/ntdll/job.rs"]
pub mod job;
#[path = "../../../libs/ntdll/src/ntdll/event_pair.rs"]
pub mod event_pair;
#[path = "../../../libs/ntdll/src/ntdll/keyed_event.rs"]
pub mod keyed_event;
#[path = "../../../libs/ntdll/src/ntdll/debug_filter.rs"]
pub mod debug_filter;
#[path = "../../../libs/ntdll/src/ntdll/timer_resolution.rs"]
pub mod timer_resolution;
pub mod privilege {
    std::thread_local! { pub static ENABLED: core::cell::Cell<bool> = const { core::cell::Cell::new(false) }; }
    pub fn system_logon() -> bool { ENABLED.with(|value| value.get()) }
    pub fn enabled(number: u32) -> bool {
        assert!(number == 20 || number == 13 || number == 7);
        ENABLED.with(|value| value.get())
    }
}
#[path = "../../../libs/ntdll/src/ntdll/status.rs"]
pub mod status;
#[path = "../../../libs/ntdll/src/ntdll/object.rs"]
pub mod object;
#[path = "../../../libs/ntdll/src/ntdll/sync.rs"]
pub mod sync;
#[path = "../../../libs/ntdll/src/ntdll/timer.rs"]
pub mod timer;
#[path = "../../../libs/ntdll/src/ntdll/types.rs"]
pub mod types;
#[path = "../../../libs/ntdll/src/ntdll/wait.rs"]
pub mod wait;
pub mod string {
    pub unsafe fn alloc_string(_: u32) -> *mut u8 {
        core::ptr::null_mut()
    }
}

#[path = "../../../libs/ntdll/src/ntdll/section.rs"]
pub mod section;

#[path = "../../../libs/ntdll/src/ntdll/alert_state.rs"]
pub mod alert_state;
#[path = "../../../libs/ntdll/src/ntdll/alert.rs"]
pub mod alert;

#[path = "../../../libs/ntdll/src/ntdll/apc.rs"]
pub mod apc;

#[path = "../../../libs/ntdll/src/ntdll/io_completion.rs"]
pub mod io_completion;

#[path = "../../../libs/ntdll/src/ntdll/namespace.rs"]
pub mod namespace;

#[path = "../../../libs/ntdll/src/ntdll/ktm.rs"]
pub mod ktm;

#[path = "../../../libs/ntdll/src/ntdll/locale.rs"]
pub mod locale;
// This sync-only harness has no registry tree. The native_registry harness
// separately exercises install-language writes against the production provider.
pub mod registry {
    pub fn persist_install_ui_language(_: u16) -> i32 {
        super::status::STATUS_OBJECT_NAME_NOT_FOUND
    }
}
#[path = "../../../libs/ntdll/src/ntdll/profile_interval.rs"]
pub mod profile_interval;

#[path = "../../../libs/ntdll/src/ntdll/luid.rs"]
pub mod luid;
#[path = "../../../libs/ntdll/src/ntdll/token_duplicate.rs"]
pub mod token_duplicate;

#[path = "../../../libs/ntdll/src/ntdll/token_privileges.rs"]
pub mod token_privileges;

#[path = "../../../libs/ntdll/src/ntdll/token_information.rs"]
pub mod token_information;

#[path = "../../../libs/ntdll/src/ntdll/token_groups.rs"]
pub mod token_groups;

pub mod security {
    pub use super::access_check::*;
    pub use super::token_information::*;
    pub use super::token_privileges::*;
    pub use super::token_groups::*;
}


#[path = "../../../libs/ntdll/src/ntdll/access_check.rs"]
pub mod access_check;

#[path = "../../../libs/ntdll/src/ntdll/token_filter.rs"]
pub mod token_filter;
