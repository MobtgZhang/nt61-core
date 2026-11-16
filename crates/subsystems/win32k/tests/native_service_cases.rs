//! Regression coverage for native USER/GDI additions. These execute production
//! handlers and queues with bounded user-memory probes from the host fixture.
use super::*;
use window::{native_state::*, session_services::*, WindowStyle, WINDOW_MANAGER};

#[path = "message_service_cases.rs"]
mod message_service_cases;

static SERVICES: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct Identity((u64, u64));
impl Identity {
    fn enter(process: u64, thread: u64) -> Self {
        Self(ps::thread::IDENTITY.with(|id| id.replace((process, thread))))
    }
}
impl Drop for Identity {
    fn drop(&mut self) {
        ps::thread::IDENTITY.with(|id| id.set(self.0));
    }
}
fn create_window(parent: Option<u64>, style: u32, extended: u32) -> u64 {
    WINDOW_MANAGER
        .lock()
        .create_window(&[], &[], style, extended, 0, 0, 120, 80, parent, 0, 0)
        .unwrap()
}
fn peek(remove: bool) -> Option<message::Msg> {
    message::with_current_queue(|queue| queue.peek_message(0, 0xff, 0xff, remove))
}

#[test]
fn color_spaces_retain_saved_dc_references_and_enforce_process_ownership() {
    use color_space::*;
    let _serial = SERVICES.lock().unwrap();
    let _identity = Identity::enter(901, 901);
    let mut descriptor = [0u32; 148];
    descriptor[..5].copy_from_slice(&[0x50534f43, 0x400, 588, 0x73524742, 4]);
    let pointer = user_pointer(&descriptor, false);
    let first = call(nt_gdi_create_color_space, &[pointer]);
    let second = call(nt_gdi_create_color_space, &[pointer]);
    assert_ne!(first, 0);
    let hdc = dc::gre_create_display_dc();
    assert_eq!(call(nt_gdi_set_color_space, &[hdc, first]), 1);
    assert_eq!(dc::gre_save_dc(hdc), 1);
    assert_eq!(call(nt_gdi_set_color_space, &[hdc, second]), 1);
    assert_eq!(call(nt_gdi_delete_color_space, &[first]), 0);
    {
        let _foreign = Identity::enter(902, 902);
        assert_eq!(call(nt_gdi_set_color_space, &[hdc, second]), 0);
        assert_eq!(call(nt_gdi_delete_color_space, &[second]), 0);
        assert!(objects::ObjectReference::acquire(first).is_none());
    }
    assert!(dc::gre_restore_dc(hdc, -1));
    assert_eq!(call(nt_gdi_delete_color_space, &[second]), 1);
    assert_eq!(call(nt_gdi_delete_color_space, &[first]), 0);
    assert_eq!(call(nt_gdi_set_icm_mode, &[hdc, 1, 0]), 1);
    assert_eq!(call(nt_gdi_set_icm_mode, &[hdc, 1, 0x100]), 0);
    assert!(dc::gre_delete_dc(hdc));
    assert_eq!(call(nt_gdi_delete_color_space, &[first]), 1);
    assert_eq!(call(nt_gdi_delete_color_space, &[first]), 0);
    let brush = objects::gdi_create_solid_brush(0);
    assert_eq!(call(nt_gdi_delete_color_space, &[brush]), 0);
    assert!(objects::gdi_delete_object(brush));
    descriptor[147] = 1;
    let pointer = user_pointer(&mut descriptor, false);
    assert_eq!(call(nt_gdi_create_color_space, &[pointer]), 0);
}

