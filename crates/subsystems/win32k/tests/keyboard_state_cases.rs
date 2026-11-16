use super::{create_window, Identity, SERVICES};
use crate::input_services::keyboard::*;
use crate::window::{WindowStyle, WINDOW_MANAGER};
use crate::{call, input_services, message, raw_input, user32_input, user_pointer, window};

fn state() -> [u8; 256] {
    let mut result = [0xaa; 256];
    let pointer = user_pointer(&mut result, true);
    assert_eq!(call(nt_user_get_keyboard_state, &[pointer]), 1);
    result
}

fn set_state(bytes: &[u8; 256]) {
    let pointer = user_pointer(bytes, false);
    // SAFETY: the fixture registered this complete readable array.
    assert_eq!(
        unsafe { user32_input::set_keyboard_state(pointer as *const u8) },
        1
    );
}

#[test]
fn keyboard_tables_capture_all_keys_mask_reserved_bits_and_remain_thread_local() {
    let _serial = SERVICES.lock().unwrap();
    let _identity = Identity::enter(960, 960);
    let bytes = core::array::from_fn(|key| key as u8);
    set_state(&bytes);
    assert_eq!(state(), bytes.map(|byte| byte & 0x81));
    for key in 0..256 {
        let expected = i16::from((bytes[key] & 0x81) as i8);
        assert_eq!(user32_input::get_key_state(key as i32), expected);
    }
    // Native GetKeyState accepts one argument. An unused second slot must not
    // choose a different key, as it did in the former placeholder handler.
    assert_eq!(call(nt_user_get_key_state, &[0x81, 0]), 0xff81);
    {
        let _other = Identity::enter(960, 1960);
        assert_eq!(state(), [0; 256]);
        set_state(&[0x80; 256]);
        assert_eq!(state(), [0x80; 256]);
        message::release_thread(1960);
    }
    assert_eq!(state(), bytes.map(|byte| byte & 0x81));
    assert_eq!(call(nt_user_set_keyboard_state, &[0]), 0);
    let short = [0u8; 255];
    assert_eq!(
        call(nt_user_set_keyboard_state, &[user_pointer(&short, false)]),
        0
    );
    assert_eq!(state(), bytes.map(|byte| byte & 0x81));
    let mut readonly = [0xaa; 256];
    let readonly_pointer = user_pointer(&mut readonly, false);
    assert_eq!(call(nt_user_get_keyboard_state, &[readonly_pointer]), 0);
    assert_eq!(readonly, [0xaa; 256]);
    message::release_thread(960);
}

