//! Native queries backed by the actual DC state.
//!
//! GetDCDword returns BOOL and writes a DWORD; its return value is not the
//! attribute itself. Selector numbers come from the Win7 SP1 implementation.

use super::{get_dc, DC_TYPE_MEMORY};
use crate::arch::common::syscall_frame::SyscallFrame;
use crate::mm::user_copy;

/// The public COLORADJUSTMENT ABI is twelve two-byte fields on every ISA.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColorAdjustment {
    pub size: u16,
    pub flags: u16,
    pub illuminant: u16,
    pub red_gamma: u16,
    pub green_gamma: u16,
    pub blue_gamma: u16,
    pub reference_black: u16,
    pub reference_white: u16,
    pub contrast: i16,
    pub brightness: i16,
    pub colorfulness: i16,
    pub red_green_tint: i16,
}

impl Default for ColorAdjustment {
    fn default() -> Self {
        Self {
            size: 24,
            flags: 0,
            illuminant: 0,
            red_gamma: 10_000,
            green_gamma: 10_000,
            blue_gamma: 10_000,
            reference_black: 0,
            reference_white: 10_000,
            contrast: 0,
            brightness: 0,
            colorfulness: 0,
            red_green_tint: 0,
        }
    }
}

impl ColorAdjustment {
    fn valid(self) -> bool {
        self.size == 24 && self.flags & !3 == 0 && self.illuminant <= 8
            && [self.red_gamma, self.green_gamma, self.blue_gamma]
                .iter().all(|value| (2500..=65_000).contains(value))
            && self.reference_black <= 4000
            && (6000..=10_000).contains(&self.reference_white)
            && [self.contrast, self.brightness, self.colorfulness, self.red_green_tint]
                .iter().all(|value| (-100..=100).contains(value))
    }
}

pub extern "C" fn nt_gdi_get_color_adjustment(frame: &SyscallFrame) -> u64 {
    let Some(dc) = get_dc(frame.args[0]) else { return 0 };
    user_copy::write_user_object(frame.args[1], &dc.attributes.color_adjustment).is_ok() as u64
}

pub extern "C" fn nt_gdi_set_color_adjustment(frame: &SyscallFrame) -> u64 {
    let Ok(value) = user_copy::read_user_object::<ColorAdjustment>(frame.args[1]) else {
        return 0;
    };
    if !value.valid() { return 0; }
    let Some(mut dc) = get_dc(frame.args[0]) else { return 0 };
    dc.attributes.color_adjustment = value;
    1
}

/// Override the physical metrics used when establishing a mapping mode.
pub extern "C" fn nt_gdi_set_virtual_resolution(frame: &SyscallFrame) -> u64 {
    let values: [u32; 4] = core::array::from_fn(|index| frame.args[index + 1] as u32);
    if values.contains(&0) && values != [0; 4] {
        return 0;
    }
    let Some(mut dc) = get_dc(frame.args[0]) else { return 0 };
    dc.attributes.virtual_resolution = values;
    1
}

/// Win7 reads the device/window boundary, independently of drawing bounds,
/// clipping, mapping extents, and the SetSizeDevice virtual-device metrics.
pub extern "C" fn nt_gdi_get_device_width(frame: &SyscallFrame) -> u64 {
    get_dc(frame.args[0]).map_or(u32::MAX as u64, |dc| {
        dc.bounds.right.wrapping_sub(dc.bounds.left) as u32 as u64
    })
}

/// Both arguments are DWORDs. Win7 rejects zero, but accepts signed encodings
/// and stores the metrics without resizing the actual surface or boundary.
pub extern "C" fn nt_gdi_set_size_device(frame: &SyscallFrame) -> u64 {
    let size = super::SizeL::new(frame.args[1] as i32, frame.args[2] as i32);
    if size.cx == 0 || size.cy == 0 {
        return 0;
    }
    let Some(mut dc) = get_dc(frame.args[0]) else { return 0 };
    dc.attributes.size_device = size;
    1
}

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
    let mut previous = dc.window_ext;
    if dc.attributes.layout & super::mirror::LAYOUT_RTL != 0 {
        previous.cx = previous.cx.wrapping_neg();
    }
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
