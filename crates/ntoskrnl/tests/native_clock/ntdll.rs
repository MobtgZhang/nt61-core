#[path = "../../../libs/ntdll/src/ntdll/clock.rs"]
pub mod clock;
pub mod types {
    pub type NTSTATUS = i32;
}
pub mod status {
    pub type NTSTATUS = i32;
    pub const STATUS_SUCCESS: NTSTATUS = 0;
}
