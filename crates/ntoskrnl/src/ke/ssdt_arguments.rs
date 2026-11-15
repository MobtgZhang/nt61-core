//! Integer register and user-stack argument capture shared by all ISAs.

/// Number of integer ABI slots used by the supported non-x86 architectures.
pub const INTEGER_REGISTER_ARGUMENTS: usize = 8;

/// Number of integer ABI slots used by RISC-V and LoongArch when the
/// eighth register (a7) holds the syscall number.
pub const SEVEN_REGISTER_ARCHITECTURES: usize = 7;

/// Uniform argument context passed to every SSDT handler.
///
/// `register_args` holds the architecture-independent integer ABI slots.
/// `stack_pointer` is the user-mode stack pointer captured by the trap
/// stub; it identifies the first argument absent from `register_args`.
#[derive(Clone, Copy, Debug)]
pub struct SyscallArguments {
    pub register_args: [usize; INTEGER_REGISTER_ARGUMENTS],
    pub integer_register_count: usize,
    pub stack_pointer: usize,
}

impl Default for SyscallArguments {
    fn default() -> Self {
        Self {
            register_args: [0; INTEGER_REGISTER_ARGUMENTS],
            integer_register_count: INTEGER_REGISTER_ARGUMENTS,
            stack_pointer: 0,
        }
    }
}

/// Read one zero-based ABI argument, probing a trailing user-stack slot.
/// The stack pointer always identifies the first argument absent from registers.
pub(super) fn read_argument(arguments: &SyscallArguments, index: usize) -> Result<usize, u32> {
    if arguments.integer_register_count > INTEGER_REGISTER_ARGUMENTS {
        return Err(0xC000000D); // STATUS_INVALID_PARAMETER
    }
    if index < arguments.integer_register_count {
        return Ok(arguments.register_args[index]);
    }
    let offset = (index - arguments.integer_register_count)
        .checked_mul(core::mem::size_of::<usize>())
        .ok_or(0xC0000005u32)?;
    let address = arguments
        .stack_pointer
        .checked_add(offset)
        .ok_or(0xC0000005u32)?;
    crate::mm::user_copy::read_user_object::<usize>(address as u64).map_err(|status| status as u32)
}

/// Normalize only argument slots declared by the registered service. Short calls
/// such as NtClose must not probe unused memory past the caller's stack pointer.
pub(super) fn capture_register_arguments(
    arguments: &SyscallArguments,
    argument_count: usize,
) -> Result<SyscallArguments, u32> {
    if arguments.integer_register_count > INTEGER_REGISTER_ARGUMENTS || argument_count > 17 {
        return Err(0xC000000D);
    }
    let mut captured = *arguments;
    let copied = argument_count.min(INTEGER_REGISTER_ARGUMENTS);
    for index in arguments.integer_register_count..copied {
        captured.register_args[index] = read_argument(arguments, index)?;
    }
    let consumed = copied.saturating_sub(arguments.integer_register_count);
    captured.stack_pointer = arguments
        .stack_pointer
        .checked_add(consumed * core::mem::size_of::<usize>())
        .ok_or(0xC0000005u32)?;
    captured.integer_register_count = INTEGER_REGISTER_ARGUMENTS;
    Ok(captured)
}
