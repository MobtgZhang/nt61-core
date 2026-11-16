use super::{create_window, Identity, SERVICES};
use crate::message::{self, services::*, Msg};
use crate::{
    call, user_pointer,
    window::{WindowStyle, WINDOW_MANAGER},
};

fn empty_message() -> Msg {
    Msg {
        hwnd: 0,
        message: 0,
        wparam: 0,
        lparam: 0,
        time: 0,
        pt_x: 0,
        pt_y: 0,
    }
}

fn peek(hwnd: u64, min: u32, max: u32, remove: bool) -> Option<Msg> {
    let mut result = empty_message();
    let pointer = user_pointer(&mut result, true);
    (call(
        nt_user_peek_message,
        &[
            pointer,
            hwnd,
            u64::from(min),
            u64::from(max),
            u64::from(remove),
        ],
    ) != 0)
        .then_some(result)
}

#[test]
fn null_window_posts_are_retrievable_and_thread_queues_do_not_collide() {
    let _serial = SERVICES.lock().unwrap();
    let _identity = Identity::enter(980, 980);
    message::release_thread(980);
    message::release_thread(1044); // Same remainder under the old user32 table.
    assert_eq!(call(nt_user_post_message, &[0, 0x400, 0x1234, u64::MAX]), 1);
    assert_eq!(call(nt_user_post_thread_message, &[1044, 0x401, 9, 10]), 0);
    {
        let _other = Identity::enter(980, 1044);
        assert!(peek(0, 0, 0, true).is_none());
    }
    assert_eq!(call(nt_user_post_thread_message, &[1044, 0, 9, 10]), 1); // WM_NULL is valid.
    {
        let _other = Identity::enter(980, 1044);
        let result = peek(u64::MAX, 0, 0, true).unwrap();
        assert_eq!(
            (result.hwnd, result.message, result.wparam, result.lparam),
            (0, 0, 9, 10)
        );
        assert!(peek(0, 0, 0, true).is_none());
    }
    let result = peek(0, 0, 0, true).unwrap();
    assert_eq!(
        (result.message, result.wparam, result.lparam),
        (0x400, 0x1234, -1)
    );
    assert!(peek(0, 0, 0, true).is_none());
    message::release_thread(980);
    message::release_thread(1044);
    assert_eq!(call(nt_user_post_thread_message, &[1044, 0x400, 0, 0]), 0);
    assert_eq!(call(nt_user_post_thread_message, &[0, 0x400, 0, 0]), 0);
}

#[test]
fn quit_bypasses_filters_is_coalesced_and_is_removed_only_on_request() {
    let _serial = SERVICES.lock().unwrap();
    let _identity = Identity::enter(981, 981);
    assert_eq!(call(nt_user_post_message, &[0, 0x400, 12, 0]), 1);
    message::post_quit_message(3);
    message::post_quit_message(-7);
    assert_eq!(peek(0, 0, 0, true).unwrap().message, 0x400);
    for _ in 0..2 {
        let quit = peek(u64::MAX, 0x500, 0x600, false).unwrap();
        assert_eq!((quit.message, quit.wparam), (0x12, (-7i32) as u32 as u64));
    }
    let mut result = empty_message();
    let pointer = user_pointer(&mut result, true);
    assert_eq!(
        call(nt_user_get_message, &[pointer, u64::MAX, 0x500, 0x600]),
        0
    );
    assert_eq!(result.message, 0x12);
    assert!(peek(0, 0, 0, false).is_none());
    assert!(!message::is_quit_posted());
    // An explicitly posted WM_QUIT must also bypass numeric filtering.
    assert_eq!(call(nt_user_post_message, &[0, 0x12, 4, 0]), 1);
    assert_eq!(peek(0, 0x500, 0x600, true).unwrap().wparam, 4);
    message::with_current_queue(|queue| {
        queue.post_quit(8);
        queue.clear();
        assert!(!queue.peek_has_messages());
        assert!(!queue.is_message_available());
    });
    message::release_thread(981);
}

