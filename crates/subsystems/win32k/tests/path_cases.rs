//! Native ABI calls exercise production path state, handle ownership and pixels.
use super::*;
use dc::PointL;
use path::services::*;
use path::{GdiPath, PathState, PT_BEZIERTO, PT_CLOSEFIGURE, PT_LINETO, PT_MOVETO};
use polygon::services::*;

const GDI_ERROR: u64 = u32::MAX as u64;

#[test]
fn compatible_rectangle_at_minimum_device_coordinate_fails_without_panicking() {
    let mut context = dc::DcObject::new();
    context.world_transform.m11 = 0.0;
    context.world_transform.dx = i32::MIN as f32;
    context.path.begin(PointL::new(0, 0));
    assert!(!path::rectangle(&mut context, Rect::new(0, 0, 10, 10)));
    assert!(context.path.entries.is_empty());
    assert_eq!(context.path.state, PathState::Open);
}

fn writable_pointer<T>(value: &mut T) -> u64 {
    let pointer = value as *mut T as u64;
    mm::user_copy::allow(pointer, core::mem::size_of::<T>(), true);
    pointer
}

fn begin(handle: u64) {
    assert_eq!(call(nt_gdi_begin_path, &[handle]), 1);
}

fn end(handle: u64) {
    assert_eq!(call(nt_gdi_end_path, &[handle]), 1);
}

fn line(handle: u64, x: i32, y: i32) {
    assert!(gdi_ops::gre_line_to(&mut dc::get_dc(handle).unwrap(), x, y));
}

fn move_to(handle: u64, x: i32, y: i32) {
    assert!(path::move_to(
        &mut dc::get_dc(handle).unwrap(),
        PointL::new(x, y)
    ));
}

fn rectangle(handle: u64, rect: [i32; 4]) {
    let [left, top, right, bottom] = rect;
    assert!(gdi_ops::gre_rectangle(
        &mut dc::get_dc(handle).unwrap(),
        left,
        top,
        right,
        bottom
    ));
}

fn read_path(handle: u64) -> (Vec<(i32, i32)>, Vec<u8>) {
    let count = call(nt_gdi_get_path, &[handle, 0, 0, 0]);
    assert_ne!(count, GDI_ERROR);
    if count == 0 {
        return (Vec::new(), Vec::new());
    }
    let mut points = vec![PointL::default(); count as usize];
    // The native service requires DWORD alignment for the BYTE array too.
    let mut types = vec![0u32; (count as usize + 3) / 4];
    let points_pointer = points.as_mut_ptr() as u64;
    let types_pointer = types.as_mut_ptr() as u64;
    mm::user_copy::allow(points_pointer, count as usize * 8, true);
    mm::user_copy::allow(types_pointer, count as usize, true);
    assert_eq!(
        call(
            nt_gdi_get_path,
            &[handle, points_pointer, types_pointer, count]
        ),
        count
    );
    let bytes = types
        .iter()
        .flat_map(|word| word.to_ne_bytes())
        .take(count as usize)
        .collect();
    (points.iter().map(|p| (p.x, p.y)).collect(), bytes)
}

fn poly_draw(handle: u64, points: &[PointL], types: &[u8]) -> u64 {
    assert_eq!(points.len(), types.len());
    let points_pointer = points.as_ptr() as u64;
    let types_pointer = types.as_ptr() as u64;
    mm::user_copy::allow(points_pointer, points.len() * 8, false);
    mm::user_copy::allow(types_pointer, types.len(), false);
    call(
        nt_gdi_poly_draw,
        &[handle, points_pointer, types_pointer, points.len() as u64],
    )
}

fn multi_draw(handle_or_mode: u64, points: &[PointL], counts: &[u32], function: u64) -> u64 {
    let points_pointer = points.as_ptr() as u64;
    let counts_pointer = counts.as_ptr() as u64;
    mm::user_copy::allow(points_pointer, points.len() * 8, false);
    mm::user_copy::allow(counts_pointer, counts.len() * 4, false);
    call(
        nt_gdi_poly_poly_draw,
        &[
            handle_or_mode,
            points_pointer,
            counts_pointer,
            counts.len() as u64,
            function,
        ],
    )
}

