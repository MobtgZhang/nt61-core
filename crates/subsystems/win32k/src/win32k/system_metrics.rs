//! Active-display system metrics shared by native USER and syscall dispatch.
use crate::arch::common::syscall_frame::SyscallFrame;

/// NtUserGetSystemMetrics - Get system metrics
pub extern "C" fn nt_user_get_system_metrics(frame: &SyscallFrame) -> u64 {
    let index = frame.args[0] as i32;
    // Primary GDI surfaces may still belong to the previous scanout. The
    // modesetting backend publishes the active framebuffer after success.
    let display = crate::hal::common::framebuffer::info();
    let width = display.width.min(i32::MAX as u32) as i32;
    let height = display.height.min(i32::MAX as u32) as i32;
    // The current shell reserves 40 pixels at the bottom. Configurable taskbar
    // placement, DPI and themes require a shared session settings service.
    nt61_win32_metrics::Display {
        width, height, work_width: width, work_height: height.saturating_sub(40).max(0),
        max_track_padding: nt61_win32_metrics::current_max_track_padding(),
    }.basic_96_metric(index) as u32 as u64
}

