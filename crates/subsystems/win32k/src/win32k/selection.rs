//! Typed native DC object selection. Type prefixes alone never validate a handle.

use super::{dc, objects::GdiObjectType};
use crate::arch::common::syscall_frame::SyscallFrame;

/// Native selectors are encoded GDI types, not the public OBJ_* ordinals.
pub extern "C" fn nt_gdi_get_dc_object(frame: &SyscallFrame) -> u64 {
    let Some(dc) = dc::get_dc(frame.args[0]) else {
        return 0;
    };
    match frame.args[1] as u32 {
        0x0030_0000 | 0x0050_0000 => dc.pen,
        0x0010_0000 => dc.brush,
        0x0008_0000 => dc.palette_handle,
        0x000a_0000 => dc.font,
        // An unselected memory DC currently has no default monochrome bitmap.
        0x0005_0000 => dc.bitmap,
        _ => 0,
    }
}

pub extern "C" fn nt_gdi_select_pen(frame: &SyscallFrame) -> u64 {
    dc::select_typed_object(frame.args[0], frame.args[1], Some(GdiObjectType::Pen))
}

pub extern "C" fn nt_gdi_select_brush(frame: &SyscallFrame) -> u64 {
    dc::select_typed_object(frame.args[0], frame.args[1], Some(GdiObjectType::Brush))
}

pub extern "C" fn nt_gdi_select_font(frame: &SyscallFrame) -> u64 {
    dc::select_typed_object(frame.args[0], frame.args[1], Some(GdiObjectType::Font))
}
