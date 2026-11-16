//! Immutable logical color spaces, selected and pinned by a DC (including
//! SaveDC). The framebuffer has no ICC conversion engine; ICM_ON is rejected.
use super::{
    dc,
    objects::{self, GdiObjectType},
};
use crate::arch::common::syscall_frame::SyscallFrame;
use crate::mm::{
    pool::{self, PoolType},
    user_copy::read_user_object,
};

pub extern "C" fn nt_gdi_create_color_space(frame: &SyscallFrame) -> u64 {
    // Win7 takes LOGCOLORSPACEEXW: LOGCOLORSPACEW (588 bytes) plus flags.
    let Ok(data) = read_user_object::<[u32; 148]>(frame.args[0]) else {
        return 0;
    };
    if data[0] != 0x5053_4f43
        || data[1] != 0x400
        || data[2] != 588
        || !matches!(data[3], 0 | 0x7352_4742 | 0x5769_6e20)
        || !matches!(data[4], 1 | 2 | 4 | 8)
        || data[147] != 0
    {
        return 0;
    }
    let allocation = pool::allocate(PoolType::NonPaged, core::mem::size_of_val(&data));
    if allocation.is_null() {
        return 0;
    }
    // SAFETY: the pool allocation has enough space and is not yet published.
    unsafe {
        core::ptr::copy_nonoverlapping(
            data.as_ptr().cast::<u8>(),
            allocation,
            core::mem::size_of_val(&data),
        );
    }
    let process = super::window::input_context::caller_identity().0 as u32;
    let handle = objects::with_handle_table(|table| {
        table.allocate(GdiObjectType::ColorSpace, allocation as u64, process)
    });
    if handle == 0 {
        // SAFETY: failed publication leaves sole ownership of this allocation.
        unsafe {
            let _ = pool::free(allocation);
        }
    }
    handle
}
pub extern "C" fn nt_gdi_delete_color_space(frame: &SyscallFrame) -> u64 {
    u64::from(objects::delete_object_checked(
        frame.args[0],
        Some(GdiObjectType::ColorSpace),
    ))
}
pub extern "C" fn nt_gdi_set_color_space(frame: &SyscallFrame) -> u64 {
    let Some(mut dc) = dc::get_dc(frame.args[0]) else {
        return 0;
    };
    if dc.process_id != super::window::input_context::caller_identity().0 {
        return 0;
    }
    // The native operation is BOOL; gdi32 obtains the previous handle from its
    // shared DC attributes before making this call.
    u64::from(dc.set_color_space(frame.args[1]))
}
pub extern "C" fn nt_gdi_set_icm_mode(frame: &SyscallFrame) -> u64 {
    let Some(_dc) = dc::get_dc(frame.args[0]) else {
        return 0;
    };
    // Win7 uses (HDC, command, value), not the public two-argument
    // SetICMMode signature. Command 1 with value zero disables ICM. Enabling
    // transforms or setting profiles requires an ICC engine and is unsupported.
    u64::from(frame.args[1] == 1 && frame.args[2] == 0)
}
