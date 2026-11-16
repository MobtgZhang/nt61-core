//! Exercise the production USER graph and native handlers, rather than copies
//! of their logic. The fixture only replaces message transport and user probes.
use super::*;
use window::{services::*, WindowManager, WindowStyle, WindowStyleEx, WINDOW_MANAGER};

fn create(manager: &mut WindowManager, parent: Option<u64>, style: u32, ex: u32) -> u64 {
    manager.create_window(&[], &[], style, ex, 10, 10, 100, 100, parent, 0, 0).unwrap()
}

fn global_window() -> u64 {
    create(&mut WINDOW_MANAGER.lock(), None, WindowStyle::VISIBLE, 0)
}

#[test]
fn properties_store_full_width_handles_replace_and_remove_atom_entries() {
    let hwnd = global_window();
    assert_eq!(call(nt_user_get_prop, &[hwnd, 0xc001]), 0);
    assert_eq!(call(nt_user_set_prop, &[hwnd, 0xc001, 0xfedc_ba98_7654_3210]), 1);
    assert_eq!(call(nt_user_get_prop, &[hwnd, 0xc001]), 0xfedc_ba98_7654_3210);
    assert_eq!(call(nt_user_set_prop, &[hwnd, 0xc001, 123]), 1);
    assert_eq!(call(nt_user_set_prop, &[hwnd, 0xc002, 456]), 1);
    assert_eq!(call(nt_user_remove_prop, &[hwnd, 0xc001]), 123);
    assert_eq!(call(nt_user_remove_prop, &[hwnd, 0xc001]), 0);
    assert_eq!(call(nt_user_get_prop, &[hwnd, 0xc002]), 456);
    assert!(window::destroy_window_internal(hwnd));
    assert_eq!(call(nt_user_get_prop, &[hwnd, 0xc002]), 0);
    assert_eq!(call(nt_user_set_prop, &[hwnd, 0xc002, 1]), 0);
    assert_eq!(call(nt_user_remove_prop, &[hwnd, 0xc002]), 0);
}

#[test]
fn properties_are_isolated_and_atom_arguments_truncate_to_sixteen_bits() {
    let first = global_window();
    let second = global_window();
    assert_eq!(call(nt_user_set_prop, &[first, 0xfeed_0000_c004, 99]), 1);
    assert_eq!(call(nt_user_get_prop, &[first, 0xc004]), 99);
    assert_eq!(call(nt_user_get_prop, &[second, 0xc004]), 0);
    assert!(window::destroy_window_internal(first));
    assert!(window::destroy_window_internal(second));
}

#[test]
fn owners_are_not_parents_and_destroying_a_parent_destroys_its_children() {
    let mut manager = WindowManager::new();
    let owner = create(&mut manager, None, 0, 0);
    let popup = create(&mut manager, Some(owner), WindowStyle::POPUP, 0);
    let child = create(&mut manager, Some(popup), WindowStyle::CHILD, 0);
    assert_eq!(manager.get_window(popup).unwrap().owner, Some(owner));
    assert_eq!(manager.get_window(popup).unwrap().parent, None);
    assert_eq!(manager.ancestor(child, 1), Some(popup));
    assert_eq!(manager.ancestor(child, 2), Some(popup));
    assert_eq!(manager.ancestor(child, 3), Some(owner));
    assert_eq!(manager.ancestor(child, 4), None);
    assert!(manager.destroy_window(popup));
    assert!(manager.get_window(child).is_none());
    assert!(manager.get_window(owner).is_some());
}

