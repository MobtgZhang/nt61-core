use super::*;
use region_geometry::RegionCombineMode;

fn check_pixels(handle: u64, rows: &[&str]) {
    assert_ne!(handle, 0);
    let geometry = region::snapshot(handle).unwrap();
    for y in -1..rows.len() as i32 + 1 {
        for x in -1..rows[0].len() as i32 + 1 {
            let expected = y >= 0
                && (y as usize) < rows.len()
                && x >= 0
                && (x as usize) < rows[0].len()
                && rows[y as usize].as_bytes()[x as usize] == b'#';
            assert_eq!(geometry.contains_point(x, y), expected, "({x},{y})");
        }
    }
    assert!(objects::gdi_delete_object(handle));
}

#[test]
fn elliptic_and_rounded_regions_match_integer_boundary_masks() {
    use region::services::*;
    check_pixels(call(nt_gdi_create_elliptic_rgn, &[0, 0, 6, 6]),
        &[".###.", "#####", "#####", "#####", ".###."]);
    check_pixels(call(nt_gdi_create_round_rect_rgn, &[0, 0, 8, 7, 4, 4]),
        &[".#####.", "#######", "#######", "#######", "#######", ".#####."]);
    check_pixels(call(nt_gdi_create_round_rect_rgn, &[0, 0, 7, 8, 5, 6]),
        &[".####.", "######", "######", "######", "######", "######", ".####."]);
}

#[test]
fn reversed_bounds_and_negative_diameters_preserve_region_identity() {
    use region::services::*;
    let first = call(nt_gdi_create_round_rect_rgn, &[2, 3, 19, 24, 7, 9]);
    let reversed = call(nt_gdi_create_round_rect_rgn,
        &[19, 24, 2, 3, (-7i64) as u64, (-9i64) as u64]);
    assert_ne!(first, 0);
    assert_eq!(region::snapshot(first), region::snapshot(reversed));
    for handle in [first, reversed] {
        assert!(objects::gdi_delete_object(handle));
    }
    let ellipse = call(nt_gdi_create_elliptic_rgn, &[0, 0, 6, 6]);
    let oversized = call(nt_gdi_create_round_rect_rgn, &[0, 0, 6, 6, 500, 500]);
    assert_eq!(region::snapshot(ellipse), region::snapshot(oversized));
    for handle in [ellipse, oversized] {
        assert!(objects::gdi_delete_object(handle));
    }
    assert_eq!(call(nt_gdi_create_elliptic_rgn, &[0, 0, 1 << 27, 6]), 0);
    assert_eq!(call(nt_gdi_create_elliptic_rgn, &[0, 0, 100000, 100000]), 0);
}

#[test]
fn rounded_region_fill_obeys_device_clip_and_preserves_corner_pixels() {
    let fixture = DrawingFixture::new();
    let clip = create([0, 0, 4, 16]);
    assert_ne!(region::gre_select_clip_rgn(fixture.handle, clip), 0);
    let shape = call(region::services::nt_gdi_create_elliptic_rgn, &[0, 0, 6, 6]);
    let brush = objects::gdi_create_solid_brush(0x000000ff);
    assert_eq!(call(raster::nt_gdi_fill_rgn, &[fixture.handle, shape, brush]), 1);
    let geometry = region::snapshot(shape).unwrap();
    for y in 0..16 {
        for x in 0..16 {
            let painted = x < 4 && geometry.contains_point(x, y);
            assert_eq!(fixture.pixel(x, y) & 0xffffff,
                if painted { 0xff0000 } else { 0x123456 });
        }
    }
    for handle in [clip, shape, brush] {
        assert!(objects::gdi_delete_object(handle));
    }
}

#[test]
fn region_frames_keep_holes_and_do_not_mutate_the_source() {
    let outer = Region::rectangle(Rect::new(1, 1, 14, 14)).unwrap();
    let hole = Region::rectangle(Rect::new(5, 5, 10, 10)).unwrap();
    let geometry = outer.combine(&hole, RegionCombineMode::Difference).unwrap();
    let frame = geometry.frame(-1, -2).unwrap();
    for y in 0..16 {
        for x in 0..16 {
            let interior = geometry.contains_point(x, y)
                && [(-1, -2), (-1, 2), (1, -2), (1, 2)].iter()
                    .all(|&(dx, dy)| geometry.contains_point(x + dx, y + dy));
            assert_eq!(frame.contains_point(x, y), geometry.contains_point(x, y) && !interior);
        }
    }
    assert!(!geometry.contains_point(7, 7));
    assert!(geometry.frame(0, 0).unwrap().rectangles().is_empty());
    assert!(geometry.frame(i32::MIN, 1).is_err());
    let fixture = DrawingFixture::new();
    let handle = objects::gdi_create_region(geometry.clone());
    let brush = objects::gdi_create_solid_brush(0x000000ff);
    assert_eq!(call(raster::nt_gdi_frame_rgn, &[fixture.handle, handle, brush, 1, 2]), 1);
    for y in 0..16 {
        for x in 0..16 {
            assert_eq!(fixture.pixel(x, y) & 0xffffff,
                if frame.contains_point(x, y) { 0xff0000 } else { 0x123456 });
        }
    }
    assert_eq!(region::snapshot(handle), Some(geometry));
    for handle in [handle, brush] {
        assert!(objects::gdi_delete_object(handle));
    }
}
