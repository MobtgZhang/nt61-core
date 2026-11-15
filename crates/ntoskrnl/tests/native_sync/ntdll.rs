pub mod file;
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
