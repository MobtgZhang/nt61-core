use super::*;
use font_services::{nt_gdi_ext_text_out, nt_gdi_set_text_justification};

fn text(handle: u64, flags: u64, rectangle: u64, units: &[u16], dx: u64) -> u64 {
    mm::user_copy::allow(units.as_ptr() as u64, units.len() * 2, false);
    call(nt_gdi_ext_text_out, &[handle, 1, 1, flags, rectangle,
        units.as_ptr() as u64, units.len() as u64, dx, 0])
}

#[test]
fn text_output_draws_real_glyph_coverage_and_obeys_clip() {
    let fixture = DrawingFixture::new();
    {
        let mut dc = dc::get_dc(fixture.handle).unwrap();
        dc.bk_mode = dc::TRANSPARENT;
        dc.text_color = 0x000000ff; // COLORREF red
    }
    let rectangle = Rect::new(3, 2, 9, 12);
    mm::user_copy::allow(&rectangle as *const _ as u64, 16, false);
    assert_eq!(text(fixture.handle, 4, &rectangle as *const _ as u64,
        &[b'W' as u16], 0), 1);
    let mut painted = 0;
    for y in 0..16 {
        for x in 0..16 {
            let pixel = fixture.pixel(x, y);
            if pixel != 0x00123456 {
                assert!(x >= 3 && x < 9 && y >= 2 && y < 12);
                assert!(pixel >> 16 > 0x12);
                painted += 1;
            }
        }
    }
    assert!(painted > 5, "text must paint glyph coverage, not merely return success");
}

#[test]
fn empty_opaque_run_paints_only_the_requested_rectangle() {
    let fixture = DrawingFixture::new();
    dc::get_dc(fixture.handle).unwrap().bg_color = 0x0000ff00;
    let rectangle = Rect::new(2, 3, 8, 9);
    mm::user_copy::allow(&rectangle as *const _ as u64, 16, false);
    assert_eq!(text(fixture.handle, 2, &rectangle as *const _ as u64, &[], 0), 1);
    for y in 0..16 {
        for x in 0..16 {
            assert_eq!(fixture.pixel(x, y), if x >= 2 && x < 8 && y >= 3 && y < 9 {
                0x0000ff00
            } else { 0x00123456 });
        }
    }
}

#[test]
fn explicit_two_axis_spacing_updates_cp_and_failed_capture_keeps_state() {
    let fixture = DrawingFixture::new();
    {
        let mut dc = dc::get_dc(fixture.handle).unwrap();
        dc.text_align = dc::TA_UPDATECP | dc::TA_BASELINE;
        dc.cur_x = 2;
        dc.cur_y = 12;
        dc.bk_mode = dc::TRANSPARENT;
    }
    let spacing = [5i32, 2, 7, -3];
    mm::user_copy::allow(spacing.as_ptr() as u64, 16, false);
    let units = [b'A' as u16, b'B' as u16];
    assert_eq!(text(fixture.handle, 0x2000, 0, &units, spacing.as_ptr() as u64), 1);
    {
        let dc = dc::get_dc(fixture.handle).unwrap();
        assert_eq!((dc.cur_x, dc.cur_y), (14, 11));
    }
    let before: Vec<_> = (0..16).flat_map(|y| (0..16).map(move |x| (x, y)))
        .map(|(x, y)| fixture.pixel(x, y)).collect();
    assert_eq!(text(fixture.handle, 0x2000, 0, &units, 1), 0);
    assert_eq!(call(nt_gdi_ext_text_out, &[fixture.handle, 0, 0, 0, 0, 1, 2, 0, 0]), 0);
    let dc = dc::get_dc(fixture.handle).unwrap();
    assert_eq!((dc.cur_x, dc.cur_y), (14, 11));
    drop(dc);
    let after: Vec<_> = (0..16).flat_map(|y| (0..16).map(move |x| (x, y)))
        .map(|(x, y)| fixture.pixel(x, y)).collect();
    assert_eq!(before, after);
}

