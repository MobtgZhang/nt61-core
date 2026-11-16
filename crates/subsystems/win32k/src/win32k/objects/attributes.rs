//! Bitmap/brush stock conversion and brush bitmap queries. The handle's stock
//! bit changes with the object; stale aliases are rejected by the handle table.
use super::*;
use crate::arch::common::syscall_frame::SyscallFrame;
use crate::mm::user_copy::write_user_object;

pub(super) const CONVERTED_STOCK: u8 = 2;
pub(super) const STOCK_HANDLE_BIT: u64 = 0x0080_0000;

fn convert(frame: &SyscallFrame, kind: GdiObjectType, stock: bool) -> u64 {
    if frame.args[1] as u32 & 1 == 0 {
        return 0;
    }
    with_handle_table(|table| {
        let Some(entry) = table.get_mut(frame.args[0]) else {
            return 0;
        };
        if entry.get_type() != Some(kind)
            || entry.uses != 0
            || entry.process_id != super::super::window::input_context::caller_identity().0 as u32
        {
            return 0;
        }
        if stock {
            // Built-in stock cache objects cannot be reassigned to a caller.
            if entry.flags & HANDLE_FLAG_STOCK != 0 {
                return 0;
            }
            entry.flags |= HANDLE_FLAG_STOCK | CONVERTED_STOCK;
            frame.args[0] | STOCK_HANDLE_BIT
        } else {
            if entry.flags & CONVERTED_STOCK == 0 {
                return 0;
            }
            entry.flags &= !(HANDLE_FLAG_STOCK | CONVERTED_STOCK);
            frame.args[0] & !STOCK_HANDLE_BIT
        }
    })
}

pub extern "C" fn nt_gdi_set_bitmap_attributes(frame: &SyscallFrame) -> u64 {
    convert(frame, GdiObjectType::Bitmap, true)
}
pub extern "C" fn nt_gdi_clear_bitmap_attributes(frame: &SyscallFrame) -> u64 {
    convert(frame, GdiObjectType::Bitmap, false)
}
pub extern "C" fn nt_gdi_set_brush_attributes(frame: &SyscallFrame) -> u64 {
    convert(frame, GdiObjectType::Brush, true)
}
pub extern "C" fn nt_gdi_clear_brush_attributes(frame: &SyscallFrame) -> u64 {
    convert(frame, GdiObjectType::Brush, false)
}
pub extern "C" fn nt_gdi_get_object_bitmap_handle(frame: &SyscallFrame) -> u64 {
    with_handle_table(|table| {
        let Some(entry) = table.get(frame.args[0]) else {
            return 0;
        };
        if entry.get_type() != Some(GdiObjectType::Brush) || entry.obj_ptr == 0 {
            return 0;
        }
        // SAFETY: the handle table lock prevents concurrent brush deletion.
        let brush = unsafe { &*(entry.obj_ptr as *const GdiBrush) };
        let usage = if brush.style == 5 { 1u32 } else { 0u32 };
        if write_user_object(frame.args[1], &usage).is_err() {
            return 0;
        }
        brush.bitmap
    })
}
