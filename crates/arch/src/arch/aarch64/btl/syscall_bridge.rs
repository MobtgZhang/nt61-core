//! NT 6.1 guest service bridge. Guest indices are resolved independently
//! of the host SSDT. The portable runner supplies typed process/VFS services.

/// Syscall bridge result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BridgeResult {
    /// Raw NTSTATUS returned by the native SSDT handler.
    pub result: u64,
    /// NTSTATUS for a failure before dispatch; zero after dispatch.
    pub status: u32,
}

use crate::arch::common::btl::abi::GuestAbi;
use crate::arch::common::btl::memory::{GuestMemory, NullGuestMemory};
use crate::arch::common::btl::nt61;

fn bridge_abi(
    abi: GuestAbi,
    syscall_num: u32,
    args: &[u64],
    memory: &mut dyn GuestMemory,
) -> BridgeResult {
    let status = nt61::legacy_status(abi, syscall_num, args, memory);
    if status & 0x8000_0000 != 0 { BridgeResult { result: 0, status } }
    else { BridgeResult { result: status as u64, status: 0 } }
}

/// Translate an x86_64 syscall into the NT 6.1 guest service path.
///
/// `args` is the guest register file indexed by `X86Reg`. Numbers not
/// present in the guest service table fail closed with `STATUS_INVALID_SYSTEM_SERVICE`.
pub fn bridge_x86_64(syscall_num: u32, args: &[u64]) -> BridgeResult {
    let mut memory = NullGuestMemory;
    bridge_abi(GuestAbi::X86_64, syscall_num, args, &mut memory)
}

/// Translate an x86_64 syscall with access to the guest address space.
pub fn bridge_x86_64_memory(
    syscall_num: u32,
    args: &[u64],
    memory: &mut dyn GuestMemory,
) -> BridgeResult {
    bridge_abi(GuestAbi::X86_64, syscall_num, args, memory)
}

/// Translate an ARM32 syscall. `args[0..4]` are guest R0-R3.
pub fn bridge_arm32(syscall_num: u32, args: &[u64]) -> BridgeResult {
    let mut memory = NullGuestMemory;
    bridge_abi(GuestAbi::Arm32, syscall_num, args, &mut memory)
}

/// Translate an ARM32 syscall with access to the guest address space.
pub fn bridge_arm32_memory(
    syscall_num: u32,
    args: &[u64],
    memory: &mut dyn GuestMemory,
) -> BridgeResult {
    bridge_abi(GuestAbi::Arm32, syscall_num, args, memory)
}

/// Translate an x86_32 syscall. Arguments are read from the guest
/// argument block addressed by guest EDX.
pub fn bridge_x86_32_memory(
    syscall_num: u32,
    args: &[u64],
    memory: &mut dyn GuestMemory,
) -> BridgeResult {
    bridge_abi(GuestAbi::X86_32, syscall_num, args, memory)
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
