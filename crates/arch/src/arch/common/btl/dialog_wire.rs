//! Reuse the pointer-free wire format in the standalone interpreter harness.
#[path = "../../../../../abi/src/dialog_bridge.rs"]
mod wire;
pub use wire::*;