#[test]
fn child_point_queries_use_immediate_siblings_and_half_open_client_bounds() {
    let mut manager = WindowManager::new();
    let parent = create(&mut manager, None, 0, 0);
    let lower = create(&mut manager, Some(parent), WindowStyle::CHILD | WindowStyle::VISIBLE, 0);
    let upper = create(&mut manager, Some(parent), WindowStyle::CHILD, WindowStyleEx::TRANSPARENT);
    let nested = create(&mut manager, Some(lower), WindowStyle::CHILD, 0);
    assert_eq!(manager.child_from_point(parent, 20, 20, 0), Some(upper));
    assert_eq!(manager.child_from_point(parent, 20, 20, 1), Some(lower));
    assert_eq!(manager.child_from_point(parent, 20, 20, 4), Some(lower));
    manager.get_window_mut(upper).unwrap().enabled = false;
    assert_eq!(manager.child_from_point(parent, 20, 20, 2), Some(lower));
    assert_ne!(manager.child_from_point(parent, 20, 20, 7), Some(nested));
    assert_eq!(manager.child_from_point(parent, 0, 0, 0), Some(parent));
    for (x, y, flags) in [(-1, 0, 0), (100, 0, 0), (0, 100, 0), (1, 1, 8)] {
        assert_eq!(manager.child_from_point(parent, x, y, flags), None);
    }
}

#[test]
fn reparent_rejects_cycles_and_preserves_the_screen_position() {
    let mut manager = WindowManager::new();
    let first = create(&mut manager, None, 0, 0);
    let second = create(&mut manager, None, 0, 0);
    manager.set_window_pos(second, 50, 60, 100, 100);
    let child = create(&mut manager, Some(first), WindowStyle::CHILD, 0);
    assert_eq!(manager.reparent(first, Some(child)), None);
    assert_eq!(manager.reparent(child, Some(second)), Some(first));
    let rect = manager.get_window(child).unwrap().rect;
    assert_eq!((rect.left, rect.top), (-30, -40));
    assert!(manager.get_window(first).unwrap().children.is_empty());
    assert_eq!(manager.get_window(second).unwrap().children, [child]);
    assert_eq!(manager.reparent(child, Some(u64::MAX)), None);
    assert_eq!(manager.reparent(child, None), Some(second));
    let rect = manager.get_window(child).unwrap().rect;
    assert_eq!((rect.left, rect.top), (20, 20));
    assert_eq!(manager.get_window(child).unwrap().style, WindowStyle::CHILD);
}

#[test]
fn deleting_windows_clears_active_focus_capture_and_destroys_owned_windows() {
    let mut manager = WindowManager::new();
    let hwnd = create(&mut manager, None, 0, 0);
    let owned = create(&mut manager, Some(hwnd), WindowStyle::POPUP, 0);
    manager.set_focus(hwnd);
    manager.set_capture(hwnd);
    manager.set_active_window(hwnd);
    manager.set_foreground_window(hwnd);
    assert!(manager.destroy_window(hwnd));
    assert_eq!(manager.get_focus(), 0);
    assert_eq!(manager.get_capture(), 0);
    assert_eq!(manager.get_active_window(), 0);
    assert_eq!(manager.get_foreground_window(), 0);
    assert!(manager.get_window(owned).is_none());
}

#[test]
fn property_calls_serialize_with_window_teardown() {
    let hwnd = global_window();
    std::thread::scope(|scope| {
        scope.spawn(|| {
            for value in 0..1000 { call(nt_user_set_prop, &[hwnd, 1, value]); }
        });
        assert!(window::destroy_window_internal(hwnd));
    });
    assert_eq!(call(nt_user_set_prop, &[hwnd, 1, 42]), 0);
}

