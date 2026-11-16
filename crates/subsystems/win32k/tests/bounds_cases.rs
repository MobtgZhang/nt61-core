use super::*;
use dc::bounds::{nt_gdi_get_bounds_rect as get, nt_gdi_set_bounds_rect as set};

fn query(handle: u64, flags: u64) -> (u64, Rect) {
    let mut rect = Rect::new(-99, -99, -99, -99);
    mm::user_copy::allow(&mut rect as *mut _ as u64, 16, true);
    (call(get, &[handle, &mut rect as *mut _ as u64, flags]), rect)
}

#[test]
fn bounds_accumulate_real_drawing_and_reset_without_restoring_with_saved_state() {
    let fixture = DrawingFixture::new();
    assert_eq!(query(fixture.handle, 0).0, 1);
    assert_eq!(call(set, &[fixture.handle, 0, 5]), 9);
    assert!(dc::get_dc(fixture.handle).unwrap().save_state());
    assert!(raster::set_pixel(&dc::get_dc(fixture.handle).unwrap(), 3, 4, 0xff));
    assert!(raster::set_pixel(&dc::get_dc(fixture.handle).unwrap(), 8, 10, 0xff));
    assert_eq!(query(fixture.handle, 0), (3, Rect::new(3, 4, 9, 11)));
    assert!(dc::get_dc(fixture.handle).unwrap().restore_state());
    assert_eq!(query(fixture.handle, 1), (3, Rect::new(3, 4, 9, 11)));
    assert_eq!(query(fixture.handle, 0), (1, Rect::new(0, 0, 0, 0)));
    assert_eq!(call(set, &[fixture.handle, 0, 8]), 5);
    assert!(raster::set_pixel(&dc::get_dc(fixture.handle).unwrap(), 1, 1, 0xff));
    assert_eq!(query(fixture.handle, 0).0, 1);
}

#[test]
fn bounds_transform_logical_input_and_keep_window_manager_accumulator_separate() {
    let fixture = DrawingFixture::new();
    {
        let mut dc = dc::get_dc(fixture.handle).unwrap();
        dc.viewport_org = dc::PointL::new(2, 3);
    }
    let rect = Rect::new(7, 8, 1, 2);
    mm::user_copy::allow(&rect as *const _ as u64, 16, false);
    assert_eq!(call(set, &[fixture.handle, &rect as *const _ as u64, 2]), 9);
    assert_eq!(query(fixture.handle, 0), (3, Rect::new(1, 2, 7, 8)));
    assert_eq!(call(set, &[fixture.handle, 0, 0x8005]), 0x800b);
    assert!(raster::set_pixel(&dc::get_dc(fixture.handle).unwrap(), 3, 4, 0xff));
    assert_eq!(query(fixture.handle, 0x8000), (3, Rect::new(5, 7, 6, 8)));
    assert_eq!(query(fixture.handle, 0), (3, Rect::new(1, 2, 7, 8)));
}

#[test]
fn bounds_reject_conflicting_flags_and_honor_reset_on_buffer_faults() {
    let fixture = DrawingFixture::new();
    assert_eq!(call(set, &[fixture.handle, 0, 12]), 0);
    assert_eq!(call(set, &[fixture.handle, 0, 4]), 9);
    assert!(raster::rectangle(&dc::get_dc(fixture.handle).unwrap(), Rect::new(2, 3, 10, 12)));
    assert_eq!(query(fixture.handle, 0), (3, Rect::new(2, 3, 10, 12)));
    assert_eq!(call(get, &[fixture.handle, 1, 1]), 0);
    assert_eq!(query(fixture.handle, 0).0, 1);
    assert_eq!(call(set, &[fixture.handle, 1, 3]), 0);
    assert_eq!(query(fixture.handle, 0).0, 1);
    assert_eq!(call(get, &[u64::MAX, 1, 0]), 0);
}

#[test]
fn bounds_obey_clipping_and_recorded_paths_only_count_when_painted() {
    let fixture = DrawingFixture::new();
    assert_eq!(call(set, &[fixture.handle, 0, 4]), 9);
    let clip = create([0, 0, 7, 16]);
    assert_ne!(region::gre_select_clip_rgn(fixture.handle, clip), 0);
    assert_eq!(call(path::services::nt_gdi_begin_path, &[fixture.handle]), 1);
    assert_eq!(call(path::shapes::nt_gdi_ellipse, &[fixture.handle, 2, 2, 14, 14]), 1);
    assert_eq!(query(fixture.handle, 0).0, 1);
    assert_eq!(call(path::services::nt_gdi_end_path, &[fixture.handle]), 1);
    assert_eq!(call(path::services::nt_gdi_stroke_and_fill_path, &[fixture.handle]), 1);
    let (status, bounds) = query(fixture.handle, 0);
    assert_eq!(status, 3);
    assert!(bounds.left >= 2 && bounds.right <= 7 && bounds.bottom <= 14);
    assert!(objects::gdi_delete_object(clip));
}