#[test]
fn pixel_formats_validate_memory_dcs_descriptors_and_single_assignment() {
    use dc::pixel_format::*;
    let _serial = SERVICES.lock().unwrap();
    let _identity = Identity::enter(903, 903);
    let hdc = dc::gre_create_display_dc();
    let memory = dc::gre_create_compatible_dc(0);
    let mut descriptor = [0xaau8; 40];
    let pointer = user_pointer(&mut descriptor, true);
    assert_eq!(call(nt_gdi_describe_pixel_format, &[hdc, 0, 0, 0]), 1);
    assert_eq!(
        call(nt_gdi_describe_pixel_format, &[hdc, 1, 39, pointer]),
        0
    );
    assert_eq!(descriptor, [0xaa; 40]);
    assert_eq!(
        call(nt_gdi_describe_pixel_format, &[hdc, 1, 40, pointer]),
        1
    );
    assert_eq!(
        u32::from_le_bytes(descriptor[4..8].try_into().unwrap()),
        0x70
    );
    assert_eq!(descriptor[9], 32);
    assert_eq!(call(nt_gdi_swap_buffers, &[hdc]), 0);
    assert_eq!(call(nt_gdi_set_pixel_format, &[memory, 1]), 0);
    assert_eq!(call(nt_gdi_set_pixel_format, &[hdc, 2]), 0);
    assert_eq!(call(nt_gdi_set_pixel_format, &[hdc, 1]), 1);
    assert_eq!(call(nt_gdi_set_pixel_format, &[hdc, 1]), 0);
    assert_eq!(call(nt_gdi_swap_buffers, &[hdc]), 1);
    assert!(dc::gre_delete_dc(memory));
    assert!(dc::gre_delete_dc(hdc));
}

#[test]
fn converted_stock_handles_reject_old_aliases_foreign_processes_and_selected_objects() {
    use objects::attributes::*;
    let _serial = SERVICES.lock().unwrap();
    let _identity = Identity::enter(904, 904);
    let brush = objects::gdi_create_solid_brush(0x123456);
    let reference = objects::ObjectReference::acquire(brush).unwrap();
    assert_eq!(call(nt_gdi_set_brush_attributes, &[brush, 1]), 0);
    drop(reference);
    let stock = call(nt_gdi_set_brush_attributes, &[brush, 1]);
    assert_eq!(stock, brush | 0x800000);
    assert_eq!(objects::gdi_get_object_type(brush), None);
    assert!(objects::gdi_delete_object(stock));
    assert!(objects::gdi_get_object_type(stock).is_some());
    {
        let _foreign = Identity::enter(905, 905);
        assert_eq!(call(nt_gdi_clear_brush_attributes, &[stock, 1]), 0);
    }
    assert_eq!(call(nt_gdi_clear_brush_attributes, &[stock, 1]), brush);
    assert_eq!(objects::gdi_get_object_type(stock), None);
    assert!(objects::gdi_delete_object(brush));
}

#[test]
fn window_touch_and_layer_attributes_validate_native_argument_order_and_ownership() {
    let _serial = SERVICES.lock().unwrap();
    let _identity = Identity::enter(906, 906);
    let hwnd = create_window(None, WindowStyle::VISIBLE, 0x80000);
    assert_eq!(
        call(nt_user_modify_window_touch_capability, &[hwnd, 3, 1]),
        1
    );
    let mut flags = 0u32;
    let flags_pointer = user_pointer(&mut flags, true);
    assert_eq!(call(nt_user_is_touch_window, &[hwnd, flags_pointer]), 1);
    assert_eq!(flags, 3);
    assert_eq!(
        call(nt_user_modify_window_touch_capability, &[hwnd, 3, 2]),
        0
    );
    assert_eq!(
        call(nt_user_modify_window_touch_capability, &[hwnd, 0, 0]),
        1
    );
    assert_eq!(call(nt_user_is_touch_window, &[hwnd, flags_pointer]), 0);
    assert_eq!(
        call(
            nt_user_set_layered_window_attributes,
            &[hwnd, 0x123456, 127, 3]
        ),
        1
    );
    let mut color = 0u32;
    let mut alpha = 0u8;
    let color_pointer = user_pointer(&mut color, true);
    let alpha_pointer = user_pointer(&mut alpha, true);
    assert_eq!(
        call(
            nt_user_get_layered_window_attributes,
            &[hwnd, color_pointer, alpha_pointer, flags_pointer]
        ),
        1
    );
    assert_eq!((color, alpha, flags), (0x123456, 127, 3));
    {
        let _foreign = Identity::enter(907, 907);
        assert_eq!(
            call(nt_user_set_layered_window_attributes, &[hwnd, 0, 0, 3]),
            0
        );
    }
    let child = create_window(Some(hwnd), WindowStyle::CHILD, 0x80000);
    assert_eq!(
        call(nt_user_set_layered_window_attributes, &[child, 0, 0, 3]),
        0
    );
    assert_eq!(
        call(nt_user_get_window_display_affinity, &[child, flags_pointer]),
        0
    );
    assert!(window::destroy_window_internal(hwnd));
    assert_eq!(
        call(nt_user_get_layered_window_attributes, &[hwnd, 0, 0, 0]),
        0
    );
}

