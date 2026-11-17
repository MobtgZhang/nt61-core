//! Compatibility build entry; all application logic lives in applications/.
#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
#[path="../../applications/native/runtime.rs"]
mod runtime;