#[test]
fn async_state_tracks_transitions_repeat_modifiers_and_physical_mouse_buttons() {
    let _serial = SERVICES.lock().unwrap();
    let _identity = Identity::enter(961, 961);
    for (scan, extended) in [(0x1e, false), (0x2a, false), (0x36, false), (0x1d, true)] {
        capture_scan(scan, false, extended).unwrap();
    }
    for key in [0x41, 0x10, 0xa0, 0xa1, 0x11, 0xa3] {
        async_key_state(key);
    }
    capture_scan(0x1e, true, false).unwrap();
    assert_eq!(user32_input::get_async_key_state(0x41) as u16, 0x8001);
    assert_eq!(async_key_state(0x41) as u16, 0x8000);
    capture_scan(0x1e, true, false).unwrap(); // autorepeat is not another press
    assert_eq!(async_key_state(0x41) as u16, 0x8000);
    capture_scan(0x1e, false, false).unwrap();
    assert_eq!(async_key_state(0x41), 0);
    capture_scan(0x1e, true, false).unwrap();
    capture_scan(0x1e, false, false).unwrap();
    assert_eq!(async_key_state(0x41), 1); // short press survives release
    assert_eq!(async_key_state(0x41), 0);

    capture_scan(0x2a, true, false).unwrap();
    capture_scan(0x36, true, false).unwrap();
    assert_eq!(async_key_state(0x10) as u16, 0x8001);
    assert_eq!(async_key_state(0xa0) as u16, 0x8001);
    assert_eq!(async_key_state(0xa1) as u16, 0x8001);
    capture_scan(0x2a, false, false).unwrap();
    assert_eq!(async_key_state(0x10) as u16, 0x8000);
    assert_eq!(async_key_state(0xa0), 0);
    capture_scan(0x36, false, false).unwrap();
    assert_eq!(async_key_state(0x10), 0);
    capture_scan(0x1d, true, true).unwrap();
    assert_eq!(async_key_state(0xa3) as u16, 0x8001);
    assert_eq!(async_key_state(0x11) as u16, 0x8001);
    capture_scan(0x1d, false, true).unwrap();

    capture_buttons(0);
    for key in [1, 2, 4, 5, 6] {
        async_key_state(key);
    }
    capture_buttons(0b10101);
    for key in [1, 4, 6] {
        assert_eq!(async_key_state(key) as u16, 0x8001);
    }
    for key in [2, 5] {
        assert_eq!(async_key_state(key), 0);
    }
    capture_buttons(0);
    for key in [1, 2, 4, 5, 6] {
        assert_eq!(async_key_state(key), 0);
    }
    for key in [-1, 256, i32::MIN, i32::MAX] {
        assert_eq!(async_key_state(key), 0);
    }
    // Changing this thread's keyboard table cannot simulate a hardware press.
    set_state(&[0x81; 256]);
    assert_eq!(async_key_state(0x41), 0);
    message::release_thread(961);
}

#[test]
fn locks_toggle_once_per_make_and_keypad_break_preserves_its_original_key() {
    let _serial = SERVICES.lock().unwrap();
    for (scan, key) in [(0x3a, 0x14), (0x45, 0x90), (0x46, 0x91)] {
        let before = capture_scan(scan, false, false).unwrap().snapshot.key(key) & 1;
        let first = capture_scan(scan, true, false).unwrap();
        assert_eq!(first.snapshot.key(key), 0x80 | (before ^ 1));
        assert_eq!(
            capture_scan(scan, true, false).unwrap().snapshot.key(key),
            0x80 | (before ^ 1)
        );
        assert_eq!(
            capture_scan(scan, false, false).unwrap().snapshot.key(key),
            before ^ 1
        );
        capture_scan(scan, true, false).unwrap();
        assert_eq!(
            capture_scan(scan, false, false).unwrap().snapshot.key(key),
            before
        );
    }
    capture_scan(0x2a, false, false).unwrap();
    capture_scan(0x36, false, false).unwrap();
    let first = capture_scan(0x48, true, false).unwrap();
    capture_scan(0x2a, true, false).unwrap();
    let released = capture_scan(0x48, false, false).unwrap();
    assert_eq!(released.virtual_key, first.virtual_key);
    assert_eq!(released.snapshot.key(first.virtual_key as u8) & 0x80, 0);
    capture_scan(0x2a, false, false).unwrap();
}

fn focus_window() -> u64 {
    let hwnd = create_window(None, WindowStyle::VISIBLE, 0);
    let mut manager = WINDOW_MANAGER.lock();
    manager.set_foreground_window(hwnd);
    manager.set_focus(hwnd);
    hwnd
}

