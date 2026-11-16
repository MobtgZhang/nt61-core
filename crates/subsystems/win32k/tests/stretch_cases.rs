use super::*;
use raster::stretch::{nt_gdi_stretch_blt, BLACKONWHITE, COLORONCOLOR, WHITEONBLACK};

const SRCCOPY: u64 = 0x00cc_0020;

fn stretch(destination: u64, source: u64, target: [i32; 4], input: [i32; 4], rop: u64) -> u64 {
    call(nt_gdi_stretch_blt, &[
        destination, target[0] as u64, target[1] as u64, target[2] as u64, target[3] as u64,
        source, input[0] as u64, input[1] as u64, input[2] as u64, input[3] as u64, rop, 0,
    ])
}

fn set_row(fixture: &DrawingFixture, values: &[u32]) {
    let dc = dc::get_dc(fixture.handle).unwrap();
    for (x, &value) in values.iter().enumerate() {
        assert!(raster::set_pixel(&dc, x as i32, 0, value));
    }
}

fn mode(fixture: &DrawingFixture, mode: i32) {
    dc::get_dc(fixture.handle).unwrap().stretch_mode = mode;
}

#[test]
fn stretch_enlarges_between_distinct_surfaces_and_obeys_destination_clip() {
    let source = DrawingFixture::new();
    let destination = DrawingFixture::new();
    set_row(&source, &[0xff, 0xff00, 0xff0000]);
    mode(&destination, COLORONCOLOR);
    call(clipping::nt_gdi_intersect_clip_rect, &[destination.handle, 1, 0, 5, 2]);
    assert_eq!(stretch(destination.handle, source.handle, [0, 0, 6, 2], [0, 0, 3, 1], SRCCOPY), 1);
    for y in 0..2 {
        for (x, pixel) in [0x123456, 0xff0000, 0xff00, 0xff00, 0xff, 0x123456].into_iter().enumerate() {
            assert_eq!(destination.pixel(x as i32, y), pixel);
        }
    }
    assert_eq!(destination.pixel(1, 2), 0x123456);
}

#[test]
fn stretch_mirrors_each_axis_and_cancels_two_reversals() {
    let source = DrawingFixture::new();
    let destination = DrawingFixture::new();
    set_row(&source, &[0xff, 0xff00, 0xff0000]);
    assert_eq!(stretch(destination.handle, source.handle, [3, 2, -3, -2], [0, 0, 3, 1], SRCCOPY), 1);
    for y in 0..2 {
        assert_eq!(destination.pixel(0, y), 0xff);
        assert_eq!(destination.pixel(2, y), 0xff0000);
    }
    assert_eq!(stretch(destination.handle, source.handle, [3, 0, -3, 1], [3, 0, -3, 1], SRCCOPY), 1);
    assert_eq!(destination.pixel(0, 0), 0xff0000);
    assert_eq!(destination.pixel(2, 0), 0xff);
}

#[test]
fn stretch_captures_source_before_overlapping_scaled_writes() {
    let fixture = DrawingFixture::new();
    set_row(&fixture, &[0xff, 0xff00, 0xff0000]);
    assert_eq!(stretch(fixture.handle, fixture.handle, [1, 0, 6, 1], [0, 0, 3, 1], SRCCOPY), 1);
    for (x, pixel) in [0xff0000, 0xff0000, 0xff00, 0xff00, 0xff, 0xff].into_iter().enumerate() {
        assert_eq!(fixture.pixel(x as i32 + 1, 0), pixel);
    }
}