#[test]
fn window_filters_include_descendants_and_thread_only_filter_excludes_windows() {
    let _serial = SERVICES.lock().unwrap();
    let _identity = Identity::enter(982, 982);
    let parent = create_window(None, WindowStyle::VISIBLE, 0);
    let child = create_window(Some(parent), WindowStyle::CHILD, 0);
    let grandchild = create_window(Some(child), WindowStyle::CHILD, 0);
    let other = create_window(None, 0, 0);
    for hwnd in [other, grandchild, 0] {
        assert_eq!(call(nt_user_post_message, &[hwnd, 0x400, hwnd, 0]), 1);
    }
    assert_eq!(peek(parent, 0x400, 0x400, false).unwrap().hwnd, grandchild);
    assert_eq!(peek(parent, 0x400, 0x400, true).unwrap().hwnd, grandchild);
    assert_eq!(peek(u64::MAX, 0, 0, true).unwrap().hwnd, 0);
    assert_eq!(peek(0, 0, 0, true).unwrap().hwnd, other);
    assert!(peek(0, 0, 0, true).is_none());
    assert!(WINDOW_MANAGER.lock().destroy_window(other));
    assert!(WINDOW_MANAGER.lock().destroy_window(parent));
    assert!(peek(parent, 0, 0, false).is_none());
    message::release_thread(982);
}

#[test]
fn invalid_output_buffers_do_not_consume_messages_and_get_message_reports_minus_one() {
    let _serial = SERVICES.lock().unwrap();
    let _identity = Identity::enter(983, 983);
    assert_eq!(call(nt_user_post_message, &[0, 0x400, 5, 0]), 1);
    let readonly = empty_message();
    let pointer = user_pointer(&readonly, false);
    assert_eq!(call(nt_user_peek_message, &[pointer, 0, 0, 0, 1]), 0);
    assert_eq!(call(nt_user_get_message, &[pointer, 0, 0, 0]), u64::MAX);
    assert_eq!(call(nt_user_get_message, &[0, 0, 0, 0]), u64::MAX);
    assert_eq!(peek(0, 0, 0, true).unwrap().wparam, 5);
    message::release_thread(983);
}

#[test]
fn get_message_waits_without_holding_the_producer_lock() {
    use std::{sync::mpsc, time::Duration};
    let _serial = SERVICES.lock().unwrap();
    let (ready_tx, ready_rx) = mpsc::channel();
    let (result_tx, result_rx) = mpsc::channel();
    let receiver = std::thread::spawn(move || {
        let _identity = Identity::enter(984, 984);
        message::with_current_queue(|queue| queue.clear());
        ready_tx.send(()).unwrap();
        let mut result = empty_message();
        let pointer = user_pointer(&mut result, true);
        let status = call(nt_user_get_message, &[pointer, 0, 0, 0]);
        result_tx
            .send((status, result.message, result.wparam))
            .unwrap();
        message::release_thread(984);
    });
    ready_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(matches!(
        result_rx.recv_timeout(Duration::from_millis(20)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    assert_eq!(call(nt_user_post_thread_message, &[984, 0x405, 77, 0]), 1);
    assert_eq!(
        result_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
        (1, 0x405, 77)
    );
    receiver.join().unwrap();
}

#[test]
fn rust_user32_translation_uses_the_same_layout_and_preserves_the_original_message() {
    use crate::user32_message_fixture::message as api;
    let _serial = SERVICES.lock().unwrap();
    let _identity = Identity::enter(985, 985);
    let mut state = [0u8; 256];
    state[0x10] = 0x80;
    let pointer = user_pointer(&state, false);
    assert_eq!(
        call(
            crate::input_services::keyboard::nt_user_set_keyboard_state,
            &[pointer]
        ),
        1
    );
    let mut key = api::MSG {
        hwnd: core::ptr::null_mut(),
        message: 0x104,
        wparam: 0x31,
        lparam: 0x0002_0003,
        time: 65,
        pt: api::POINT { x: 9, y: 10 },
    };
    let mut translated = api::MSG::new();
    // SAFETY: all MSG pointers below remain alive and appropriately writable.
    unsafe {
        assert_eq!(api::translate_message(&key), 1);
        assert_eq!(
            api::peek_message_w(&mut translated, core::ptr::null_mut(), 0x106, 0x106, 1),
            1
        );
        assert_eq!(
            (
                translated.message,
                translated.wparam,
                translated.lparam,
                translated.time
            ),
            (0x106, b'!' as usize, key.lparam, 65)
        );
        assert_eq!((translated.pt.x, translated.pt.y), (9, 10));
        key.message = 0x101;
        assert_eq!(api::translate_message(&key), 1);
        assert_eq!(
            api::peek_message_w(&mut translated, core::ptr::null_mut(), 0, 0, 1),
            0
        );
    }
    message::release_thread(985);
}
