//! Legacy scalar syscall entry. Complete guest contexts use `nt61::dispatch`.
//! This entry has no guest RSP and cannot capture x64 stack arguments.
#![cfg(any(feature = "btl", test))]
use crate::arch::common::btl::{abi::GuestAbi, ir::X86Reg, memory::NullGuestMemory, nt61};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BtlSyscallResult { Handled { result: u64 }, NotHandled }
pub type BridgeResult = BtlSyscallResult;
pub fn btl_syscall_dispatch(num: u64, a0: u64, a1: u64, a2: u64, a3: u64, _a4: u64) -> BtlSyscallResult {
    let Ok(number) = u32::try_from(num) else { return BtlSyscallResult::NotHandled; };
    if nt61::resolve(GuestAbi::X86_64, number).is_none() { return BtlSyscallResult::NotHandled; }
    let mut regs = [0; 17];
    for (reg, value) in [(X86Reg::R10, a0), (X86Reg::Rdx, a1), (X86Reg::R8, a2), (X86Reg::R9, a3)] {
        regs[reg as usize] = value;
    }
    // Null memory deliberately rejects every service needing stack arguments.
    let result = nt61::legacy_status(GuestAbi::X86_64, number, &regs, &mut NullGuestMemory);
    BtlSyscallResult::Handled { result: result as u64 }
}
pub fn init() {}
pub fn smoke_test() -> bool {
    btl_syscall_dispatch(0x43, 0, 0, 0, 0, 0) == BtlSyscallResult::Handled { result: 0x4000_0024 }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn incomplete_capture_fails_closed() {
        assert_eq!(btl_syscall_dispatch(5, 0, 0, 0, 0, 0), BtlSyscallResult::Handled { result: nt61::STATUS_ACCESS_VIOLATION as u64 });
        assert_eq!(btl_syscall_dispatch(0x3f5, 0, 0, 0, 0, 0), BtlSyscallResult::NotHandled);
        assert!(smoke_test());
    }
}