#[test]
fn stretch_reduction_uses_selected_scan_mode_and_saves_it_in_dc_state() {
    let source = DrawingFixture::new();
    let destination = DrawingFixture::new();
    set_row(&source, &[0xff, 0xff00, 0xff0000, 0xffffff]);
    assert_eq!(dc::get_dc(destination.handle).unwrap().stretch_mode, BLACKONWHITE);
    assert_eq!(stretch(destination.handle, source.handle, [0, 0, 2, 1], [0, 0, 4, 1], SRCCOPY), 1);
    assert_eq!(destination.pixel(0, 0), 0);
    assert_eq!(destination.pixel(1, 0), 0xff);
    assert_eq!(dc::gre_save_dc(destination.handle), 1);
    mode(&destination, WHITEONBLACK);
    assert_eq!(stretch(destination.handle, source.handle, [0, 0, 2, 1], [0, 0, 4, 1], SRCCOPY), 1);
    assert_eq!(destination.pixel(0, 0), 0xffff00);
    assert_eq!(destination.pixel(1, 0), 0xffffff);
    mode(&destination, COLORONCOLOR);
    assert_eq!(stretch(destination.handle, source.handle, [0, 0, 2, 1], [0, 0, 4, 1], SRCCOPY), 1);
    assert_eq!(destination.pixel(0, 0), 0xff0000);
    assert_eq!(destination.pixel(1, 0), 0xff);
    assert!(dc::gre_restore_dc(destination.handle, -1));
    assert_eq!(dc::get_dc(destination.handle).unwrap().stretch_mode, BLACKONWHITE);
}

#[test]
fn stretch_applies_rop_and_selected_brush_without_requiring_unused_source() {
    let source = DrawingFixture::new();
    let destination = DrawingFixture::new();
    set_row(&source, &[0xff]);
    assert_eq!(stretch(destination.handle, source.handle, [0, 0, 1, 1], [0, 0, 1, 1], 0x0066_0046), 1);
    assert_eq!(destination.pixel(0, 0), 0x123456 ^ 0xff0000);
    let brush = objects::gdi_create_solid_brush(0xff00);
    call(selection::nt_gdi_select_brush, &[destination.handle, brush]);
    assert_eq!(stretch(destination.handle, u64::MAX, [2, 2, 3, 3], [i32::MAX; 4], 0x00f0_0021), 1);
    assert_eq!(destination.pixel(2, 2), 0xff00);
    call(selection::nt_gdi_select_brush, &[destination.handle, objects::gdi_get_stock_object(0)]);
    assert!(objects::gdi_delete_object(brush));
}

#[test]
fn stretch_uses_both_dc_mappings_and_does_not_apply_source_clip() {
    let source = DrawingFixture::new();
    let destination = DrawingFixture::new();
    set_row(&source, &[0xff, 0xff00, 0xff0000]);
    {
        let mut dc = dc::get_dc(source.handle).unwrap();
        dc.viewport_org = dc::PointL::new(1, 0);
        dc.application_clip = Some(Region::default());
    }
    dc::get_dc(destination.handle).unwrap().viewport_org = dc::PointL::new(2, 3);
    assert_eq!(stretch(destination.handle, source.handle, [0, 0, 4, 2], [0, 0, 2, 1], SRCCOPY), 1);
    assert_eq!(destination.pixel(2, 3), 0xff00);
    assert_eq!(destination.pixel(5, 4), 0xff);
    assert_eq!(destination.pixel(1, 3), 0x123456);
}

#[test]
fn stretch_rejects_invalid_input_and_excessive_work_before_any_writes() {
    let source = DrawingFixture::new();
    let destination = DrawingFixture::new();
    for (target, input, rop) in [
        ([i32::MAX, 0, 2, 2], [0, 0, 1, 1], SRCCOPY),
        ([0, 0, 2, 2], [i32::MAX, 0, 2, 2], SRCCOPY),
        ([0, 0, 2, 2], [0, 0, 1, 1], SRCCOPY | 0x4000_0000),
        ([0, 0, 1, 1], [0, 0, 100_000, 100_000], SRCCOPY),
    ] {
        assert_eq!(stretch(destination.handle, source.handle, target, input, rop), 0);
        assert_eq!(destination.pixel(0, 0), 0x123456);
    }
    mode(&destination, 4);
    assert_eq!(stretch(destination.handle, source.handle, [0, 0, 2, 2], [0, 0, 1, 1], SRCCOPY), 0);
    mode(&destination, COLORONCOLOR);
    assert_eq!(stretch(destination.handle, 0, [0, 0, 2, 2], [0, 0, 1, 1], SRCCOPY), 0);
    assert_eq!(stretch(0, source.handle, [0, 0, 2, 2], [0, 0, 1, 1], SRCCOPY), 0);
}
