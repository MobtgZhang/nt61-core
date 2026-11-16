#[path = "../../../libs/ntdll/src/ntdll/atom.rs"]
pub mod atom;
#[path = "../../../libs/ntdll/src/ntdll/luid.rs"]
pub mod luid;
#[path = "../../../libs/ntdll/src/ntdll/status.rs"]
pub mod status;
pub mod types {
    pub type NTSTATUS = i32;
}
