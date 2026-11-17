//! Native user-buffer queries run in the host fixture that owns user-memory registration.
use crate::{
    arch::common::syscall_frame::SyscallFrame,
    dc::{self, PointL},
    objects::Rect,
    transforms,
};

#[test]
fn native_layout_widths_queries_and_extent_scaling_preserve_logical_signs() {
    let handle = dc::gre_create_display_dc();
    {
        let mut dc = dc::get_dc(handle).unwrap();
        dc.bounds = Rect::new(0, 0, 8, 6);
        dc.window_org = PointL::new(2, 3);
    }
    let mut frame = SyscallFrame::default();
    frame.args[0] = handle;
    frame.args[1] = 0x12345678ffffffff;
    frame.args[2] = 0xabcdef0100000001;
    assert_eq!(dc::mirror::nt_gdi_set_layout(&frame), 0);
    let query = |selector| {
        let mut point = PointL::new(0, 0);
        let mut query_frame = SyscallFrame::default();
        query_frame.args[0] = handle;
        query_frame.args[1] = selector;
        query_frame.args[2] = (&mut point as *mut PointL) as u64;
        crate::mm::user_copy::allow(query_frame.args[2], core::mem::size_of::<PointL>(), true);
        assert_eq!(transforms::nt_gdi_get_dc_point(&query_frame), 1);
        (point.x, point.y)
    };
    assert_eq!(query(2), (1, 1));
    assert_eq!(query(8), (2, 3));
    let mut old = PointL::new(0, 0);
    assert!(dc::gre_set_viewport_org_ex(handle, 4, 5, Some(&mut old)));
    assert_eq!((old.x, old.y), (0, 0));
    assert_eq!(query(4), (4, 5));
    let mut old_size = dc::SizeL::new(0, 0);
    frame.args[..8].copy_from_slice(&[
        handle,
        2,
        1,
        3,
        1,
        (&mut old_size as *mut dc::SizeL) as u64,
        0,
        0,
    ]);
    crate::mm::user_copy::allow(frame.args[5], core::mem::size_of::<dc::SizeL>(), true);
    assert_eq!(dc::services::nt_gdi_scale_window_ext_ex(&frame), 1);
    assert_eq!((old_size.cx, old_size.cy), (1, 1));
    assert_eq!(query(2), (2, 3));
    assert_eq!(dc::get_dc(handle).unwrap().effective_window_origin().x, 16);
    frame.args[0] = handle;
    assert_eq!(dc::mirror::nt_gdi_mirror_window_org(&frame), 1);
    assert!(dc::gre_delete_dc(handle));
    assert_eq!(dc::mirror::nt_gdi_mirror_window_org(&frame), 0);
    assert_eq!(dc::mirror::nt_gdi_set_layout(&frame), u64::from(u32::MAX));
}