#[test]
fn prop_list_reports_required_bytes_then_copies_whole_items_with_zero_padding() {
    use window::attributes::nt_user_build_prop_list;
    let hwnd = global_window();
    call(nt_user_set_prop, &[hwnd, 0xc010, u64::MAX]);
    call(nt_user_set_prop, &[hwnd, 0xc011, 123]);
    let mut count = 0u32;
    let count_address = &mut count as *mut u32 as u64;
    mm::user_copy::allow(count_address, 4, true);
    assert_eq!(call(nt_user_build_prop_list, &[hwnd, 0, 0, count_address]), 0);
    assert_eq!(count, 32);
    let mut item = [0xaau8; 16];
    let address = item.as_mut_ptr() as u64;
    mm::user_copy::allow(address, 16, true);
    assert_eq!(call(nt_user_build_prop_list, &[hwnd, address, 15, count_address]), 0xc000000d);
    assert_eq!(item, [0xaa; 16]);
    assert_eq!(call(nt_user_build_prop_list, &[hwnd, address, 16, count_address]), 0);
    assert_eq!(count, 1);
    assert_eq!(&item[..8], &0xc010u64.to_le_bytes());
    assert_eq!(&item[8..], &u64::MAX.to_le_bytes());
    assert_eq!(call(nt_user_build_prop_list, &[hwnd, 1, 16, count_address]), 0xc0000005);
    window::destroy_window_internal(hwnd);
}

#[test]
fn placement_validates_length_and_preserves_restore_rectangle_while_minimized() {
    use window::placement::*;
    let hwnd = global_window();
    let mut placement = Placement { length: 44, flags: 0, show: 2,
        min_x: 40, min_y: 50, max_x: -1, max_y: -1,
        normal: Rect { left: 30, top: 40, right: 230, bottom: 140 } };
    let pointer = &mut placement as *mut Placement as u64;
    mm::user_copy::allow(pointer, 44, true);
    assert_eq!(call(nt_user_set_window_placement, &[hwnd, pointer]), 1);
    placement.normal = Rect::default();
    assert_eq!(call(nt_user_get_window_placement, &[hwnd, pointer]), 1);
    assert_eq!(placement.show, 2);
    assert_eq!(placement.normal.right, 230);
    assert_eq!((placement.min_x, placement.min_y), (40, 50));
    placement.show = 9;
    let pointer = user_pointer(&mut placement, true);
    assert_eq!(call(nt_user_set_window_placement, &[hwnd, pointer]), 1);
    assert_eq!(WINDOW_MANAGER.lock().get_window(hwnd).unwrap().rect.left, 30);
    placement.length = 43;
    let pointer = user_pointer(&mut placement, true);
    assert_eq!(call(nt_user_get_window_placement, &[hwnd, pointer]), 0);
    assert_eq!(call(nt_user_set_window_placement, &[hwnd, pointer]), 0);
    placement.length = 44;
    placement.flags = 8;
    let pointer = user_pointer(&mut placement, true);
    assert_eq!(call(nt_user_set_window_placement, &[hwnd, pointer]), 0);
    assert_eq!(call(nt_user_get_window_placement, &[hwnd, 1]), 0);
    window::destroy_window_internal(hwnd);
}

#[test]
fn pointer_attributes_preserve_high_bits_and_long_and_word_use_their_own_width() {
    use window::attributes::*;
    let hwnd = global_window();
    WINDOW_MANAGER.lock().get_window_mut(hwnd).unwrap().extra.resize(12, 0);
    assert_eq!(call(nt_user_set_window_long_ptr, &[hwnd, (-21i32) as u64, u64::MAX]), 0);
    assert_eq!(call(nt_user_set_window_long_ptr, &[hwnd, (-21i32) as u64, 1]), u64::MAX);
    assert_eq!(call(nt_user_set_window_long_ptr, &[hwnd, 0, 0x1122334455667788]), 0);
    assert_eq!(call(nt_user_set_window_long, &[hwnd, 0, 0xaabbccdd]), 0x55667788);
    assert_eq!(call(nt_user_set_window_word, &[hwnd, 4, 0xeeee]), 0x3344);
    assert_eq!(call(nt_user_set_window_long_ptr, &[hwnd, 0, 0]), 0x1122eeeeaabbccdd);
    assert_eq!(call(nt_user_set_window_long_ptr, &[hwnd, 5, 1]), 0);
    window::destroy_window_internal(hwnd);
}

