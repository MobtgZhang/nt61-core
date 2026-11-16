//! Retained Win7 service bodies whose work was removed before SP1.
//!
//! These return values follow the original instructions, rather than treating
//! an unimplemented GDI/USER operation as successful.
use crate::arch::common::syscall_frame::SyscallFrame;

/// Win7 SP1 x64 RVA 0xb5d00: `mov eax, 1; ret`. Initialization occurs elsewhere.
pub extern "C" fn nt_gdi_init(_: &SyscallFrame) -> u64 {
    1
}

/// Win7 SP1 x64 RVA 0x1b8d8c: EngSetLastError(ERROR_NOT_SUPPORTED), return FALSE.
/// This obsolete entry does not initialize or report a usable print spooler.
pub extern "C" fn nt_gdi_init_spool(_: &SyscallFrame) -> u64 {
    super::input_services::set_last_error(50);
    0
}

/// Win7 SP1 x64 RVA 0xd6654 ignores its argument and returns zero without waiting.
///
/// Original USER entry/exit also locks the session resource, publishes gptiCurrent
/// while locked, and releases pending session-switch semaphore counts on exit.
/// This kernel currently has one USER session, no gptiCurrent global and no
/// session-switch semaphore producer. The body accesses no shared USER state;
/// it therefore needs no local state lock. Adding session switching must add the
/// common USER entry/exit protocol here as well as to other USER service entries.
pub extern "C" fn nt_user_wait_for_msg_and_event(_: &SyscallFrame) -> u64 {
    0
}

/// Win7 SP1 x64 RVA 0x1b9efc: `mov eax, 0xc0000001; ret`.
/// All five arguments are ignored; this retained entry has no fullscreen work.
pub extern "C" fn nt_gdi_fullscreen_control(_: &SyscallFrame) -> u64 {
    0xc0000001u32 as u64
}

/// Win7 SP1 x64 RVA 0x18d3a4: `xor eax, eax; ret`.
/// All four arguments and caller last error remain untouched.
pub extern "C" fn nt_gdi_get_spool_message(_: &SyscallFrame) -> u64 {
    0
}

/// Win7 SP1 aliases the GetSpoolMessage constant-zero body at RVA 0x18d3a4.
/// This entry does not unload a driver; both arguments are ignored.
pub extern "C" fn nt_gdi_unload_printer_driver(_: &SyscallFrame) -> u64 {
    0
}