#[test]
fn only_removing_hardware_messages_advances_the_owner_queues_keyboard_snapshot() {
    let _serial = SERVICES.lock().unwrap();
    let _identity = Identity::enter(962, 962);
    raw_input::drain();
    let hwnd = focus_window();
    raw_input::publish_device(true);
    set_state(&[0; 256]);
    raw_input::capture_keyboard(0x2e, true); // C
    raw_input::capture_keyboard(0x2e, false);
    raw_input::drain();
    assert_eq!(user32_input::get_key_state(0x43), 0);
    assert_eq!(async_key_state(0x43), 1); // hardware is already up
    let first =
        message::with_current_queue(|queue| queue.peek_message(hwnd, 0x100, 0x105, false)).unwrap();
    assert_eq!((first.message, first.wparam), (0x100, 0x43));
    assert_eq!(user32_input::get_key_state(0x43), 0);
    {
        let _other = Identity::enter(962, 1962);
        assert_eq!(user32_input::get_key_state(0x43), 0);
        assert!(message::with_current_queue(|queue| queue.peek_message(0, 0, 0, true)).is_none());
        message::release_thread(1962);
    }
    message::with_current_queue(|queue| queue.get_message(hwnd, 0x100, 0x105, 0)).unwrap();
    assert_eq!(state()[0x43], 0x80);
    let released =
        message::with_current_queue(|queue| queue.peek_message(hwnd, 0x100, 0x105, true)).unwrap();
    assert_eq!((released.message, released.wparam), (0x101, 0x43));
    assert_eq!(released.lparam as u32 >> 30, 3);
    assert_eq!(state()[0x43], 0);
    assert!(message::post_message(hwnd, 0x100, 0x43, 1));
    message::with_current_queue(|queue| queue.peek_message(hwnd, 0, 0, true)).unwrap();
    assert_eq!(state()[0x43], 0); // posted WM_KEYDOWN has no hardware snapshot
    message::release_thread(962);
    assert!(window::destroy_window_internal(hwnd));
}

#[test]
fn keyboard_nolegacy_keeps_async_and_raw_input_and_blocking_discards_hardware_updates() {
    let _serial = SERVICES.lock().unwrap();
    let _identity = Identity::enter(963, 963);
    raw_input::drain();
    let hwnd = focus_window();
    raw_input::publish_device(true);
    let registration = raw_input::RawRegistration {
        page: 1,
        usage: 6,
        flags: 0x30,
        target: hwnd,
    };
    assert_eq!(
        call(
            raw_input::nt_user_register_raw_input_devices,
            &[user_pointer(&registration, false), 1, 16]
        ),
        1
    );
    raw_input::capture_keyboard(0x20, true); // D
    raw_input::drain();
    assert_eq!(async_key_state(0x44) as u16 & 0x8000, 0x8000);
    assert!(
        message::with_current_queue(|queue| queue.peek_message(hwnd, 0x100, 0x105, true)).is_none()
    );
    assert!(
        message::with_current_queue(|queue| queue.peek_message(hwnd, 0xff, 0xff, true)).is_some()
    );
    raw_input::capture_keyboard(0x20, false);
    assert_eq!(call(input_services::nt_user_block_input, &[1]), 1);
    raw_input::capture_keyboard(0x20, true);
    assert_eq!(async_key_state(0x44), 0);
    assert_eq!(call(input_services::nt_user_block_input, &[0]), 1);
    raw_input::drain();
    raw_input::release_process(963);
    message::release_thread(963);
    assert!(window::destroy_window_internal(hwnd));
}

#[test]
fn alt_context_and_previous_state_are_captured_before_each_transition() {
    let _serial = SERVICES.lock().unwrap();
    let _identity = Identity::enter(964, 964);
    raw_input::drain();
    let hwnd = focus_window();
    raw_input::publish_device(true);
    capture_scan(0x38, false, false).unwrap();
    raw_input::capture_keyboard(0x38, true);
    raw_input::capture_keyboard(0x38, true);
    raw_input::capture_keyboard(0x38, false);
    raw_input::drain();
    for (message, context, previous) in [
        (0x104, false, false),
        (0x104, true, true),
        (0x105, true, true),
    ] {
        let event =
            message::with_current_queue(|queue| queue.peek_message(hwnd, 0x100, 0x105, true))
                .unwrap();
        assert_eq!(event.message, message);
        assert_eq!(event.wparam, 0x12);
        assert_eq!(event.lparam as u32 & (1 << 29) != 0, context);
        assert_eq!(event.lparam as u32 & (1 << 30) != 0, previous);
    }
    message::release_thread(964);
    assert!(window::destroy_window_internal(hwnd));
}
