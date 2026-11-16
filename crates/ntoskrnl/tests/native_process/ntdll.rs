pub mod file;
#[path = "../../../libs/ntdll/src/ntdll/process.rs"]
pub mod process;
#[path = "../../../libs/ntdll/src/ntdll/status.rs"]
pub mod status;
#[path = "../../../libs/ntdll/src/ntdll/types.rs"]
pub mod types;
pub mod timer {
    pub(crate) fn refresh_timer_at(_: &mut super::file::TimerState, _: u64, _: u64) {}
}
pub mod section {
    pub fn release_section(_: u64) {}
}
// These object kinds cannot be allocated by the process-only fixture. A call
// here indicates an incorrect handle kind or an unexpected cleanup path.
pub mod io_completion {
    pub fn close_port(_: u64) {
        panic!("process-only fixture closed an I/O completion port");
    }
    pub fn close_reserve(_: u64) {
        panic!("process-only fixture closed an I/O reserve");
    }
}
pub mod ktm {
    pub fn release_open(_: u64) {
        panic!("process-only fixture closed a transaction object");
    }
}
pub mod string {
    pub unsafe fn alloc_string(_: u32) -> *mut u8 {
        core::ptr::null_mut()
    }
}

#[allow(unused_imports)]
#[path = "../../../libs/ntdll/src/ntdll/object.rs"]
pub mod object;

#[path = "../../../libs/ntdll/src/ntdll/alert_state.rs"]
pub mod alert_state;
