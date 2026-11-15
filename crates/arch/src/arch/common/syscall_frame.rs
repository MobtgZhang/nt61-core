//! Architecture-neutral system-call argument frame.

/// Maximum number of register and stack arguments retained by dispatch.
pub const SYSCALL_ARGUMENT_COUNT: usize = 17;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct SyscallFrame {
    pub args: [u64; SYSCALL_ARGUMENT_COUNT],
}

impl SyscallFrame {
    pub fn from_args<const N: usize>(arguments: [u64; N]) -> Self {
        let mut frame = Self::default();
        assert!(
            N <= SYSCALL_ARGUMENT_COUNT,
            "system-call argument frame overflow"
        );
        frame.args[..N].copy_from_slice(&arguments);
        frame
    }

    /// Capture a shadow service's declared arguments from the integer ABI.
    /// The service table stores total argument bytes, including register slots.
    pub fn capture_shadow(
        service_number: u64,
        registers: &[u64],
        stack_pointer: u64,
    ) -> Result<Self, i32> {
        if !(0x1000..=0x1FFF).contains(&service_number) {
            return Err(crate::libs::ntdll::status::STATUS_INVALID_SYSTEM_SERVICE);
        }
        let count = crate::ke::ssdt::get_shadow_argument_size(service_number as u32) as usize / 8;
        if count > SYSCALL_ARGUMENT_COUNT {
            return Err(crate::libs::ntdll::status::STATUS_INVALID_PARAMETER);
        }
        let mut frame = Self::default();
        let copied = count.min(registers.len());
        frame.args[..copied].copy_from_slice(&registers[..copied]);
        for index in copied..count {
            let offset = ((index - registers.len()) * core::mem::size_of::<u64>()) as u64;
            let address = stack_pointer
                .checked_add(offset)
                .ok_or(crate::libs::ntdll::status::STATUS_ACCESS_VIOLATION)?;
            frame.args[index] = crate::mm::user_copy::read_user_object::<u64>(address)?;
        }
        Ok(frame)
    }
}