#[test]
fn justification_distributes_extra_pixels_and_saved_dc_restores_it() {
    let fixture = DrawingFixture::new();
    {
        let mut dc = dc::get_dc(fixture.handle).unwrap();
        dc.text_align = dc::TA_UPDATECP;
        dc.bk_mode = dc::TRANSPARENT;
    }
    let units = [32, 32, 32];
    assert_eq!(text(fixture.handle, 0, 0, &units, 0), 1);
    let normal = dc::get_dc(fixture.handle).unwrap().cur_x;
    dc::get_dc(fixture.handle).unwrap().cur_x = 0;
    let saved = dc::gre_save_dc(fixture.handle);
    assert!(saved > 0);
    assert_eq!(call(nt_gdi_set_text_justification, &[fixture.handle, 7, 3]), 1);
    assert_eq!(text(fixture.handle, 0, 0, &units, 0), 1);
    assert_eq!(dc::get_dc(fixture.handle).unwrap().cur_x, normal + 7);
    assert_eq!(call(nt_gdi_set_text_justification, &[fixture.handle, 7, u32::MAX as u64]), 0);
    assert!(dc::gre_restore_dc(fixture.handle, saved));
    let dc = dc::get_dc(fixture.handle).unwrap();
    assert_eq!(dc.attributes.text_justification_extra, 0);
    assert_eq!(dc.attributes.text_justification_breaks, 0);
}

#[test]
fn polytext_captures_every_descriptor_before_painting_and_preserves_order() {
    let fixture = DrawingFixture::new();
    dc::get_dc(fixture.handle).unwrap().bg_color = 0x000000ff;
    let mut descriptors = [0u8; 112];
    for (index, rect) in [[1i32, 2, 5, 7], [8, 9, 13, 14]].iter().enumerate() {
        let base = index * 56;
        descriptors[base + 24..base + 28].copy_from_slice(&2u32.to_le_bytes());
        for (slot, value) in descriptors[base + 28..base + 44].chunks_exact_mut(4).zip(rect) {
            slot.copy_from_slice(&value.to_le_bytes());
        }
    }
    mm::user_copy::allow(descriptors.as_ptr() as u64, descriptors.len(), false);
    // Second string is nonempty but its pointer is invalid. The first opaque
    // rectangle must remain unpainted even though its descriptor is valid.
    descriptors[64..68].copy_from_slice(&1u32.to_le_bytes());
    descriptors[72..80].copy_from_slice(&1u64.to_le_bytes());
    assert_eq!(call(font_services::nt_gdi_poly_text_out,
        &[fixture.handle, descriptors.as_ptr() as u64, 2, 0]), 0);
    assert_eq!(fixture.pixel(2, 3), 0x00123456);
    descriptors[64..68].fill(0);
    assert_eq!(call(font_services::nt_gdi_poly_text_out,
        &[fixture.handle, descriptors.as_ptr() as u64, 2, 0]), 1);
    assert_eq!(fixture.pixel(2, 3), 0x00ff0000);
    assert_eq!(fixture.pixel(9, 10), 0x00ff0000);
    assert_eq!(fixture.pixel(6, 6), 0x00123456);
}

#[test]
fn right_and_center_alignment_update_current_position_by_native_rules() {
    let fixture = DrawingFixture::new();
    let distance = [6i32];
    mm::user_copy::allow(distance.as_ptr() as u64, 4, false);
    for (alignment, expected) in [(dc::TA_RIGHT, 6), (dc::TA_CENTER, 12)] {
        {
            let mut dc = dc::get_dc(fixture.handle).unwrap();
            dc.text_align = alignment | dc::TA_UPDATECP | dc::TA_BASELINE;
            dc.cur_x = 12;
            dc.cur_y = 14;
            dc.bk_mode = dc::TRANSPARENT;
        }
        assert_eq!(text(fixture.handle, 0, 0, &[b'A' as u16], distance.as_ptr() as u64), 1);
        assert_eq!(dc::get_dc(fixture.handle).unwrap().cur_x, expected);
    }
}
