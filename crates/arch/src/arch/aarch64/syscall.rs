//! aarch64 system call (SVC #0) dispatch.
//!
//! ARMv8-A does not have an equivalent of the AMD64 `syscall`
//! instruction; user-mode programs issue `svc #0` which the CPU
//! traps to the configured `VBAR_EL1`+0x400 vector (lower-EL sync).
//! The lower-EL sync stub in `arch::aarch64::exception` saves the
//! user register context into a stack-resident [`TrapFrame`], reads
//! `ESR_EL1` to identify the trap class, and forwards the trap to
//! [`crate::arch::aarch64::trap::arch_aarch64_trap_dispatch`]. The
//! SVC case in that dispatcher reads the syscall number from `X16`
//! (Windows ARM64 calling convention) and calls
//! [`syscall_dispatch_with_tf`] defined here.
//!
//! The dispatch table currently implements a small but useful subset
//! of the NT 6.1 native API. Unsupported shipped services return
//! `STATUS_NOT_IMPLEMENTED`; invalid service numbers are rejected.

use super::trap::TrapFrame;

/// Trap frame for the aarch64 SVC handler (matches the layout saved
/// by the stubs in `exception.rs`).
pub use super::trap::TrapFrame as SvcTrapFrame;

#[cfg(target_arch = "aarch64")]
const STATUS_INVALID_SYSTEM_SERVICE: u64 = 0xC000_001C;
const STATUS_INVALID_PARAMETER: u64 = 0xC000_000D;
const STATUS_NO_DATA: u64 = 0xC000_0011;

use crate::arch::common::cmd_host as host;
use crate::arch::common::desktop_host as desktop;