#[test]
fn path_lifecycle_and_empty_consumption_match_native_contract() {
    let fixture = DrawingFixture::new();
    let h = fixture.handle;
    for service in [
        nt_gdi_end_path,
        nt_gdi_close_figure,
        nt_gdi_flatten_path,
        nt_gdi_fill_path,
        nt_gdi_stroke_path,
        nt_gdi_stroke_and_fill_path,
    ] {
        assert_eq!(call(service, &[h]), 0);
        assert_eq!(call(service, &[0]), 0);
    }
    assert_eq!(call(nt_gdi_get_path, &[h, 0, 0, 0]), GDI_ERROR);
    assert_eq!(call(nt_gdi_abort_path, &[h]), 1);
    begin(h);
    assert_eq!(call(nt_gdi_close_figure, &[h]), 1);
    assert_eq!(call(nt_gdi_get_path, &[h, 0, 0, 0]), GDI_ERROR);
    end(h);
    assert_eq!(read_path(h), (vec![], vec![]));
    assert_eq!(call(nt_gdi_flatten_path, &[h]), 1);
    assert_eq!(call(nt_gdi_path_to_region, &[h]), 0);
    assert_eq!(dc::get_dc(h).unwrap().path.state, PathState::None);
    for service in [
        nt_gdi_fill_path,
        nt_gdi_stroke_path,
        nt_gdi_stroke_and_fill_path,
    ] {
        begin(h);
        end(h);
        assert_eq!(call(service, &[h]), 1);
        assert_eq!(call(service, &[h]), 0);
    }
    begin(h);
    end(h);
    assert_eq!(call(nt_gdi_select_clip_path, &[h, 5]), 0);
    assert_eq!(dc::get_dc(h).unwrap().path.state, PathState::None);
    assert_eq!(fixture.pixel(0, 0), 0x00123456);
}

#[test]
fn repeated_begin_retains_path_and_close_marks_without_adding_points() {
    let fixture = DrawingFixture::new();
    let h = fixture.handle;
    move_to(h, 2, 3);
    begin(h);
    move_to(h, 4, 5);
    assert!(dc::get_dc(h).unwrap().path.entries.is_empty());
    line(h, 9, 5);
    begin(h);
    line(h, 9, 10);
    assert_eq!(call(nt_gdi_close_figure, &[h]), 1);
    end(h);
    assert_eq!(
        read_path(h),
        (
            vec![(4, 5), (9, 5), (9, 10)],
            vec![PT_MOVETO, PT_LINETO, PT_LINETO | PT_CLOSEFIGURE]
        )
    );
    begin(h);
    end(h);
    assert!(read_path(h).0.is_empty());
}

#[test]
fn recording_defers_surface_writes_and_stroke_consumes_the_path() {
    let fixture = DrawingFixture::new();
    let h = fixture.handle;
    move_to(h, 1, 2);
    begin(h);
    line(h, 7, 2);
    assert_eq!(fixture.pixel(3, 2), 0x00123456);
    assert_eq!(call(nt_gdi_stroke_path, &[h]), 0);
    assert_eq!(dc::get_dc(h).unwrap().path.state, PathState::Open);
    end(h);
    assert_eq!(call(nt_gdi_stroke_path, &[h]), 1);
    for x in 1..7 {
        assert_eq!(fixture.pixel(x, 2), 0);
    }
    assert_eq!(fixture.pixel(7, 2), 0x00123456);
    assert_eq!(call(nt_gdi_stroke_path, &[h]), 0);
}

#[test]
fn save_restore_owns_independent_open_and_closed_paths() {
    let fixture = DrawingFixture::new();
    let h = fixture.handle;
    begin(h);
    line(h, 2, 3);
    assert_eq!(dc::gre_save_dc(h), 1);
    line(h, 5, 6);
    end(h);
    assert_eq!(dc::gre_save_dc(h), 2);
    assert_eq!(call(nt_gdi_abort_path, &[h]), 1);
    assert!(dc::gre_restore_dc(h, -1));
    assert_eq!(read_path(h).0, vec![(0, 0), (2, 3), (5, 6)]);
    assert!(dc::gre_restore_dc(h, -1));
    assert_eq!(dc::get_dc(h).unwrap().path.state, PathState::Open);
    line(h, 8, 9);
    end(h);
    assert_eq!(read_path(h).0, vec![(0, 0), (2, 3), (8, 9)]);
}

