//! Production native string exports, called through their real Microsoft x64 ABI.
//! rustc --edition=2021 --test crates/ntoskrnl/tests/native_strings.rs -o /tmp/nt61-native-strings
#![allow(dead_code)]
#[path = "../src/native_exports.rs"]
mod api;
#[path = "native_strings_cases.rs"]
mod cases;
