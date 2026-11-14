//! NT 6.1 guest service bridge. Guest indices are resolved independently
//! of the host SSDT. The portable runner supplies typed process/VFS services.

#![cfg(target_arch = "loongarch64")]

pub mod nt;
pub mod win32;

use crate::arch::common::btl::abi::GuestAbi;
use crate::arch::common::btl::memory::{GuestMemory, NullGuestMemory};
use crate::arch::common::btl::nt61;

/// Result of a bridged guest syscall.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BridgeResult {
    pub result: u64,
    pub status: u32,
}

fn bridge_abi(
    abi: GuestAbi,
    number: u32,
    args: &[u64],
    memory: &mut dyn GuestMemory,
) -> BridgeResult {
    let status = nt61::legacy_status(abi, number, args, memory);
    if status & 0x8000_0000 != 0 { BridgeResult { result: 0, status } }
    else { BridgeResult { result: status as u64, status: 0 } }
}

/// Bridge an x86_64 guest syscall using the NT 6.1 guest service table.
pub fn bridge_x86_64(number: u32, args: &[u64]) -> BridgeResult {
    let mut memory = NullGuestMemory;
    bridge_abi(GuestAbi::X86_64, number, args, &mut memory)
}

/// Bridge an x86_64 guest syscall with guest-memory access.
pub fn bridge_x86_64_memory(
    number: u32,
    args: &[u64],
    memory: &mut dyn GuestMemory,
) -> BridgeResult {
    bridge_abi(GuestAbi::X86_64, number, args, memory)
}

/// Bridge an ARM32 guest syscall using the NT 6.1 guest service table.
pub fn bridge_arm32(number: u32, args: &[u64]) -> BridgeResult {
    let mut memory = NullGuestMemory;
    bridge_abi(GuestAbi::Arm32, number, args, &mut memory)
}

/// Bridge an x86_32 guest syscall; arguments are read from guest EDX.
pub fn bridge_x86_32_memory(
    number: u32,
    args: &[u64],
    memory: &mut dyn GuestMemory,
) -> BridgeResult {
    bridge_abi(GuestAbi::X86_32, number, args, memory)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn guest_indices_are_not_native_indices() {
        assert_eq!(bridge_x86_64(0x3f3, &[0; 17]).status, nt61::STATUS_INVALID_SYSTEM_SERVICE);
        assert_eq!(bridge_x86_64(0x43, &[]).result, 0x4000_0024);
        // x86 CreateFile must read its 11 DWORD arguments through GuestMemory.
        let mut memory = NullGuestMemory;
        assert_eq!(bridge_x86_32_memory(0x42, &[0; 17], &mut memory).status, nt61::STATUS_ACCESS_VIOLATION);
        assert_eq!(bridge_arm32(0x43, &[0; 17]).status, nt61::STATUS_INVALID_SYSTEM_SERVICE);
    }
}

pub fn init() {}