#[test]
fn get_path_uses_query_time_inverse_and_region_preserves_device_geometry() {
    let fixture = DrawingFixture::new();
    let h = fixture.handle;
    dc::get_dc(h).unwrap().viewport_org = PointL::new(5, 6);
    begin(h);
    rectangle(h, [1, 1, 6, 6]);
    end(h);
    assert_eq!(read_path(h).0, vec![(5, 1), (1, 1), (1, 5), (5, 5)]);
    dc::get_dc(h).unwrap().viewport_org = PointL::new(0, 0);
    assert_eq!(read_path(h).0, vec![(10, 7), (6, 7), (6, 11), (10, 11)]);
    let region = call(nt_gdi_path_to_region, &[h]);
    assert_ne!(region, 0);
    assert_eq!(bounds(region), (2, Rect::new(6, 7, 10, 11)));
    assert!(objects::gdi_delete_object(region));
    assert_eq!(call(nt_gdi_get_path, &[h, 0, 0, 0]), GDI_ERROR);
}

#[test]
fn get_path_probes_full_capacities_and_both_outputs_before_writing() {
    let fixture = DrawingFixture::new();
    let h = fixture.handle;
    begin(h);
    line(h, 3, 4);
    end(h);
    let mut points = [PointL::new(-99, -98); 3];
    let p = writable_pointer(&mut points);
    let mut types = [0x55555555u32; 2];
    let t = writable_pointer(&mut types);
    for arguments in [
        [h, p, t, 1],
        [h, p, 0xdeadbeef, 2],
        [h, p + 1, t, 2],
        [h, p, t + 1, 2],
        [h, p, t, 4],
        [h, p, t, u32::MAX as u64],
        [h, p, t, 0x20000000],
    ] {
        assert_eq!(call(nt_gdi_get_path, &arguments), GDI_ERROR);
        assert!(points.iter().all(|point| point.x == -99 && point.y == -98));
        assert_eq!(types, [0x55555555; 2]);
    }
    assert_eq!(call(nt_gdi_get_path, &[h, 0, 0, 0]), 2);
    assert_eq!(call(nt_gdi_get_path, &[h, p, t, 3]), 2);
    assert_eq!((points[0].x, points[1].x, points[2].x), (0, 3, -99));
}

#[test]
fn compound_polygon_fill_distinguishes_alternate_and_winding_holes() {
    let outer = [
        PointL::new(-5, -5),
        PointL::new(5, -5),
        PointL::new(5, 5),
        PointL::new(-5, 5),
    ];
    let inner = [
        PointL::new(-2, -2),
        PointL::new(2, -2),
        PointL::new(2, 2),
        PointL::new(-2, 2),
    ];
    let mut reversed = inner;
    reversed.reverse();
    for (mode, hole, has_center) in [(1, &inner, false), (2, &inner, true), (2, &reversed, false)] {
        let region = polygon::region_from_contours(&[&outer, hole], mode).unwrap();
        assert_eq!(region.contains_point(0, 0), has_center);
        assert!(region.contains_point(-5, -5));
        assert!(!region.contains_point(5, 0));
        assert_eq!(region.bounds(), Rect::new(-5, -5, 5, 5));
    }
    assert!(polygon::region_from_contours(&[&outer], 0).is_err());
}

#[test]
fn polygon_sloping_edges_use_integer_scanlines_and_exclusive_bottom_right() {
    let triangle = [PointL::new(1, 1), PointL::new(5, 1), PointL::new(1, 5)];
    for offset in [PointL::new(0, 0), PointL::new(-7, -8)] {
        let shifted = triangle.map(|p| PointL::new(p.x + offset.x, p.y + offset.y));
        let mut reversed = shifted;
        reversed.reverse();
        for contour in [&shifted, &reversed] {
            for mode in [1, 2] {
                let region = polygon::region_from_contours(&[contour], mode).unwrap();
                for y in 0..7 {
                    for x in 0..7 {
                        let expected = (1..5).contains(&y) && x >= 1 && x < 6 - y;
                        assert_eq!(
                            region.contains_point(x + offset.x, y + offset.y),
                            expected,
                            "({x},{y}), offset ({},{}), mode {mode}",
                            offset.x,
                            offset.y
                        );
                    }
                }
            }
        }
    }
    // Non-integral crossings exercise upward rounding on both sides of zero.
    let slanted = [PointL::new(-2, 0), PointL::new(2, 0), PointL::new(0, 3)];
    let region = polygon::region_from_contours(&[&slanted], 1).unwrap();
    assert_eq!(
        region.rectangles(),
        &[
            Rect::new(-2, 0, 2, 1),
            Rect::new(-1, 1, 2, 2),
            Rect::new(0, 2, 1, 3),
        ]
    );
}

