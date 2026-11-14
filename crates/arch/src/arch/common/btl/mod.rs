//! Shared BTL infrastructure for all non-x86 architectures.
//!
//! Provides the x86_64 decoder and IR that each target-specific
//! code generator consumes.

pub mod abi;
pub mod exec;
pub mod ir;
pub mod io;
pub mod memory;
pub mod nt61;
pub mod pe;
pub mod runtime;
pub mod syscall;
pub mod x86_decoder;

pub use ir::{AluOp, BranchCond, IROp, IrInst, MemOperand, X86Reg};
pub use x86_decoder::{decode_block, decode_one, MAX_BLOCK_BYTES, MAX_DECODED_INSTS};

/// Native emitters are experimental; the portable runner is the execution backend.
pub const NATIVE_EXECUTION_READY: bool = false;
