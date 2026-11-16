//! Native bitmap metadata services. Logical dimensions do not resize pixels.

use super::dc::SizeL;
use super::objects::{self, GdiBitmap, GdiObjectType};
use crate::arch::common::syscall_frame::SyscallFrame;
use crate::mm::user_copy;

#[path = "bitmap/storage.rs"]
pub mod storage;
#[path = "bitmap/dib.rs"]
pub mod dib;

fn with_bitmap<R>(handle: u64, operation: impl FnOnce(&mut GdiBitmap) -> R) -> Option<R> {
    objects::with_handle_table(|table| {
        let entry = table.get(handle)?;
        let process = super::window::input_context::caller_identity().0 as u32;
        if entry.get_type()? != GdiObjectType::Bitmap || entry.obj_ptr == 0
            || entry.process_id != 0 && entry.process_id != process
        {
            return None;
        }
        // SAFETY: the validated entry owns a GdiBitmap allocation. The table
        // guard serializes metadata access and prevents concurrent deletion.
        let bitmap = unsafe { &mut *(entry.obj_ptr as *mut GdiBitmap) };
        Some(operation(bitmap))
    })
}

/// Query the actual device-dependent bitmap format under the object guard.
/// Testing the handle prefix alone also accepts stale or forged handles.
pub extern "C" fn nt_gdi_mono_bitmap(frame: &SyscallFrame) -> u64 {
    with_bitmap(frame.args[0], |bitmap| {
        u64::from(bitmap.planes == 1 && bitmap.bit_count == 1)
    }).unwrap_or(0)
}

pub extern "C" fn nt_gdi_get_bitmap_dimension(frame: &SyscallFrame) -> u64 {
    let Some(dimensions) = with_bitmap(frame.args[0], |bitmap| bitmap.logical_dimensions) else {
        return 0;
    };
    // Win7 x64 copies a SIZE without an alignment probe. Output is required.
    user_copy::write_user_object(frame.args[1], &dimensions).is_ok() as u64
}

pub extern "C" fn nt_gdi_set_bitmap_dimension(frame: &SyscallFrame) -> u64 {
    let dimensions = SizeL::new(frame.args[1] as i32, frame.args[2] as i32);
    let Some(previous) = with_bitmap(frame.args[0], |bitmap| {
        core::mem::replace(&mut bitmap.logical_dimensions, dimensions)
    }) else {
        return 0;
    };
    // State is committed before copying the optional previous SIZE. A failed
    // user copy returns FALSE without undoing the metadata change.
    if frame.args[3] != 0 && user_copy::write_user_object(frame.args[3], &previous).is_err() {
        return 0;
    }
    1
}
