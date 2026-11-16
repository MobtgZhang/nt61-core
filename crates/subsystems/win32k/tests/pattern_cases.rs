use super::*;
use raster::pattern::nt_gdi_poly_pat_blt as transfer;

#[derive(Clone, Copy)]
#[repr(C)]
struct PatternRect {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    brush: u64,
}

fn rectangle(x: i32, y: i32, width: i32, height: i32, brush: u64) -> PatternRect {
    PatternRect {
        x,
        y,
        width,
        height,
        brush,
    }
}

fn draw(dc: u64, operation: u32, rectangles: &[PatternRect]) -> u64 {
    mm::user_copy::allow(
        rectangles.as_ptr() as u64,
        core::mem::size_of_val(rectangles),
        false,
    );
    call(
        transfer,
        &[
            dc,
            operation as u64,
            rectangles.as_ptr() as u64,
            rectangles.len() as u64,
            0,
        ],
    )
}

#[test]
fn poly_pattern_empty_batch_does_not_validate_other_arguments() {
    assert_eq!(core::mem::size_of::<PatternRect>(), 24);
    assert_eq!(core::mem::offset_of!(PatternRect, brush), 16);
    assert_eq!(call(transfer, &[u64::MAX, 0, 1, 0, u64::MAX]), 1);
}

#[test]
fn poly_pattern_draws_in_order_and_preserves_selected_brush() {
    let target = DrawingFixture::new();
    let red = objects::gdi_create_solid_brush(0xff);
    let green = objects::gdi_create_solid_brush(0xff00);
    let blue = objects::gdi_create_solid_brush(0xff0000);
    let previous = call(selection::nt_gdi_select_brush, &[target.handle, blue]);
    assert_ne!(previous, 0);
    assert_eq!(
        draw(
            target.handle,
            gdi_ops::PATCOPY,
            &[
                rectangle(0, 0, 8, 8, red),
                rectangle(2, 2, 4, 4, green),
                rectangle(3, 3, 1, 1, 0),
            ]
        ),
        1
    );
    assert_eq!(target.pixel(0, 0), 0xff0000);
    assert_eq!(target.pixel(2, 2), 0xff00);
    assert_eq!(target.pixel(3, 3), 0xff);
    assert_eq!(target.pixel(8, 8), 0x123456);
    assert_eq!(dc::get_dc(target.handle).unwrap().brush, blue);
    // Per-record brushes must not retain selection references after drawing.
    assert!(objects::gdi_delete_object(red));
    assert!(objects::gdi_delete_object(green));
    assert!(!objects::gdi_delete_object(blue));
    call(selection::nt_gdi_select_brush, &[target.handle, previous]);
    assert!(objects::gdi_delete_object(blue));
}

#[test]
fn poly_pattern_applies_clip_mapping_and_negative_extents() {
    let target = DrawingFixture::new();
    {
        let mut dc = dc::get_dc(target.handle).unwrap();
        dc.map_mode = 8;
        dc.viewport_org = dc::PointL::new(2, 1);
        dc.viewport_ext = dc::SizeL::new(2, 2);
        dc.window_ext = dc::SizeL::new(1, 1);
    }
    assert_ne!(
        call(
            clipping::nt_gdi_intersect_clip_rect,
            &[target.handle, 1, 1, 3, 3]
        ),
        0
    );
    assert_eq!(
        draw(
            target.handle,
            gdi_ops::PATCOPY,
            &[rectangle(4, 4, -4, -4, 0),]
        ),
        1
    );
    for y in 0..16 {
        for x in 0..16 {
            let expected = if (4..8).contains(&x) && (3..7).contains(&y) {
                0xffffff
            } else {
                0x123456
            };
            assert_eq!(target.pixel(x, y), expected, "pixel {x}, {y}");
        }
    }
}

