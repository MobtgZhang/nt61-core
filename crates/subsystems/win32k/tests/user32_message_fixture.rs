//! Adapt only user callback invocation; queueing and translation execute the
//! production Rust user32 implementation.
pub use crate::types;

pub mod window {
    pub unsafe fn call_window_procedure(
        _: crate::types::HWND,
        _: u32,
        _: usize,
        _: isize,
    ) -> isize {
        0
    }
}

#[path = "../../../libs/user32/src/user32/message.rs"]
pub mod message;
