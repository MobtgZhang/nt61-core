//! Count live objects in this backend. Stock GDI objects are session-shared.
use crate::arch::common::syscall_frame::SyscallFrame;

pub extern "C" fn nt_user_get_gui_resources(frame: &SyscallFrame) -> u64 {
    let Some(process) = crate::ps::process::gui_resource_process(frame.args[0]) else {
        return 0;
    };
    let count = match frame.args[1] as u32 {
        0 => {
            super::objects::with_handle_table(|table| table.count_for_process(process as u32))
                + super::dc::get_dc_table().count_for_process(process)
        }
        1 => {
            super::window::WINDOW_MANAGER
                .lock()
                .count_for_process(process)
                + super::user_state::with_user_state(|state| state.count_for_process(process))
        }
        // Peak counts require allocation-time accounting; never synthesize a
        // historical maximum from a current snapshot.
        _ => return 0,
    };
    u64::from(u32::try_from(count).unwrap_or(u32::MAX))
}
