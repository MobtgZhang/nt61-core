#[path = "../../../libs/ntdll/src/ntdll/clock.rs"]
pub mod clock;

// The clock wrapper fixture does not exercise delays. Keep that boundary
// explicit; production delay behavior is exercised by native_kernel32_sleep.
pub mod sync {
    pub fn nt_delay_execution_kernel(_: u8, _: i64) -> i32 {
        panic!("clock-only fixture entered delay execution");
    }
}

pub mod types {
    pub type NTSTATUS = i32;
}

pub mod status {
    pub type NTSTATUS = i32;
    pub const STATUS_SUCCESS: NTSTATUS = 0;
}