#[test]
fn fill_path_uses_the_default_white_brush_and_closes_open_figures() {
    let fixture = DrawingFixture::new();
    let h = fixture.handle;
    begin(h);
    assert_eq!(
        poly_draw(
            h,
            &[PointL::new(1, 1), PointL::new(5, 1), PointL::new(1, 5)],
            &[PT_MOVETO, PT_LINETO, PT_LINETO]
        ),
        1
    );
    end(h);
    assert_eq!(call(nt_gdi_fill_path, &[h]), 1);
    for y in 0..16 {
        for x in 0..16 {
            let covered = (1..5).contains(&y) && x >= 1 && x < 6 - y;
            assert_eq!(
                fixture.pixel(x, y),
                if covered { 0x00ffffff } else { 0x00123456 }
            );
        }
    }
    assert_eq!(call(nt_gdi_get_path, &[h, 0, 0, 0]), GDI_ERROR);
}

#[test]
fn select_clip_path_consumes_geometry_and_survives_path_replacement() {
    let fixture = DrawingFixture::new();
    let h = fixture.handle;
    begin(h);
    rectangle(h, [2, 3, 10, 11]);
    end(h);
    assert_eq!(call(nt_gdi_select_clip_path, &[h, 0]), 0);
    assert_eq!(dc::get_dc(h).unwrap().path.state, PathState::Closed);
    assert_eq!(call(nt_gdi_select_clip_path, &[h, 5]), 1);
    assert_eq!(dc::get_dc(h).unwrap().path.state, PathState::None);
    begin(h);
    rectangle(h, [0, 0, 2, 2]);
    assert_eq!(call(nt_gdi_abort_path, &[h]), 1);
    assert_eq!(call(clipping::nt_gdi_pt_visible, &[h, 3, 4]), 1);
    assert_eq!(call(clipping::nt_gdi_pt_visible, &[h, 0, 0]), 0);
    begin(h);
    rectangle(h, [4, 5, 8, 9]);
    end(h);
    assert_eq!(call(nt_gdi_select_clip_path, &[h, 4]), 1);
    assert_eq!(call(clipping::nt_gdi_pt_visible, &[h, 5, 6]), 0);
    assert_eq!(call(clipping::nt_gdi_pt_visible, &[h, 3, 4]), 1);
}

#[test]
fn stroke_and_fill_xor_draws_each_covered_pixel_once_with_clipping() {
    let fixture = DrawingFixture::new();
    let h = fixture.handle;
    let brush = objects::gdi_create_solid_brush(0x00ffffff);
    let pen = objects::gdi_create_pen(objects::PenStyle::Solid, 1, 0x00ffffff);
    call(selection::nt_gdi_select_brush, &[h, brush]);
    call(selection::nt_gdi_select_pen, &[h, pen]);
    dc::get_dc(h).unwrap().rop_mode = 7; // R2_XORPEN
    assert_eq!(
        call(clipping::nt_gdi_intersect_clip_rect, &[h, 0, 0, 8, 16]),
        3
    );
    begin(h);
    rectangle(h, [2, 2, 11, 11]);
    end(h);
    assert_eq!(call(nt_gdi_stroke_and_fill_path, &[h]), 1);
    for y in 0..16 {
        for x in 0..16 {
            let expected = if (2..8).contains(&x) && (2..=10).contains(&y) {
                0x00123456 ^ 0x00ffffff
            } else {
                0x00123456
            };
            assert_eq!(fixture.pixel(x, y), expected, "({x},{y})");
        }
    }
    drop(fixture);
    assert!(objects::gdi_delete_object(brush));
    assert!(objects::gdi_delete_object(pen));
}

