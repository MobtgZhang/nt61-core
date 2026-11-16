use super::*;
use dc::services::*;

#[test]
fn all_six_hatch_styles_draw_repeated_device_space_patterns() {
    // Rows are the conventional 8x8 monochrome hatch definitions. Bit 7 is
    // the leftmost pixel; using literal masks detects transposed diagonals.
    let masks = [
        [0, 0, 0, 255, 0, 0, 0, 0],
        [8, 8, 8, 8, 8, 8, 8, 8],
        [128, 64, 32, 16, 8, 4, 2, 1],
        [1, 2, 4, 8, 16, 32, 64, 128],
        [8, 8, 8, 255, 8, 8, 8, 8],
        [129, 66, 36, 24, 24, 36, 66, 129],
    ];
    for (style, rows) in masks.iter().enumerate() {
        let fixture = DrawingFixture::new();
        let brush = call(objects::nt_gdi_create_hatch_brush_internal, &[style as u64, 0x000000ff, 0]);
        assert_ne!(brush, 0);
        assert_ne!(call(selection::nt_gdi_select_brush, &[fixture.handle, brush]), 0);
        dc::gre_set_bk_color(fixture.handle, 0x00ff0000);
        {
            let mut dc = dc::get_dc(fixture.handle).unwrap();
            assert!(gdi_ops::gre_pat_blt(&mut dc, 0, 0, 16, 16, 0x00f00021));
        }
        for y in 0..16 {
            for x in 0..16 {
                let ink = rows[y as usize % 8] & (0x80 >> (x % 8)) != 0;
                assert_eq!(fixture.pixel(x, y) & 0xffffff, if ink { 0xff0000 } else { 0x0000ff },
                    "style={style}, x={x}, y={y}");
            }
        }
        call(selection::nt_gdi_select_brush, &[fixture.handle, objects::gdi_get_stock_object(0)]);
        assert!(objects::gdi_delete_object(brush));
    }
    assert_eq!(call(objects::nt_gdi_create_hatch_brush_internal, &[6, 0, 0]), 0);
}

#[test]
fn transparent_hatches_preserve_destination_and_follow_saved_brush_origin() {
    let fixture = DrawingFixture::new();
    let brush = call(objects::nt_gdi_create_hatch_brush_internal, &[4, 0xff, 0]);
    call(selection::nt_gdi_select_brush, &[fixture.handle, brush]);
    dc::gre_set_bk_mode(fixture.handle, dc::TRANSPARENT);
    assert_eq!(call(nt_gdi_set_brush_org, &[fixture.handle, (-2i64) as u64, 1, 0]), 1);
    assert_eq!(dc::gre_save_dc(fixture.handle), 1);
    call(nt_gdi_set_brush_org, &[fixture.handle, 0, 0, 0]);
    assert!(dc::gre_restore_dc(fixture.handle, -1));
    let geometry = create([0, 0, 16, 16]);
    assert_eq!(call(raster::nt_gdi_fill_rgn, &[fixture.handle, geometry, brush]), 1);
    for y in 0..16 {
        for x in 0..16 {
            let ink = (x + 2) % 8 == 4 || (y + 7) % 8 == 3;
            assert_eq!(fixture.pixel(x, y) & 0xffffff, if ink { 0xff0000 } else { 0x123456 });
        }
    }
    assert!(objects::gdi_delete_object(geometry));
    call(selection::nt_gdi_select_brush, &[fixture.handle, objects::gdi_get_stock_object(0)]);
    assert!(objects::gdi_delete_object(brush));
}

#[test]
fn color_adjustment_validates_complete_input_and_restores_dc_state() {
    let handle = dc::gre_create_compatible_dc(0);
    let mut value = ColorAdjustment::default();
    let address = &mut value as *mut _ as u64;
    mm::user_copy::allow(address, 24, true);
    value.contrast = 25;
    value.red_gamma = 65000;
    assert_eq!(call(nt_gdi_set_color_adjustment, &[handle, address]), 1);
    assert_eq!(dc::gre_save_dc(handle), 1);
    value.contrast = -100;
    assert_eq!(call(nt_gdi_set_color_adjustment, &[handle, address]), 1);
    assert!(dc::gre_restore_dc(handle, -1));
    assert_eq!(call(nt_gdi_get_color_adjustment, &[handle, address]), 1);
    std::hint::black_box(&mut value);
    assert_eq!(value.contrast, 25);
    assert_eq!(value.red_gamma, 65000);
    let saved = value;
    for invalid in [
        ColorAdjustment { size: 23, ..saved },
        ColorAdjustment { flags: 4, ..saved },
        ColorAdjustment { illuminant: 9, ..saved },
        ColorAdjustment { red_gamma: 2499, ..saved },
        ColorAdjustment { reference_white: 5999, ..saved },
        ColorAdjustment { brightness: 101, ..saved },
    ] {
        value = invalid;
        std::hint::black_box(&mut value);
        assert_eq!(call(nt_gdi_set_color_adjustment, &[handle, address]), 0);
        assert_eq!(call(nt_gdi_get_color_adjustment, &[handle, address]), 1);
        std::hint::black_box(&mut value);
        assert_eq!(value, saved);
    }
    assert_eq!(call(nt_gdi_get_color_adjustment, &[handle, 0x1000]), 0);
    assert_eq!(call(nt_gdi_set_color_adjustment, &[handle, 0x1000]), 0);
    assert!(dc::gre_delete_dc(handle));
}
