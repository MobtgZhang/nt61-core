//! Win7 SP1 x64 guest service identities, never native LA64 SSDT indices.
//! Actual service conversion belongs to the checked common NT 6.1 dispatcher.

#![cfg(target_arch = "loongarch64")]

/// Win7 SP1 x64 indices from j00ru/windows-syscalls/x64/csv/nt.csv.
pub mod nt {
    pub const NT_ALLOCATE_VIRTUAL_MEMORY: u32 = 0x15;
    pub const NT_FREE_VIRTUAL_MEMORY: u32 = 0x1B;
    pub const NT_OPEN_FILE: u32 = 0x30;
    pub const NT_READ_FILE: u32 = 0x03;
    pub const NT_WRITE_FILE: u32 = 0x05;
    pub const NT_CLOSE: u32 = 0x0C;
    pub const NT_CREATE_FILE: u32 = 0x52;
    pub const NT_QUERY_INFORMATION_FILE: u32 = 0x0E;
    pub const NT_SET_INFORMATION_FILE: u32 = 0x24;
    pub const NT_TERMINATE_PROCESS: u32 = 0x29;
    pub const NT_TERMINATE_THREAD: u32 = 0x50;
    pub const NT_DELAY_EXECUTION: u32 = 0x31;
    pub const NT_WAIT_FOR_SINGLE_OBJECT: u32 = 0x01;
    pub const NT_WAIT_FOR_MULTIPLE_OBJECTS: u32 = 0x58;
    pub const NT_DEVICE_IO_CONTROL_FILE: u32 = 0x04;
    pub const NT_QUERY_VIRTUAL_MEMORY: u32 = 0x20;
    pub const NT_CREATE_SECTION: u32 = 0x47;
    pub const NT_MAP_VIEW_OF_SECTION: u32 = 0x25;
    pub const NT_UNMAP_VIEW_OF_SECTION: u32 = 0x27;
    pub const NT_OPEN_PROCESS_TOKEN: u32 = 0xF9;
}

/// Scalar native-number conversion is deliberately unavailable. Use
/// `common::btl::nt61::dispatch` with a complete guest register context and
/// typed services; matching a numeric host SSDT slot is not ABI conversion.
pub fn translate_nt_syscall(_nr: u32) -> Option<u64> { None }