#[test]
fn bezier_flatten_retains_endpoints_and_close_flags_without_consumption() {
    let fixture = DrawingFixture::new();
    let h = fixture.handle;
    begin(h);
    let points = [
        PointL::new(1, 1),
        PointL::new(1, 12),
        PointL::new(12, 12),
        PointL::new(12, 1),
    ];
    assert_eq!(
        poly_draw(
            h,
            &points,
            &[
                PT_MOVETO,
                PT_BEZIERTO,
                PT_BEZIERTO,
                PT_BEZIERTO | PT_CLOSEFIGURE
            ]
        ),
        1
    );
    end(h);
    assert_eq!(read_path(h).0.len(), 4);
    assert_eq!(call(nt_gdi_flatten_path, &[h]), 1);
    let (points, types) = read_path(h);
    assert!(points.len() > 4);
    assert_eq!(points.first(), Some(&(1, 1)));
    assert_eq!(points.last(), Some(&(12, 1)));
    assert!(points.iter().any(|point| point.1 >= 9));
    assert_eq!(types[0], PT_MOVETO);
    assert!(types[1..types.len() - 1]
        .iter()
        .all(|kind| *kind == PT_LINETO));
    assert_eq!(*types.last().unwrap(), PT_LINETO | PT_CLOSEFIGURE);
    assert_eq!(call(nt_gdi_flatten_path, &[h]), 1);
    assert_eq!(read_path(h).0, points);
}

#[test]
fn collinear_bezier_loop_does_not_collapse_to_its_endpoint() {
    let mut path = GdiPath::default();
    path.begin(PointL::new(0, 0));
    assert!(path.append(
        &[PointL::new(100, 0), PointL::new(-100, 0), PointL::new(0, 0)],
        PT_BEZIERTO
    ));
    let flat = path.flattened().unwrap();
    assert!(flat.entries.iter().any(|entry| entry.point.x > 20));
    assert!(flat.entries.iter().any(|entry| entry.point.x < -20));
    assert_eq!(flat.entries.last().unwrap().point.x, 0);
}

#[test]
fn poly_draw_invalid_commands_leave_position_and_recording_unchanged() {
    let fixture = DrawingFixture::new();
    let h = fixture.handle;
    begin(h);
    line(h, 3, 4);
    for types in [
        [PT_LINETO, PT_BEZIERTO, PT_LINETO],
        [PT_MOVETO, PT_LINETO, 0],
        [PT_MOVETO, PT_LINETO, PT_MOVETO | PT_CLOSEFIGURE],
    ] {
        assert_eq!(poly_draw(h, &[PointL::new(9, 9); 3], &types), 0);
        let dc = dc::get_dc(h).unwrap();
        assert_eq!((dc.cur_x, dc.cur_y, dc.path.entries.len()), (3, 4, 2));
    }
    assert_eq!(call(nt_gdi_poly_draw, &[h, 0, 0, 0]), 1);
    assert_eq!(call(nt_gdi_poly_draw, &[h, 0, 0, 65537]), 0);
    assert_eq!(call(nt_gdi_poly_draw, &[h, u64::MAX - 2, 1, 2]), 0);
}

#[test]
fn poly_draw_close_keeps_logical_endpoint_and_restarts_device_figure() {
    let fixture = DrawingFixture::new();
    let h = fixture.handle;
    move_to(h, 1, 2);
    begin(h);
    let points = [PointL::new(7, 2), PointL::new(7, 8)];
    assert_eq!(
        poly_draw(h, &points, &[PT_LINETO, PT_LINETO | PT_CLOSEFIGURE]),
        1
    );
    {
        let dc = dc::get_dc(h).unwrap();
        assert_eq!((dc.cur_x, dc.cur_y), (7, 8));
    }
    line(h, 3, 4);
    end(h);
    assert_eq!(read_path(h).0, vec![(1, 2), (7, 2), (7, 8), (1, 2), (3, 4)]);
}