#[test]
fn hit_testing_respects_front_root_before_descending_into_children() {
    use window::attributes::nt_user_window_from_physical_point;
    let back = global_window();
    let child = create(&mut WINDOW_MANAGER.lock(), Some(back), WindowStyle::CHILD | WindowStyle::VISIBLE, 0);
    let front = global_window();
    {
        let mut manager = WINDOW_MANAGER.lock();
        manager.set_window_pos(back, 5000, 5000, 100, 100);
        manager.set_window_pos(front, 5000, 5000, 100, 100);
    }
    assert_eq!(call(nt_user_window_from_physical_point, &[5030, 5030]), front);
    window::destroy_window_internal(front);
    assert_eq!(call(nt_user_window_from_physical_point, &[5030, 5030]), child);
    window::destroy_window_internal(back);
}

#[test]
fn client_bounds_remain_ordered_for_extreme_and_tiny_rectangles() {
    let mut window = window::WindowObject::new(1);
    window.style = WindowStyle::CAPTION | WindowStyle::BORDER;
    window.rect = Rect { left: i32::MIN, top: i32::MIN, right: i32::MAX, bottom: i32::MAX };
    window.update_client_rect();
    assert!(window.client_rect.right > 0 && window.client_rect.bottom > 0);
    window.set_rect(10, 10, 1, 1);
    assert!(window.client_rect.left <= window.client_rect.right);
    assert!(window.client_rect.top <= window.client_rect.bottom);
}

#[test]
fn registered_class_keeps_identity_after_module_changes_and_uses_case_insensitive_names() {
    use window::class_services::*;
    let name: Vec<u16> = "ClassIdentityTest".encode_utf16().collect();
    let mut raw_name = [0u8; 16];
    raw_name[..2].copy_from_slice(&((name.len() * 2) as u16).to_le_bytes());
    raw_name[2..4].copy_from_slice(&((name.len() * 2) as u16).to_le_bytes());
    raw_name[8..].copy_from_slice(&(name.as_ptr() as u64).to_le_bytes());
    let mut class = [0u8; 80];
    class[..4].copy_from_slice(&80u32.to_le_bytes());
    class[8..16].copy_from_slice(&0x1234u64.to_le_bytes());
    class[16..20].copy_from_slice(&12i32.to_le_bytes());
    class[20..24].copy_from_slice(&16i32.to_le_bytes());
    class[24..32].copy_from_slice(&0x9999u64.to_le_bytes());
    mm::user_copy::allow(name.as_ptr() as u64, name.len() * 2, false);
    mm::user_copy::allow(raw_name.as_ptr() as u64, 16, false);
    mm::user_copy::allow(class.as_ptr() as u64, 80, false);
    let atom = call(nt_user_register_class_ex_wow, &[class.as_ptr() as u64, raw_name.as_ptr() as u64, 0, 0, 0, 0, 0]);
    assert_ne!(atom, 0);
    assert_eq!(call(nt_user_register_class_ex_wow, &[class.as_ptr() as u64, raw_name.as_ptr() as u64, 0, 0, 0, 0, 0]), 0);
    let lower: Vec<u16> = "classidentitytest".encode_utf16().collect();
    let hwnd = WINDOW_MANAGER.lock().create_window(&lower, &[], 0, 0, 0, 0, 40, 40, None, 0, 0).unwrap();
    assert_eq!(WINDOW_MANAGER.lock().get_window(hwnd).unwrap().extra.len(), 16);
    assert_eq!(call(nt_user_set_class_long_ptr, &[hwnd, (-16i32) as u32 as u64, 0x5678, 0]), 0x9999);
    assert_eq!(call(nt_user_set_class_long_ptr, &[hwnd, 0, u64::MAX, 0]), 0);
    assert_eq!(call(nt_user_set_class_long_ptr, &[hwnd, 0, 99, 0]), u64::MAX);
    assert_eq!(call(nt_user_set_class_word, &[hwnd, 7, 0x1234]), 0);
    assert_eq!(call(nt_user_set_class_long_ptr, &[hwnd, 5, 1, 0]), 0);
    window::destroy_window_internal(hwnd);
}
