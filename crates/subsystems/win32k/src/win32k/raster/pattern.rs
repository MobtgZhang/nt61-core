//! Ordered pattern rectangle transfers using the Win7 64-bit PATRECT layout.
//! Capture caller descriptors before taking the DC guard. Brush samples are
//! copied while holding the object guard, so temporary brushes never change
//! the selected object or its reference count.

use super::*;
use crate::mm::user_copy;

const RECORD_SIZE: usize = 24;
const MAX_CAPTURE_BYTES: usize = 16 * 1024 * 1024;

fn record_brush(
    dc: &DcObject,
    handle: u64,
    selected: Option<BrushSample>,
) -> Option<Option<BrushSample>> {
    if handle == 0 {
        return Some(selected);
    }
    objects::with_handle_table(|table| {
        let process = super::super::window::input_context::caller_identity().0 as u32;
        let entry = table.get(handle).filter(|entry| {
            entry.get_type() == Some(GdiObjectType::Brush)
                && entry.obj_ptr != 0
                && (entry.process_id == 0 || entry.process_id == process)
        });
        let Some(entry) = entry else {
            // Win7's internal brush selector leaves the selected brush intact
            // on an invalid handle. PolyPatBlt still draws that rectangle.
            return Some(selected);
        };
        // SAFETY: the object table guard pins the brush until its values have
        // been copied into the independent sampler.
        let brush = unsafe { &*(entry.obj_ptr as *const GdiBrush) };
        sample_brush(dc, brush)
    })
}

/// Arguments: HDC, ROP3, PATRECT pointer, DWORD count, DWORD mode (zero).
pub extern "C" fn nt_gdi_poly_pat_blt(frame: &SyscallFrame) -> u64 {
    let count = frame.args[3] as u32;
    // Win7 accepts an empty batch without probing the other arguments.
    if count == 0 {
        return 1;
    }
    let address = frame.args[2];
    let Some(length) = (count as usize).checked_mul(RECORD_SIZE) else {
        return 0;
    };
    if count > 0x0aaa_aaaa || length > MAX_CAPTURE_BYTES || address == 0 || address & 3 != 0 {
        return 0;
    }
    let mut records = Vec::new();
    if records.try_reserve_exact(length).is_err() {
        return 0;
    }
    records.resize(length, 0);
    if user_copy::copy_from_user(&mut records, address).is_err() {
        return 0;
    }
    let operation = frame.args[1] as u32;
    if frame.args[4] as u32 != 0 || uses_source(operation) {
        return 0;
    }
    let Some(dc) = super::super::dc::get_dc(frame.args[0]) else {
        return 0;
    };
    let Some(selected) = brush_sample(&dc, dc.brush) else {
        return 0;
    };
    let mut result = true;
    for record in records.chunks_exact(RECORD_SIZE) {
        let coordinate = |offset: usize| {
            i32::from_le_bytes([
                record[offset],
                record[offset + 1],
                record[offset + 2],
                record[offset + 3],
            ])
        };
        let brush_handle = u64::from_le_bytes([
            record[16], record[17], record[18], record[19], record[20], record[21], record[22],
            record[23],
        ]);
        let Some(brush) = record_brush(&dc, brush_handle, selected) else {
            // A valid but unsupported pattern brush cannot be rendered by a
            // solid-color substitute.
            return 0;
        };
        let x = coordinate(0);
        let y = coordinate(4);
        let width = coordinate(8);
        let height = coordinate(12);
        if width == 0 || height == 0 {
            continue;
        }
        // Native LONG endpoint arithmetic wraps before rectangle ordering.
        let rectangle = Rect::new(x, y, x.wrapping_add(width), y.wrapping_add(height));
        let Some(rectangle) = clipping::logical_rect(&dc, rectangle) else {
            return 0;
        };
        let Ok(region) = Region::rectangle(rectangle) else {
            return 0;
        };
        // Null brushes suppress pattern-dependent draws, but BLACKNESS,
        // WHITENESS and DSTINVERT still operate on the destination.
        let brush = if uses_pattern(operation) {
            brush
        } else {
            Some(BrushSample::solid(0))
        };
        result = brush.is_none_or(|brush| paint_sampled_region(&dc, &region, brush, operation));
    }
    u64::from(result)
}
