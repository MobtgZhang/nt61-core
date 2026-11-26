//! Resumable x86/x64 execution shared by all hosts.

use super::exec::{execute_block, ExecOutcome, GuestState, SyscallHook};
use super::ir::{DecodeMode, IROp, IrInst, X86Reg};
use super::memory::GuestMemory;
use super::x86_decoder::decode_one_mode;
use alloc::vec::Vec;

#[path = "runtime/jit.rs"]
pub mod jit;

const CACHE_SLOTS: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopReason {
    DivideError { pc: u64 },
    FloatingPointError { pc: u64, exceptions: u8 },
    QuantumExpired,
    /// RIP points after the instruction. The caller must service EAX using
    /// the guest ABI before resuming; execution never calls a host code pointer.
    SystemCall {
        number: u32,
    },
    InstructionFault {
        pc: u64,
    },
    DataFault {
        pc: u64,
    },
    IllegalInstruction {
        pc: u64,
    },
    /// A guest DLL returned FALSE from DLL_PROCESS_ATTACH.
    InitializationFailed {
        module_base: u64,
    },
    /// A checked runtime gateway was called for an API without an implementation.
    UnsupportedApi { id: u32 },
    MappedWritebackFailed { error: u32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunResult {
    pub reason: StopReason,
    pub retired: u64,
}

struct CachedInstruction {
    instruction: IrInst,
    mode: DecodeMode,
    bytes: [u8; 15],
}

/// Each guest thread owns its CPU state and cache. No process-global decode
/// mode or current PC can be overwritten by another CPU/thread.
pub struct GuestThread {
    pub context: GuestState,
    cache: Vec<Option<CachedInstruction>>,
    pub retired: u64,
}

impl GuestThread {
    pub fn new(mode: DecodeMode, pc: u64, sp: u64) -> Self {
        let mut context = GuestState::new();
        context.mode = mode;
        context.set_reg(X86Reg::Rip, pc);
        context.set_reg(X86Reg::Rsp, sp);
        Self {
            context,
            cache: Vec::new(),
            retired: 0,
        }
    }

    pub fn invalidate_code(&mut self) {
        self.cache.fill_with(|| None);
    }

    fn instruction(&mut self, memory: &mut dyn GuestMemory) -> Result<IrInst, StopReason> {
        let pc = self.context.rip;
        let slot = ((pc ^ (pc >> 12) ^ (pc >> 24)) as usize) & (CACHE_SLOTS - 1);
        // Incremental fetch avoids demanding executable bytes beyond the end
        // of an instruction (e.g. RET at the final byte of an executable page).
        if let Some(entry) = self
            .cache
            .get(slot)
            .and_then(Option::as_ref)
            .filter(|entry| entry.mode == self.context.mode && entry.instruction.guest_va == pc)
        {
            let len = entry.instruction.guest_len as usize;
            let mut bytes = [0; 15];
            if memory.fetch_virt(pc, &mut bytes[..len]) == len && bytes[..len] == entry.bytes[..len]
            {
                return Ok(entry.instruction);
            }
            // Re-fetch/re-decode below. This also rechecks permissions after
            // unmapping/remapping, protection changes and self-modifying code.
        }
        let mut bytes = [0; 15];
        for len in 1..=15 {
            let address = pc
                .checked_add((len - 1) as u64)
                .ok_or(StopReason::InstructionFault { pc })?;
            if self.context.mode == DecodeMode::X86 && address > u32::MAX as u64 {
                return Err(StopReason::InstructionFault { pc });
            }
            if memory.fetch_virt(address, &mut bytes[len - 1..len]) != 1 {
                return Err(StopReason::InstructionFault { pc });
            }
            if let Some((instruction, _)) = decode_one_mode(&bytes[..len], pc, self.context.mode) {
                if self.cache.is_empty() && self.cache.try_reserve_exact(CACHE_SLOTS).is_ok() {
                    self.cache.resize_with(CACHE_SLOTS, || None);
                }
                if let Some(entry) = self.cache.get_mut(slot) {
                    *entry = Some(CachedInstruction {
                        instruction,
                        mode: self.context.mode,
                        bytes,
                    });
                }
                return Ok(instruction);
            }
        }
        Err(StopReason::IllegalInstruction { pc })
    }

    /// Run at most `quantum` guest instructions, yielding at every syscall or
    /// precise fault. Callers can reschedule without holding a global lock.
    pub fn run(&mut self, memory: &mut dyn GuestMemory, quantum: u64) -> RunResult {
        self.run_inner(memory, quantum, None)
    }

    pub fn run_with_jit(&mut self, memory: &mut dyn GuestMemory, quantum: u64, jit: &mut jit::Jit<'_>) -> RunResult {
        self.run_inner(memory, quantum, Some(jit))
    }

    fn run_inner(&mut self, memory: &mut dyn GuestMemory, quantum: u64, mut jit: Option<&mut jit::Jit<'_>>) -> RunResult {
        struct Trap;
        impl SyscallHook for Trap {
            fn syscall(&mut self, _: &mut GuestState, _: &mut dyn GuestMemory) {}
        }
        let mut retired = 0;
        let reason = loop {
            if retired == quantum {
                break StopReason::QuantumExpired;
            }
            let instruction = match self.instruction(memory) {
                Ok(i) => i,
                Err(reason) => break reason,
            };
            if jit.as_deref_mut().is_some_and(|jit| jit.try_execute(instruction, &mut self.context)) {
                retired += 1;
                self.retired = self.retired.saturating_add(1);
                continue;
            }
            match execute_block(
                core::slice::from_ref(&instruction),
                &mut self.context,
                memory,
                &mut Trap,
            ) {
                ExecOutcome::DivideError { guest_va } => {
                    break StopReason::DivideError { pc: guest_va }
                }
                ExecOutcome::FloatingPointError { guest_va, exceptions } => {
                    break StopReason::FloatingPointError { pc: guest_va, exceptions }
                }
                ExecOutcome::MemoryFault { guest_va } => {
                    break StopReason::DataFault { pc: guest_va }
                }
                ExecOutcome::Unsupported { guest_va } => {
                    break StopReason::IllegalInstruction { pc: guest_va }
                }
                ExecOutcome::Retired { .. } => {
                    retired += 1;
                    self.retired = self.retired.saturating_add(1);
                    if instruction.op == IROp::Syscall {
                        break StopReason::SystemCall {
                            number: self.context.reg(X86Reg::Rax) as u32,
                        };
                    }
                }
            }
        };
        RunResult { reason, retired }
    }
}
