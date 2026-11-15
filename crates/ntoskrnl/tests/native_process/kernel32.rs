#[path = "../../../libs/kernel32/src/kernel32/process.rs"]
pub mod process;
#[path = "../../../libs/kernel32/src/kernel32/types.rs"]
pub mod types;
pub mod error {
    use std::sync::atomic::{AtomicU32, Ordering};
    static LAST_ERROR: AtomicU32 = AtomicU32::new(0);
    pub fn set_last_error(error: u32) {
        LAST_ERROR.store(error, Ordering::SeqCst);
    }
    pub fn get_last_error() -> u32 {
        LAST_ERROR.load(Ordering::SeqCst)
    }
}
pub mod sync {
    pub unsafe fn wait_for_single_object(_: super::types::HANDLE, _: u32) -> u32 {
        0xffffffff
    }
}