#[test]
fn poly_poly_draw_uses_windows_selectors_and_updates_only_to_position() {
    let fixture = DrawingFixture::new();
    let h = fixture.handle;
    begin(h);
    assert_eq!(multi_draw(h, &[PointL::new(3, 4)], &[1], 4), 1); // PolylineTo
    assert_eq!(multi_draw(h, &[PointL::new(9, 9)], &[1], 5), 0); // BezierTo needs three
    let controls = [PointL::new(5, 8), PointL::new(8, 8), PointL::new(10, 4)];
    assert_eq!(multi_draw(h, &controls, &[3], 5), 1);
    let standalone = [
        PointL::new(1, 1),
        PointL::new(1, 3),
        PointL::new(3, 3),
        PointL::new(3, 1),
    ];
    assert_eq!(multi_draw(h, &standalone, &[4], 3), 1);
    {
        let dc = dc::get_dc(h).unwrap();
        assert_eq!((dc.cur_x, dc.cur_y), (10, 4));
    }
    end(h);
    let (_, types) = read_path(h);
    assert_eq!(
        &types[..5],
        &[PT_MOVETO, PT_LINETO, PT_BEZIERTO, PT_BEZIERTO, PT_BEZIERTO]
    );
    assert_eq!(multi_draw(h, &[], &[], 4), 0);
    assert_eq!(multi_draw(h, &[], &[0], 4), 1);
    assert_eq!(multi_draw(h, &standalone, &[2, 2], 4), 0);
}

#[test]
fn poly_polygon_region_selector_uses_fill_mode_without_a_dc() {
    let points = [
        PointL::new(-2, -3),
        PointL::new(5, -3),
        PointL::new(5, 6),
        PointL::new(-2, 6),
    ];
    let handle = multi_draw(1, &points, &[4], 6);
    assert_ne!(handle, 0);
    assert_eq!(bounds(handle), (2, Rect::new(-2, -3, 5, 6)));
    assert!(objects::gdi_delete_object(handle));
    assert_eq!(multi_draw(3, &points, &[4], 6), 0);
    assert_eq!(multi_draw(1, &points, &[u32::MAX], 6), 0);
}

#[test]
fn miter_limit_validates_float_bits_alignment_and_native_failure_order() {
    let fixture = DrawingFixture::new();
    let h = fixture.handle;
    let mut old = [0u32; 2];
    let p = writable_pointer(&mut old);
    assert_eq!(call(nt_gdi_get_miter_limit, &[h, p]), 1);
    assert_eq!(f32::from_bits(old[0]), 10.0);
    for value in [-1.0f32, 0.0, 0.99, f32::NEG_INFINITY] {
        assert_eq!(
            call(nt_gdi_set_miter_limit, &[h, value.to_bits() as u64, p]),
            0
        );
        assert_eq!(dc::get_dc(h).unwrap().miter_limit, 10.0);
    }
    assert_eq!(
        call(nt_gdi_set_miter_limit, &[h, 2.5f32.to_bits() as u64, p]),
        1
    );
    assert_eq!(f32::from_bits(old[0]), 10.0);
    assert_eq!(dc::gre_save_dc(h), 1);
    assert_eq!(
        call(nt_gdi_set_miter_limit, &[h, 4f32.to_bits() as u64, p + 1]),
        0
    );
    assert_eq!(dc::get_dc(h).unwrap().miter_limit, 4.0);
    assert_eq!(call(nt_gdi_get_miter_limit, &[h, p + 1]), 0);
    assert!(dc::gre_restore_dc(h, -1));
    assert_eq!(dc::get_dc(h).unwrap().miter_limit, 2.5);
    for bits in [f32::INFINITY.to_bits(), 0x7fc01234] {
        assert_eq!(call(nt_gdi_set_miter_limit, &[h, bits as u64, 0]), 1);
        assert_eq!(call(nt_gdi_get_miter_limit, &[h, p]), 1);
        assert_eq!(old[0], bits);
    }
}

#[test]
fn scanline_canonicalization_merges_equal_bands_and_rejects_invalid_input() {
    let region = Region::from_scanlines(&[
        Rect::new(-3, -1, -1, 0),
        Rect::new(1, -1, 4, 0),
        Rect::new(-3, 0, -1, 1),
        Rect::new(1, 0, 4, 1),
        Rect::new(-3, 2, -1, 3),
    ])
    .unwrap();
    assert_eq!(
        region.rectangles(),
        &[
            Rect::new(-3, -1, -1, 1),
            Rect::new(1, -1, 4, 1),
            Rect::new(-3, 2, -1, 3)
        ]
    );
    for rows in [
        vec![Rect::new(0, 0, 2, 2)],
        vec![Rect::new(0, 0, 2, 1), Rect::new(2, 0, 3, 1)],
        vec![Rect::new(0, 1, 2, 2), Rect::new(0, 0, 2, 1)],
        vec![Rect::new(0, 0, 0, 1)],
    ] {
        assert!(Region::from_scanlines(&rows).is_err());
    }
}
