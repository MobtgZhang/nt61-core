#[path = "../../../libs/kernel32/src/kernel32/sync.rs"]
pub mod sync;
#[path = "../../../libs/kernel32/src/kernel32/sync_ext.rs"]
pub mod sync_ext;
#[path = "../../../libs/kernel32/src/kernel32/handle.rs"]
pub mod handle;
#[path = "../../../libs/kernel32/src/kernel32/sleep.rs"]
pub mod sleep;
pub fn sleep_ex(milliseconds: u32, alertable: bool) -> u32 {
    sleep::sleep_ex(milliseconds, alertable)
}
#[path = "../../../libs/kernel32/src/kernel32/types.rs"]
pub mod types;
pub mod error {
    std::thread_local! { static ERROR: core::cell::Cell<u32> = const { core::cell::Cell::new(0) }; }
    pub fn set_last_error(value: u32) {
        ERROR.with(|error| error.set(value));
    }
    pub fn get_last_error() -> u32 {
        ERROR.with(|error| error.get())
    }
}
