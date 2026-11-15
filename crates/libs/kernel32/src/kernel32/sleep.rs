//! Shared Win32 delay conversion. Native APC delivery remains a prerequisite
//! for alertable sleeps to execute queued user callbacks.

pub(super) fn sleep_ex(milliseconds: u32, alertable: bool) -> u32 {
    // INFINITE must not become a finite 49.7-day delay. NT's largest relative
    // interval is effectively infinite and does not depend on wall time.
    let mut interval = if milliseconds == u32::MAX {
        i64::MIN
    } else {
        -i64::from(milliseconds) * 10_000
    };
    let start = crate::ke::time::get_time_snapshot().interrupt_time;
    loop {
        let status = crate::libs::ntdll::sync::nt_delay_execution_kernel(
            u8::from(alertable), interval,
        );
        // Native thread alerts are consumed internally by Win32 SleepEx.
        if status == 0x101 { // STATUS_ALERTED
            if milliseconds != u32::MAX {
                let elapsed = crate::ke::time::get_time_snapshot()
                    .interrupt_time.saturating_sub(start);
                let remaining = (u64::from(milliseconds) * 10_000).saturating_sub(elapsed);
                if remaining == 0 {
                    return 0;
                }
                interval = -(remaining as i64);
            }
            continue;
        }
        return if status == 0xc0 { 0xc0 } else { 0 }; // WAIT_IO_COMPLETION
    }
}
