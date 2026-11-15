#[path = "../../../libs/ntdll/src/ntdll/clock.rs"]
pub mod clock;
// Delay behavior is exercised against production code in native_sync.
pub mod sync {
    pub fn nt_delay_execution_kernel(_: u8, _: i64) -> i32 {
        panic!("clock-only fixture entered delay execution")
    }
}
pub mod types {
    pub type NTSTATUS = i32;
}
pub mod status {
    pub type NTSTATUS = i32;
    pub const STATUS_SUCCESS: NTSTATUS = 0;
}
