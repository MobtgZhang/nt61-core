use super::*;
use path::shapes::*;

#[test]
fn ellipse_and_roundrect_draw_fill_outline_and_preserve_current_position() {
    for shape in [nt_gdi_ellipse, nt_gdi_round_rect] {
        let fixture = DrawingFixture::new();
        let brush = objects::gdi_create_solid_brush(0x000000ff);
        {
            let mut dc = dc::get_dc(fixture.handle).unwrap();
            dc.brush = brush;
            dc.cur_x = 7;
            dc.cur_y = 8;
        }
        assert_eq!(call(shape, &[fixture.handle, 2, 2, 14, 14, 6, 6]), 1);
        assert_eq!(fixture.pixel(8, 8) & 0xffffff, 0xff0000);
        assert_eq!(fixture.pixel(2, 2) & 0xffffff, 0x123456);
        assert_eq!(fixture.pixel(0, 0) & 0xffffff, 0x123456);
        let dc = dc::get_dc(fixture.handle).unwrap();
        assert_eq!((dc.cur_x, dc.cur_y), (7, 8));
        drop(dc);
        assert!(objects::gdi_delete_object(brush));
    }
}

#[test]
fn curved_shapes_record_beziers_and_defer_pixels_until_the_path_is_painted() {
    let fixture = DrawingFixture::new();
    assert_eq!(call(path::services::nt_gdi_begin_path, &[fixture.handle]), 1);
    assert_eq!(call(nt_gdi_ellipse, &[fixture.handle, 2, 2, 14, 14]), 1);
    assert_eq!(fixture.pixel(8, 8), 0x123456);
    {
        let dc = dc::get_dc(fixture.handle).unwrap();
        assert_eq!(dc.path.entries.len(), 13);
        assert_eq!(dc.path.entries[0].kind, path::PT_MOVETO);
        assert_eq!(dc.path.entries[12].kind, path::PT_BEZIERTO | path::PT_CLOSEFIGURE);
    }
    assert_eq!(call(path::services::nt_gdi_end_path, &[fixture.handle]), 1);
    assert_eq!(call(path::services::nt_gdi_stroke_and_fill_path, &[fixture.handle]), 1);
    assert_ne!(fixture.pixel(8, 8), 0x123456);
}

#[test]
fn angle_arc_continues_the_current_figure_and_updates_logical_position() {
    let fixture = DrawingFixture::new();
    assert!(path::move_to(&mut dc::get_dc(fixture.handle).unwrap(), dc::PointL::new(1, 8)));
    assert_eq!(call(path::services::nt_gdi_begin_path, &[fixture.handle]), 1);
    for start in [0.0f32, 90.0] {
        assert_eq!(call(nt_gdi_angle_arc,
            &[fixture.handle, 8, 8, 4, start.to_bits() as u64, 90f32.to_bits() as u64]), 1);
    }
    let dc = dc::get_dc(fixture.handle).unwrap();
    assert_eq!((dc.cur_x, dc.cur_y), (4, 8));
    assert_eq!(dc.path.entries.iter().filter(|e| e.kind == path::PT_MOVETO).count(), 1);
    assert_eq!(dc.path.entries.iter().filter(|e| e.kind == path::PT_BEZIERTO).count(), 6);
}

#[test]
fn invalid_arc_input_preserves_the_path_current_position_and_surface() {
    let fixture = DrawingFixture::new();
    assert_eq!(call(path::services::nt_gdi_begin_path, &[fixture.handle]), 1);
    for (radius, angle, sweep) in [(u32::MAX, 0.0, 90.0), (2, f32::NAN, 90.0),
        (2, 0.0, f32::INFINITY), (2, 0.0, 1e20)]
    {
        assert_eq!(call(nt_gdi_angle_arc, &[fixture.handle, 8, 8, radius as u64,
            angle.to_bits() as u64, sweep.to_bits() as u64]), 0);
    }
    assert_eq!(call(nt_gdi_arc_internal, &[4, fixture.handle, 2, 2, 14, 14, 14, 8, 8, 2]), 0);
    let dc = dc::get_dc(fixture.handle).unwrap();
    assert!(dc.path.entries.is_empty());
    assert_eq!((dc.cur_x, dc.cur_y), (0, 0));
    assert_eq!(fixture.pixel(8, 8), 0x123456);
}

#[test]
fn chord_and_pie_close_figures_but_arc_preserves_an_open_contour() {
    for kind in 0..4 {
        let fixture = DrawingFixture::new();
        assert_eq!(call(path::services::nt_gdi_begin_path, &[fixture.handle]), 1);
        assert_eq!(call(nt_gdi_arc_internal,
            &[kind, fixture.handle, 2, 2, 14, 14, 14, 8, 8, 2]), 1);
        let dc = dc::get_dc(fixture.handle).unwrap();
        assert_eq!(dc.path.entries.last().unwrap().kind & path::PT_CLOSEFIGURE != 0, kind >= 2);
        if kind != 1 { assert_eq!((dc.cur_x, dc.cur_y), (0, 0)); }
    }
}

#[test]
fn arc_to_current_position_uses_the_original_logical_ellipse() {
    let fixture = DrawingFixture::new();
    assert_eq!(call(nt_gdi_arc_internal,
        &[1, fixture.handle, 2, 2, 14, 14, 14, 8, 8, 2]), 1);
    let dc = dc::get_dc(fixture.handle).unwrap();
    assert_eq!((dc.cur_x, dc.cur_y), (8, 2));
}

#[test]
fn curved_rendering_obeys_clip_and_supports_affine_path_coordinates() {
    let fixture = DrawingFixture::new();
    let clip = create([0, 0, 8, 16]);
    assert_ne!(region::gre_select_clip_rgn(fixture.handle, clip), 0);
    assert_eq!(call(nt_gdi_round_rect, &[fixture.handle, 2, 2, 14, 14, 6, 6]), 1);
    assert_ne!(fixture.pixel(5, 8), 0x123456);
    assert_eq!(fixture.pixel(10, 8), 0x123456);
    {
        let mut dc = dc::get_dc(fixture.handle).unwrap();
        dc.graphics_mode = 2;
        dc.world_transform = dc::Xform { m11: 1.0, m12: 0.5, m21: 0.0,
            m22: 1.0, dx: 0.0, dy: 0.0 };
        dc.world_transform_valid = true;
    }
    assert_eq!(call(path::services::nt_gdi_begin_path, &[fixture.handle]), 1);
    assert_eq!(call(nt_gdi_ellipse, &[fixture.handle, 2, 2, 14, 14]), 1);
    let dc = dc::get_dc(fixture.handle).unwrap();
    let start = dc.path.entries[0].point;
    assert_eq!((start.x, start.y), (14, 15));
    drop(dc);
    assert!(objects::gdi_delete_object(clip));
}
