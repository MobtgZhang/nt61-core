use super::*;
use system_colors::*;

fn set(indices: &[u32], colors: &[u32], count: u64, flags: u64) -> u64 {
    mm::user_copy::allow(indices.as_ptr() as u64, indices.len() * 4, false);
    mm::user_copy::allow(colors.as_ptr() as u64, colors.len() * 4, false);
    call(nt_user_set_sys_colors, &[count, indices.as_ptr() as u64, colors.as_ptr() as u64, flags])
}

#[test]
fn shared_system_colors_capture_inputs_update_live_brush_pixels_and_repaint_consumers() {
    let original: Vec<u32> = (0..COLOR_COUNT as i32).map(get_sys_color).collect();
    let brush = get_sys_color_brush(5);
    assert_ne!(brush, 0);
    assert_eq!(get_sys_color_brush(5), brush);
    objects::with_handle_table(|table| {
        let entry = table.get(brush).unwrap();
        assert_eq!(entry.process_id, 0);
        assert_ne!(entry.flags & objects::HANDLE_FLAG_STOCK, 0);
    });
    assert!(objects::gdi_delete_object(brush));
    assert_eq!(get_sys_color_brush(5), brush);
    assert!(objects::with_handle_table(|table| table.get(brush).is_some()));
    assert_eq!(get_sys_color(-1), 0);
    assert_eq!(get_sys_color(COLOR_COUNT as i32), 0);
    assert_eq!(get_sys_color_brush(-1), 0);
    assert_eq!(get_sys_color_brush(COLOR_COUNT as i32), 0);

    // Capture is complete before commit, including ignored/out-of-range indices.
    assert_eq!(set(&[5, u32::MAX], &[0x112233], 2, 0), 0);
    assert_eq!(get_sys_color(5), original[5]);

    assert_eq!(set(&[5], &[0xa2112233], 1, 0), 1);
    assert_eq!(get_sys_color(5), 0x00112233);
    assert_eq!(set(&[5, 6], &[0, 0x01000001], 2, 0), 0);
    assert_eq!(get_sys_color(5), 0x00112233);
    assert!(set_system_colors(&[5], &[original[5]], 0));
    assert_eq!(set(&[5], &[0x112233], u32::MAX as u64, 0), 0);
    assert_eq!(call(nt_user_set_sys_colors, &[0, 1, 1, 0]), 1);
    let input = [0x55555555u32; 2];
    mm::user_copy::allow(input.as_ptr() as u64, 8, false);
    assert_eq!(call(nt_user_set_sys_colors, &[1, input.as_ptr() as u64 + 1, input.as_ptr() as u64, 0]), 0);
    assert_eq!(call(nt_user_set_sys_colors, &[1, 1, input.as_ptr() as u64, 0]), 0);
    assert_eq!(get_sys_color(5), original[5]);

    // Count is not clamped to 31: all 40 pairs are captured, and the last
    // occurrence wins. Invalid signed indices and COLOR_COUNT are ignored.
    let mut indices = vec![u32::MAX; 40];
    let mut colors = vec![0; 40];
    indices[0] = 5; colors[0] = 0xff;
    indices[1] = COLOR_COUNT as u32; colors[1] = 0xffffff;
    indices[39] = 5; colors[39] = 0x00112233;
    assert_eq!(set(&indices, &colors, 40 | 1 << 32, 4 | 1 << 32), 1);
    assert_eq!(get_sys_color(5), 0x00112233);
    assert_eq!(objects::gdi_get_brush_color(brush), 0x00112233);
    assert_eq!(background_color(6), Some(0x00112233));
    assert_eq!(background_color(brush), Some(0x00112233));

    // A brush already selected into a real DC changes its rendered pixels
    // without replacing the handle or changing the DC selection.
    let fixture = DrawingFixture::new();
    assert_ne!(call(selection::nt_gdi_select_brush, &[fixture.handle, brush]), 0);
    {
        let mut dc = dc::get_dc(fixture.handle).unwrap();
        assert!(gdi_ops::gre_pat_blt(&mut dc, 0, 0, 4, 4, 0x00f00021));
    }
    assert_eq!(fixture.pixel(0, 0) & 0xffffff, 0x00332211);
    assert_eq!(set(&[5], &[0x00998877], 1, 0), 1);
    assert_eq!(get_sys_color_brush(5), brush);
    {
        let mut dc = dc::get_dc(fixture.handle).unwrap();
        assert!(gdi_ops::gre_pat_blt(&mut dc, 0, 0, 4, 4, 0x00f00021));
    }
    assert_eq!(fixture.pixel(0, 0) & 0xffffff, 0x00778899);

    // Window class backgrounds consume the same symbolic color slot and refresh
    // invalidates children as well as top-level windows, including hidden ones.
    let mut manager = window::WindowManager::new();
    let hwnd = manager.create_window(&[], &[], 0, 0, 0, 0, 20, 20, None, 0, 0).unwrap();
    let child = manager.create_window(&[], &[], window::WindowStyle::CHILD, 0,
        0, 0, 10, 10, Some(hwnd), 0, 0).unwrap();
    manager.classes.push(Some(window::WindowClass {
        name: [0; 64], instance: 0, style: 0, wndproc: 0, cls_extra: 0,
        wnd_extra: 0, h_icon: 0, h_cursor: 0, h_brbackground: 6,
        menu_name: 0, atom: 42, small_icon: 0, extra: Vec::new(),
    }));
    manager.get_window_mut(hwnd).unwrap().class_atom = 42;
    manager.get_window_mut(hwnd).unwrap().dirty = false;
    manager.get_window_mut(child).unwrap().dirty = false;
    assert_eq!(manager.class_background_color(hwnd), Some(0x00998877));
    manager.classes[0].as_mut().unwrap().h_brbackground = brush;
    assert_eq!(manager.class_background_color(hwnd), Some(0x00998877));
    manager.system_colors_changed();
    assert!(manager.get_window(hwnd).unwrap().dirty);
    assert!(manager.get_window(child).unwrap().dirty);
    assert!(!manager.get_window(child).unwrap().dirty_rects.is_empty());

    // Flags requiring desktop authorization remain deliberately unavailable in
    // the unregistered adapter. The trusted update path supports repaint.
    assert_eq!(set(&[5], &[0], 1, 1), 0);
    assert_eq!(get_sys_color(5), 0x00998877);
    assert!(!set_system_colors(&[5], &[], 0));
    assert!(set_system_colors(&(0..COLOR_COUNT as u32).collect::<Vec<_>>(), &original, 0));
}