#[test]
fn window_region_transfer_rejects_selected_objects_and_preserves_old_geometry() {
    let _serial = SERVICES.lock().unwrap();
    let _identity = Identity::enter(908, 908);
    let hwnd = create_window(None, 0, 0);
    let region = region::gre_create_rect_rgn(1, 2, 30, 40);
    let output = region::gre_create_rect_rgn(0, 0, 0, 0);
    let reference = objects::ObjectReference::acquire(region).unwrap();
    assert_eq!(call(nt_user_set_window_rgn_ex, &[hwnd, region, 1]), 0);
    assert_eq!(call(nt_user_get_window_rgn_ex, &[hwnd, output, 0]), 0);
    drop(reference);
    assert_eq!(call(nt_user_set_window_rgn_ex, &[hwnd, region, 1]), 1);
    assert_eq!(objects::gdi_get_object_type(region), None);
    assert_eq!(call(nt_user_get_window_rgn_ex, &[hwnd, output, 0]), 2);
    assert_eq!(bounds(output), (2, Rect::new(1, 2, 30, 40)));
    assert_eq!(call(nt_user_set_window_rgn_ex, &[hwnd, region, 1]), 0);
    assert_eq!(call(nt_user_get_window_rgn_ex, &[hwnd, output, 0]), 2);
    assert!(objects::gdi_delete_object(output));
    assert!(window::destroy_window_internal(hwnd));
}

#[test]
fn raw_input_routes_to_target_thread_buffers_fifo_and_retires_handles() {
    use raw_input::*;
    let _serial = SERVICES.lock().unwrap();
    let _identity = Identity::enter(910, 910);
    let hwnd = create_window(None, 0, 0);
    let registrations = [
        RawRegistration {
            page: 1,
            usage: 2,
            flags: 0x100,
            target: hwnd,
        },
        RawRegistration {
            page: 1,
            usage: 6,
            flags: 0x100,
            target: hwnd,
        },
    ];
    assert_eq!(
        call(
            nt_user_register_raw_input_devices,
            &[user_pointer(&registrations, false), 2, 16]
        ),
        1
    );
    publish_device(false);
    publish_device(true);
    capture_mouse(12, -7, 1, 1);
    capture_keyboard_extended(0x48, true, true);
    {
        let _other_thread = Identity::enter(910, 911);
        drain();
        assert!(peek(false).is_none());
    }
    let first = peek(false).unwrap();
    assert_eq!(first.hwnd, hwnd);
    let mut capacity = 0u32;
    let capacity_pointer = user_pointer(&mut capacity, true);
    assert_eq!(
        call(nt_user_get_raw_input_buffer, &[0, capacity_pointer, 24]),
        0
    );
    assert_eq!(capacity, 48); // first packet size, not sum of packets
    let mut bytes = [0xaau8; 96];
    let pointer = user_pointer(&mut bytes, true);
    capacity = 47;
    assert_eq!(
        call(
            nt_user_get_raw_input_buffer,
            &[pointer, capacity_pointer, 24]
        ),
        u32::MAX as u64
    );
    assert_eq!(capacity, 48);
    assert_eq!(bytes, [0xaa; 96]);
    capacity = 96;
    let capacity_pointer = user_pointer(&mut capacity, true);
    assert_eq!(
        call(
            nt_user_get_raw_input_buffer,
            &[pointer, capacity_pointer, 24]
        ),
        2
    );
    assert!(peek(false).is_none());
    assert_eq!(i32::from_le_bytes(bytes[36..40].try_into().unwrap()), 12);
    assert_eq!(i32::from_le_bytes(bytes[40..44].try_into().unwrap()), -7);
    assert_eq!(u16::from_le_bytes(bytes[30..32].try_into().unwrap()), 120);
    assert_eq!(u32::from_le_bytes(bytes[48..52].try_into().unwrap()), 1);
    assert_eq!(u16::from_le_bytes(bytes[74..76].try_into().unwrap()), 2); // E0
    assert_eq!(u16::from_le_bytes(bytes[78..80].try_into().unwrap()), 0x26); // VK_UP
    assert_eq!(
        call(
            nt_user_get_raw_input_data,
            &[first.lparam as u64, 0x10000003, 0, capacity_pointer, 24]
        ),
        u32::MAX as u64
    );
    capture_pointer(65535, 32768, true, 0, 0);
    drain();
    let packet = peek(true).unwrap();
    dequeued(packet.lparam as u64);
    capacity = 96;
    let capacity_pointer = user_pointer(&mut capacity, true);
    assert_eq!(
        call(
            nt_user_get_raw_input_data,
            &[
                packet.lparam as u64,
                0x10000003,
                pointer,
                capacity_pointer,
                24
            ]
        ),
        48
    );
    assert_eq!(u16::from_le_bytes(bytes[24..26].try_into().unwrap()), 1);
    {
        let _other_thread = Identity::enter(910, 911);
        assert_eq!(
            call(
                nt_user_get_raw_input_data,
                &[
                    packet.lparam as u64,
                    0x10000003,
                    pointer,
                    capacity_pointer,
                    24
                ]
            ),
            u32::MAX as u64
        );
    }
    message::default_wndproc(hwnd, 0xff, packet.wparam, packet.lparam);
    assert_eq!(
        call(
            nt_user_get_raw_input_data,
            &[
                packet.lparam as u64,
                0x10000003,
                pointer,
                capacity_pointer,
                24
            ]
        ),
        u32::MAX as u64
    );
    release_process(910);
    capacity = 99;
    assert_eq!(
        call(
            nt_user_get_registered_raw_input_devices,
            &[0, capacity_pointer, 16]
        ),
        0
    );
    assert_eq!(capacity, 0);
    message::release_thread(910);
    assert!(window::destroy_window_internal(hwnd));
}

