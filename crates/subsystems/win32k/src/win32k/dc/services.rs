//! Native queries backed by the actual DC state.
//!
//! GetDCDword returns BOOL and writes a DWORD; its return value is not the
//! attribute itself. Selector numbers come from the Win7 SP1 implementation.

use super::{get_dc, DC_TYPE_MEMORY};
use crate::arch::common::syscall_frame::SyscallFrame;
use crate::mm::user_copy;

/// Brush origins use device coordinates and belong to the saved drawing state.
pub extern "C" fn nt_gdi_set_brush_org(frame: &SyscallFrame) -> u64 {
    let Some(mut dc) = get_dc(frame.args[0]) else {
        return 0;
    };
    let origin = super::PointL::new(frame.args[1] as i32, frame.args[2] as i32);
    let previous = core::mem::replace(&mut dc.attributes.brush_origin, origin);
    if frame.args[3] != 0 && user_copy::write_user_object(frame.args[3], &previous).is_err() {
        return 0;
    }
    1
}

pub extern "C" fn nt_gdi_get_dc_dword(frame: &SyscallFrame) -> u64 {
    let Some(dc) = get_dc(frame.args[0]) else {
        return 0;
    };
    let value = match frame.args[1] as u32 {
        1 => dc.attributes.relative_mode,
        4 => dc.attributes.arc_direction,
        // The active state is depth one; SaveDC adds a saved level.
        5 => dc.saved_states.len() as u32 + 1,
        7 => u32::from(dc.dc_type == DC_TYPE_MEMORY),
        8 => dc.map_mode as u32,
        // Journal, text justification, RTL arc direction and font
        // language information need additional engine state. Do not fabricate
        // successful output for these currently unsupported selectors.
        _ => return 0,
    };
    user_copy::write_user_object(frame.args[2], &value).is_ok() as u64
}

/// DWORD parameters truncate their ABI slot to 32 bits. The output is required
/// for supported selectors, and is written after committing the new state.
pub extern "C" fn nt_gdi_get_and_set_dc_dword(frame: &SyscallFrame) -> u64 {
    let selector = frame.args[1] as u32;
    let value = frame.args[2] as u32;
    let Some(mut dc) = get_dc(frame.args[0]) else {
        if selector == 5 {
            let _ = user_copy::write_user_object(frame.args[3], &0x8000_0000u32);
        }
        return 0;
    };
    let previous = match selector {
        1 => {
            let previous = if dc.eps_printing { 0x400u32 } else { 0 };
            dc.eps_printing = false;
            previous
        }
        2 => core::mem::replace(&mut dc.print_copies, value),
        4 => core::mem::replace(&mut dc.attributes.relative_mode, value),
        6 => 0,
        7 => {
            if value > 1 {
                u32::MAX
            } else {
                core::mem::replace(&mut dc.attributes.mapper_flags, value)
            }
        }
        8 => super::mapping::set_map_mode(&mut dc, value as i32) as u32,
        9 => {
            if value != 1 && value != 2 {
                0
            } else {
                core::mem::replace(&mut dc.attributes.arc_direction, value)
            }
        }
        _ => return 0,
    };
    user_copy::write_user_object(frame.args[3], &previous).is_ok() as u64
}

/// The viewport service probes and writes the optional previous extent before
/// validating scaling factors. A failed scale still reports the old extent.
pub extern "C" fn nt_gdi_scale_viewport_ext_ex(frame: &SyscallFrame) -> u64 {
    let Some(mut dc) = get_dc(frame.args[0]) else {
        return 0;
    };
    if frame.args[5] != 0
        && (frame.args[5] & 3 != 0
            || user_copy::write_user_object(frame.args[5], &dc.viewport_ext).is_err())
    {
        return 0;
    }
    scale_dc_extent(&mut dc, frame, true) as u64
}

/// The window service scales first and copies the old extent only on success.
/// A bad output address therefore does not undo a successful state update.
pub extern "C" fn nt_gdi_scale_window_ext_ex(frame: &SyscallFrame) -> u64 {
    let Some(mut dc) = get_dc(frame.args[0]) else {
        return 0;
    };
    let previous = dc.window_ext;
    if !scale_dc_extent(&mut dc, frame, false) {
        return 0;
    }
    if frame.args[5] != 0 && user_copy::write_user_object(frame.args[5], &previous).is_err() {
        return 0;
    }
    1
}

fn scale_dc_extent(dc: &mut super::DcObject, frame: &SyscallFrame, viewport: bool) -> bool {
    if dc.map_mode <= super::MM_TWIPS {
        return true;
    }
    let previous = if viewport {
        dc.viewport_ext
    } else {
        dc.window_ext
    };
    let factors = core::array::from_fn(|index| frame.args[index + 1] as i32);
    let Some(next) = super::mapping::scale_extent(previous, factors) else {
        return false;
    };
    super::mapping::set_extent(dc, next, viewport)
}
