pub const SYS_PUTCHAR: u32 = 0x0202;
#[allow(dead_code)]
pub const SYS_EXIT_PROCESS: u32 = 0x0201;
pub const SYS_READ_LINE: u32 = 0x0211;
pub const SYS_CMD_EXECUTE: u32 = 0x0215;
pub const SYS_PRINT_PROMPT: u32 = 0x0216;

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
#[unsafe(naked)]
unsafe extern "sysv64" fn syscall5_raw(
    number: u32,
    arg0: usize,
    arg1: usize,
    arg2: usize,
    arg3: usize,
    arg4: usize,
) -> u64 {
    core::arch::naked_asm!(
        "sub rsp, 16",
        "mov [rsp + 8], r9", // fifth argument follows the return-address slot
        "mov r9, r8",
        "mov r8, rcx",
        "mov r10, rsi",
        "mov rax, rdi",
        "syscall",
        "add rsp, 16",
        "ret",
    );
}

#[inline(always)]
pub fn syscall5(
    number: u32,
    arg0: usize,
    arg1: usize,
    arg2: usize,
    arg3: usize,
    arg4: usize,
) -> u64 {
    unsafe { syscall5_raw(number, arg0, arg1, arg2, arg3, arg4) }
}

#[inline(always)]
pub fn put_char(byte: u8) {
    syscall5(SYS_PUTCHAR, byte as usize, 0, 0, 0, 0);
}

#[inline(always)]
pub fn read_line(buffer: &mut [u8]) -> usize {
    syscall5(
        SYS_READ_LINE,
        buffer.as_mut_ptr() as usize,
        buffer.len(),
        0,
        0,
        0,
    ) as usize
}

#[inline]
pub fn execute_command(command: &[u8]) -> bool {
    let mut buffer = [0u8; 1024];
    let length = command.len().min(buffer.len() - 1);
    buffer[..length].copy_from_slice(&command[..length]);
    syscall5(
        SYS_CMD_EXECUTE,
        buffer.as_ptr() as usize,
        buffer.len(),
        0,
        0,
        0,
    ) != 0
}

#[inline(always)]
pub fn print_prompt() {
    syscall5(SYS_PRINT_PROMPT, 0, 0, 0, 0, 0);
}

#[allow(dead_code)]
#[inline(always)]
pub fn halt() -> ! {
    exit_process(0)
}

#[inline(always)]
pub fn exit_process(exit_code: u32) -> ! {
    syscall5(SYS_EXIT_PROCESS, exit_code as usize, 0, 0, 0, 0);
    loop {
        core::hint::spin_loop();
    }
}