#[test]
fn cursor_clipping_and_blocking_release_on_owner_thread_exit() {
    use input_services::*;
    let _serial = SERVICES.lock().unwrap();
    let _identity = Identity::enter(912, 912);
    let clip = Rect::new(10, 20, 100, 120);
    assert_eq!(call(nt_user_clip_cursor, &[user_pointer(&clip, false)]), 1);
    assert_eq!(position_hardware_cursor(300, -10), (99, 20));
    assert_eq!(call(nt_user_block_input, &[1]), 1);
    assert!(!hardware_allowed());
    {
        let _other = Identity::enter(913, 913);
        assert_eq!(call(nt_user_block_input, &[0]), 0);
        assert!(!set_cursor(15, 25));
    }
    release_thread(912);
    assert!(hardware_allowed());
    assert!(set_cursor(15, 25));
    let mut point = [0i32; 2];
    assert!(get_cursor(user_pointer(&mut point, true)));
    assert_eq!(point, [15, 25]);
    assert_eq!(call(nt_user_clip_cursor, &[0]), 1);
}

#[test]
fn display_config_reports_capacity_without_partial_writes_and_uses_native_layouts() {
    use display_config::*;
    let _serial = SERVICES.lock().unwrap();
    let mut path_count = 0u32;
    let mut mode_count = 0u32;
    let paths_pointer = user_pointer(&mut path_count, true);
    let modes_pointer = user_pointer(&mut mode_count, true);
    assert_eq!(
        call(
            nt_user_get_display_config_buffer_sizes,
            &[3, paths_pointer, modes_pointer]
        ),
        87
    );
    assert_eq!(
        call(
            nt_user_get_display_config_buffer_sizes,
            &[2, paths_pointer, modes_pointer]
        ),
        0
    );
    assert_eq!((path_count, mode_count), (1, 2));
    let mut paths = [0xaau8; 72];
    let mut modes = [0xaau8; 128];
    let path_buffer = user_pointer(&mut paths, true);
    let mode_buffer = user_pointer(&mut modes, true);
    mode_count = 1;
    assert_eq!(
        call(
            nt_user_query_display_config,
            &[2, paths_pointer, path_buffer, modes_pointer, mode_buffer, 0]
        ),
        122
    );
    assert_eq!(modes, [0xaa; 128]);
    assert_eq!(mode_count, 2);
    assert_eq!(
        call(
            nt_user_query_display_config,
            &[2, paths_pointer, path_buffer, modes_pointer, mode_buffer, 0]
        ),
        0
    );
    let word =
        |bytes: &[u8], offset| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
    assert_eq!(word(&paths, 32), 1); // target mode index
    assert_eq!(word(&paths, 68), 1); // active
    assert_eq!((word(&modes, 16), word(&modes, 20)), (1024, 768));
    assert_eq!((word(&modes, 104), word(&modes, 108)), (1024, 768));
    assert_eq!(word(&modes, 100), 1); // target VSync denominator
    assert_eq!(
        call(
            nt_user_query_display_config,
            &[2, paths_pointer, path_buffer, modes_pointer, mode_buffer, 1]
        ),
        87
    );
}