#[test]
fn poly_pattern_overlaps_apply_rop_for_each_record() {
    let target = DrawingFixture::new();
    assert_eq!(
        draw(
            target.handle,
            0x0055_0009,
            &[rectangle(0, 0, 5, 5, 0), rectangle(2, 2, 5, 5, 0),]
        ),
        1
    );
    assert_eq!(target.pixel(0, 0), 0xedcba9);
    assert_eq!(target.pixel(2, 2), 0x123456);
    assert_eq!(target.pixel(6, 6), 0xedcba9);
}

#[test]
fn poly_pattern_invalid_brush_keeps_current_brush_and_null_brush_skips_fill() {
    let target = DrawingFixture::new();
    let null_brush = objects::gdi_get_stock_object(5);
    assert_eq!(
        draw(
            target.handle,
            gdi_ops::PATCOPY,
            &[
                rectangle(0, 0, 2, 2, u64::MAX),
                rectangle(2, 2, 2, 2, null_brush),
            ]
        ),
        1
    );
    assert_eq!(target.pixel(0, 0), 0xffffff);
    assert_eq!(target.pixel(2, 2), 0x123456);
    assert_eq!(
        draw(
            target.handle,
            0x0000_0042,
            &[rectangle(2, 2, 2, 2, null_brush),]
        ),
        1
    );
    assert_eq!(target.pixel(2, 2), 0);
}

#[test]
fn poly_pattern_hatch_uses_device_origin_and_background_mode() {
    let target = DrawingFixture::new();
    let hatch = call(objects::nt_gdi_create_hatch_brush_internal, &[0, 0xff, 0]);
    assert_ne!(hatch, 0);
    {
        let mut dc = dc::get_dc(target.handle).unwrap();
        dc.bk_mode = dc::TRANSPARENT;
        dc.attributes.brush_origin = dc::PointL::new(0, 1);
    }
    assert_eq!(
        draw(
            target.handle,
            gdi_ops::PATCOPY,
            &[rectangle(0, 0, 8, 8, hatch),]
        ),
        1
    );
    for y in 0..8 {
        assert_eq!(target.pixel(0, y), if y == 4 { 0xff0000 } else { 0x123456 });
    }
    assert!(objects::gdi_delete_object(hatch));
}

#[test]
fn poly_pattern_captures_full_batch_and_rejects_invalid_arguments_without_drawing() {
    let target = DrawingFixture::new();
    let rectangles = [rectangle(0, 0, 4, 4, 0)];
    let address = rectangles.as_ptr() as u64;
    mm::user_copy::allow(address, 24, false);
    let args = [target.handle, gdi_ops::PATCOPY as u64, address, 1, 0];
    for (index, value) in [
        (0, u64::MAX),
        (1, 0x00cc0020),
        (2, 0),
        (2, 1),
        (2, address + 1),
        (3, 2),
        (3, u32::MAX as u64),
        (4, 1),
    ] {
        let mut invalid = args;
        invalid[index] = value;
        assert_eq!(call(transfer, &invalid), 0, "argument {index}");
        assert_eq!(target.pixel(0, 0), 0x123456);
    }
}

#[test]
fn poly_pattern_accepts_four_byte_aligned_native_descriptors() {
    let target = DrawingFixture::new();
    let mut storage = [0u64; 4];
    let address = storage.as_mut_ptr() as u64 + 4;
    let bytes = unsafe {
        // SAFETY: the 24-byte descriptor fits within the 32-byte allocation.
        std::slice::from_raw_parts_mut(address as *mut u8, 24)
    };
    bytes[8..12].copy_from_slice(&1i32.to_le_bytes());
    bytes[12..16].copy_from_slice(&1i32.to_le_bytes());
    mm::user_copy::allow(address, bytes.len(), false);
    assert_eq!(
        call(
            transfer,
            &[target.handle, gdi_ops::PATCOPY as u64, address, 1, 0]
        ),
        1
    );
    assert_eq!(target.pixel(0, 0), 0xffffff);
}
