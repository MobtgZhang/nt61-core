//! Host-only harness for register packing/table decoding; no privileged I/O.
//! Can also run independently: rustc --edition=2021 --test
//! crates/hal/tests/hardware_layout.rs -o /tmp/nt61-hal-layout-test
#![allow(dead_code)]
mod mm {
    pub mod syspte {
        pub fn map_io_space(_: u64, _: u64) -> Option<u64> {
            None
        }
        pub fn unmap_io_space(_: u64, _: u64) {}
    }
}
mod hal {
    pub mod x86_64 {
        pub mod pit {
            pub fn tsc_frequency_hz() -> u64 {
                1_000_000_000
            }
        }
    }
}
#[path = "../src/hal/x86_64/apic.rs"]
mod apic;
#[path = "../src/hal/x86_64/hpet.rs"]
mod hpet;
