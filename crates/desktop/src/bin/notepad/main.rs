//! Compatibility build entry; all application logic lives in applications/.
#![no_std]
#![no_main]
#[path="../../applications/native/runtime.rs"]
mod runtime;
