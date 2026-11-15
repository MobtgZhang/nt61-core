//! Exercise the production CMOS register protocol without privileged host I/O.
#![allow(dead_code)]
mod hal {
    pub mod common { pub mod acpi { pub fn rtc_century_register() -> Option<u8> { None } } }
    pub mod x86_64 {
        pub mod io_port {
            pub fn read_port_uchar(_: u16) -> u8 {
                panic!("unexpected hardware read");
            }
            pub fn write_port_uchar(_: u16, _: u8) {
                panic!("unexpected hardware write");
            }
        }
    }
}
#[path = "../src/hal/x86_64/cmos.rs"]
mod cmos;
