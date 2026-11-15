//! Typed native DC object selection. Type prefixes alone never validate a handle.

use super::{dc, objects::GdiObjectType};
use crate::arch::common::syscall_frame::SyscallFrame;

pub extern "C" fn nt_gdi_select_pen(frame: &SyscallFrame) -> u64 {
    dc::select_typed_object(frame.args[0], frame.args[1], Some(GdiObjectType::Pen))
}

pub extern "C" fn nt_gdi_select_brush(frame: &SyscallFrame) -> u64 {
    dc::select_typed_object(frame.args[0], frame.args[1], Some(GdiObjectType::Brush))
}

pub extern "C" fn nt_gdi_select_font(frame: &SyscallFrame) -> u64 {
    dc::select_typed_object(frame.args[0], frame.args[1], Some(GdiObjectType::Font))
}