/// Dispatch a system call whose arguments and result are passed
/// via the `TrapFrame`. Reads the syscall number from `tf.x16`,
/// returns the NTSTATUS (placed back into `tf.x0` by the
/// dispatcher).
///
/// # Safety
///
/// `tf` must point to a valid [`TrapFrame`] captured by the SVC
/// stub.
#[cfg(target_arch = "aarch64")]
pub unsafe extern "C" fn syscall_dispatch_with_tf(syscall_num: u64, tf: *mut TrapFrame) -> u64 {
    // Bump the per-CPU syscall counter for instrumentation.
    let percpu = crate::arch::common::percpu::get_current();
    percpu.syscall_count = percpu.syscall_count.wrapping_add(1);

    if syscall_num >= 0x2000 {
        return STATUS_INVALID_SYSTEM_SERVICE;
    }
    let tf_ref: &mut TrapFrame = &mut *tf;
    let register_args = [
        tf_ref.x0 as usize,
        tf_ref.x1 as usize,
        tf_ref.x2 as usize,
        tf_ref.x3 as usize,
        tf_ref.x4 as usize,
        tf_ref.x5 as usize,
        tf_ref.x6 as usize,
        tf_ref.x7 as usize,
    ];
    if (0x1000..=0x1FFF).contains(&syscall_num) {
        let result = match crate::arch::common::syscall_frame::SyscallFrame::capture_shadow(
            syscall_num,
            &register_args.map(|value| value as u64),
            tf_ref.sp,
        ) {
            Ok(arguments) => {
                crate::ke::shadow_ssdt::dispatch_shadow_service(syscall_num as u32, &arguments)
            }
            Err(status) => status as u32 as u64,
        };
        return result;
    }
    let common_arguments = crate::ke::ssdt_common::SyscallArguments {
        register_args,
        integer_register_count: crate::ke::ssdt_common::INTEGER_REGISTER_ARGUMENTS,
        stack_pointer: tf_ref.sp as usize,
    };
    // SAFETY: the trap stub supplied the captured integer registers and user SP.
    let common_result =
        unsafe { crate::ke::ssdt_common::try_dispatch(syscall_num as usize, &common_arguments) };
    if let Some(status) = common_result {
        return status as u64;
    }

    if syscall_num < 0x0200 {
        return crate::ke::ssdt_common::unregistered_service_status(syscall_num as usize) as u64;
    }
    match syscall_num as u32 {
        host::SYS_PUTCHAR => host::putchar((tf_ref.x0 & 0xFF) as u8),
        host::SYS_POLL_KEY => match host::poll_key() {
            Some(byte) => byte as u64,
            None => 0,
        },
        host::SYS_GET_KEY => {
            let byte = host::get_key();
            byte as u64
        }
        host::SYS_CLEARSCREEN => host::clear_screen(),
        host::SYS_READ_LINE => {
            match unsafe { host::read_line(tf_ref.x0 as *mut u8, tf_ref.x1 as usize) } {
                Some(written) => written as u64,
                None => 0,
            }
        }
        host::SYS_RETURN_TO_FIRMWARE => host::return_to_firmware(),
        host::SYS_EXIT_PROCESS => host::exit_process(tf_ref.x0 as u32),
        host::SYS_CMD_EXECUTE => {
            match host::execute_command(tf_ref.x0 as *const u8, tf_ref.x1 as usize) {
                Ok(exited) => exited as u64,
                Err(_) => STATUS_INVALID_PARAMETER,
            }
        }
        host::SYS_PRINT_PROMPT => host::print_prompt(),
        0x030A => unsafe { crate::arch::common::desktop_service::dispatch(tf_ref.x0) },
        desktop::SYS_DESKTOP_INFO => unsafe { desktop::info(tf_ref.x0 as *mut _) },
        desktop::SYS_DESKTOP_FILL_RECT => desktop::fill_rect(
            tf_ref.x0 as u32,
            tf_ref.x1 as u32,
            tf_ref.x2 as u32,
            tf_ref.x3 as u32,
            tf_ref.x4 as u32,
        ),
        desktop::SYS_DESKTOP_DRAW_TEXT => unsafe {
            desktop::draw_text(
                tf_ref.x0 as *const u8,
                tf_ref.x1 as usize,
                tf_ref.x2 as u32,
                tf_ref.x3 as u32,
                // User thunk packs {fg | bg << 32} into one argument.
                ((tf_ref.x4 as u64) >> 32) as u32,
                (tf_ref.x4 as u64 & 0xFFFF_FFFF) as u32,
            )
        },
        desktop::SYS_DESKTOP_DRAW_RGBA => unsafe {
            desktop::draw_rgba(
                tf_ref.x0 as *const u8,
                // User thunk packs {y | x << 32} and {h | w << 32}.
                ((tf_ref.x1 as u64) >> 32) as u32,
                (tf_ref.x1 as u64 & 0xFFFF_FFFF) as u32,
                ((tf_ref.x2 as u64) >> 32) as u32,
                (tf_ref.x2 as u64 & 0xFFFF_FFFF) as u32,
                tf_ref.x3 as usize,
            )
        },
        desktop::SYS_DESKTOP_POLL_KEY => unsafe { desktop::poll_key(tf_ref.x0 as *mut u8) },
        desktop::SYS_DESKTOP_TIME => desktop::shell_time(),
        desktop::SYS_DESKTOP_TICKS => desktop::shell_ticks(),
        desktop::SYS_DESKTOP_AUDIO => desktop::shell_audio(tf_ref.x0, tf_ref.x1),
        desktop::SYS_DESKTOP_DEVICES => desktop::shell_devices(),
        desktop::SYS_DESKTOP_POLL_MOUSE => unsafe { desktop::poll_mouse(tf_ref.x0 as *mut _) },
        _ => STATUS_INVALID_SYSTEM_SERVICE,
    }
}

/// Compatibility entry point uses the same trap-frame dispatcher.
///
/// # Safety
/// `tf` must reference the saved SVC trap frame.
#[no_mangle]
pub unsafe extern "C" fn syscall_dispatch(syscall_num: u64, tf: *mut TrapFrame) -> u64 {
    #[cfg(target_arch = "aarch64")]
    {
        syscall_dispatch_with_tf(syscall_num, tf)
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        let _ = tf;
        crate::ke::ssdt_common::unregistered_service_status(syscall_num as usize) as u64
    }
}