#[test]
fn desktop_handles_enforce_switch_access_process_ownership_and_close_lifetime() {
    let _serial = SERVICES.lock().unwrap();
    let _identity = Identity::enter(914, 914);
    assert_eq!(call(nt_user_get_thread_desktop, &[0]), 0);
    assert_eq!(call(nt_user_get_thread_desktop, &[914]), DEFAULT_DESKTOP);
    let read_only = call(nt_user_open_input_desktop, &[0, 0, 1]);
    let switch = call(nt_user_open_input_desktop, &[0, 0, 0x20000000]);
    assert_ne!(read_only, 0);
    assert_eq!(call(nt_user_switch_desktop, &[read_only, 0]), 0);
    assert_eq!(call(nt_user_switch_desktop, &[switch, 0]), 1);
    assert_eq!(call(nt_user_switch_desktop, &[switch, 1]), 0);
    {
        let _other = Identity::enter(915, 915);
        assert_eq!(call(nt_user_close_desktop, &[switch]), 0);
        assert_eq!(call(nt_user_set_thread_desktop, &[switch]), 0);
    }
    assert_eq!(call(nt_user_set_thread_desktop, &[switch]), 1);
    assert_eq!(call(nt_user_get_thread_desktop, &[914]), switch);
    assert_eq!(call(nt_user_close_desktop, &[switch]), 0);
    window::session_services::release_thread(914);
    assert_eq!(call(nt_user_get_thread_desktop, &[914]), DEFAULT_DESKTOP);
    assert_eq!(call(nt_user_close_desktop, &[switch]), 1);
    assert_eq!(call(nt_user_switch_desktop, &[switch, 0]), 0);
    assert_eq!(call(nt_user_close_desktop, &[switch]), 0);
    assert_eq!(call(nt_user_close_desktop, &[read_only]), 1);
    assert_eq!(call(nt_user_close_desktop, &[DEFAULT_DESKTOP]), 0);
}

#[test]
fn popup_position_honors_alignment_exclusion_and_rejects_unknown_flags() {
    let _serial = SERVICES.lock().unwrap();
    let anchor = [100i32, 100];
    let size = [40i32, 30];
    let exclude = Rect::new(90, 90, 150, 150);
    let mut output = Rect::default();
    let anchor_pointer = user_pointer(&anchor, false);
    let size_pointer = user_pointer(&size, false);
    let output_pointer = user_pointer(&mut output, true);
    assert_eq!(
        call(
            nt_user_calculate_popup_window_position,
            &[anchor_pointer, size_pointer, 8 | 0x20, 0, output_pointer]
        ),
        1
    );
    assert_eq!(output, Rect::new(60, 70, 100, 100));
    assert_eq!(
        call(
            nt_user_calculate_popup_window_position,
            &[
                anchor_pointer,
                size_pointer,
                0,
                user_pointer(&exclude, false),
                output_pointer
            ]
        ),
        1
    );
    assert_eq!(output, Rect::new(100, 150, 140, 180));
    let previous = output;
    assert_eq!(
        call(
            nt_user_calculate_popup_window_position,
            &[anchor_pointer, size_pointer, 0x80, 0, output_pointer]
        ),
        0
    );
    assert_eq!(output, previous);
}

#[path = "mouse_history_cases.rs"]
mod mouse_history_cases;

#[path = "keyboard_state_cases.rs"]
mod keyboard_state_cases;

#[path = "keyboard_layout_cases.rs"]
mod keyboard_layout_cases;
