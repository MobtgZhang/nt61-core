use super::{Identity, SERVICES};
use crate::input_services::{keyboard::nt_user_set_keyboard_state, layout::*};
use crate::{call, message, user32_input, user_pointer};

#[test]
fn scan_mapping_covers_native_hkl_slot_modifiers_keypad_and_extended_prefixes() {
    let _serial = SERVICES.lock().unwrap();
    for (key, scan) in [
        (0x41, 0x1e),
        (0x30, 0x0b),
        (0x10, 0x2a),
        (0xa1, 0x36),
        (0x11, 0x1d),
        (0xa3, 0xe01d),
        (0xa5, 0xe038),
        (0x6f, 0xe035),
        (0x70, 0x3b),
        (0x7b, 0x58),
        (0x90, 0x45),
        (0x13, 0xe11d),
        (0x03, 0xe046),
        (0x2c, 0x54),
        (0x26, 0x48),
        (0x68, 0x48),
    ] {
        assert_eq!(user32_input::map_virtual_key_w(key, 4), scan, "key {key:x}");
        assert_eq!(user32_input::map_virtual_key_w(key, 0), scan & 255);
    }
    for (scan, generic, specific) in [
        (0x2a, 0x10, 0xa0),
        (0x36, 0x10, 0xa1),
        (0xe01d, 0x11, 0xa3),
        (0xe038, 0x12, 0xa5),
        (0xe037, 0x2c, 0x2c),
        (0xe11d, 0x13, 0x13),
        (0xe046, 0x03, 0x03),
        (0xe048, 0x26, 0x26),
    ] {
        assert_eq!(user32_input::map_virtual_key_w(scan, 1), generic);
        assert_eq!(user32_input::map_virtual_key_w(scan, 3), specific);
    }
    for scan in [
        0x136,
        0x11d,
        0x21d,
        0xe036,
        0xe045,
        0xe145,
        0xe137,
        u32::MAX,
    ] {
        assert_eq!(user32_input::map_virtual_key_w(scan, 3), 0);
    }
    assert_eq!(
        call(nt_user_map_virtual_key_ex, &[0x41, 0, 0xdead, US_LAYOUT]),
        0x1e
    );
    assert_eq!(
        call(nt_user_map_virtual_key_ex, &[0x41, 0, US_LAYOUT, 0xdead]),
        0
    );
    assert_eq!(
        call(
            nt_user_map_virtual_key_ex,
            &[0xaaaa_0000_0000_0041, 0, 0, 0]
        ),
        0x1e
    );
    assert_eq!(user32_input::map_virtual_key_w(0x141, 0), 0);
    assert_eq!(user32_input::map_virtual_key_w(0x41, 5), 0);
}

#[test]
fn character_mapping_and_reverse_lookup_cover_every_us_printable_ascii_character() {
    let _serial = SERVICES.lock().unwrap();
    let rows = [
        (b"1234567890", b"!@#$%^&*()", b"1234567890".as_slice()),
        (
            b"-=[]\\;',./",
            b"_+{}|:\"<>?",
            &[0xbd, 0xbb, 0xdb, 0xdd, 0xdc, 0xba, 0xde, 0xbc, 0xbe, 0xbf],
        ),
    ];
    for (plain, shift, keys) in rows {
        for ((&plain, &shift), &key) in plain.iter().zip(shift).zip(keys) {
            assert_eq!(
                user32_input::map_virtual_key_w(u32::from(key), 2),
                u32::from(plain)
            );
            assert_eq!(
                user32_input::vk_key_scan_ex_w(u16::from(plain), US_LAYOUT),
                i16::from(key)
            );
            assert_eq!(
                user32_input::vk_key_scan_ex_w(u16::from(shift), US_LAYOUT),
                0x100 | i16::from(key)
            );
        }
    }
    for key in b'A'..=b'Z' {
        assert_eq!(
            user32_input::map_virtual_key_w(u32::from(key), 2),
            u32::from(key)
        );
        assert_eq!(
            user32_input::vk_key_scan_ex_w(u16::from(key + 32), US_LAYOUT),
            i16::from(key)
        );
        assert_eq!(
            user32_input::vk_key_scan_ex_w(u16::from(key), US_LAYOUT),
            0x100 | i16::from(key)
        );
    }
    assert_eq!(user32_input::vk_key_scan_ex_w(b'`' as u16, US_LAYOUT), 0xc0);
    assert_eq!(
        user32_input::vk_key_scan_ex_w(b'~' as u16, US_LAYOUT),
        0x1c0
    );
    assert_eq!(user32_input::vk_key_scan_ex_w(b' ' as u16, US_LAYOUT), 0x20);
    assert_eq!(user32_input::vk_key_scan_ex_w(0x4e2d, US_LAYOUT), -1);
    // The native result is a DWORD; user32 truncates it to SHORT.
    assert_eq!(
        call(nt_user_vk_key_scan_ex, &[0x4e2d, US_LAYOUT, 1]),
        u64::from(u32::MAX)
    );
    assert_eq!(user32_input::vk_key_scan_ex_w(b'A' as u16, 0xdead), -1);
    // bUseHKL=FALSE selects the calling thread's layout instead of slot 1.
    assert_eq!(
        call(nt_user_vk_key_scan_ex, &[b'a' as u64, 0xdead, 0]),
        0x41
    );
}

fn convert(key: u32, scan: u32, state: &[u8; 256], flags: u32) -> (i32, [u16; 3]) {
    let mut output = [0xcccc; 3];
    let input = user_pointer(state, false);
    let pointer = user_pointer(&mut output, true);
    // SAFETY: both complete buffers are registered with the fixture.
    let count = unsafe {
        user32_input::to_unicode_ex(
            key,
            scan,
            input as *const u8,
            pointer as *mut u16,
            3,
            flags,
            US_LAYOUT,
        )
    };
    (count, output)
}

#[test]
fn unicode_conversion_uses_shift_caps_control_and_explicit_key_state() {
    let _serial = SERVICES.lock().unwrap();
    let mut state = [0u8; 256];
    for shift in [false, true] {
        for caps in [false, true] {
            state[0x10] = if shift { 0x80 } else { 0 };
            state[0x14] = u8::from(caps);
            for key in b'A'..=b'Z' {
                let expected = u16::from(key + if shift ^ caps { 0 } else { 32 });
                assert_eq!(
                    convert(u32::from(key), 0x1e, &state, 0),
                    (1, [expected, 0xcccc, 0xcccc])
                );
            }
        }
    }
    state = [0; 256];
    state[0x11] = 0x80;
    for key in b'A'..=b'Z' {
        assert_eq!(
            convert(u32::from(key), 0, &state, 0).1[0],
            u16::from(key - b'A' + 1)
        );
    }
    assert_eq!(convert(0x32, 0, &state, 0), (1, [0, 0xcccc, 0xcccc]));
    assert_eq!(convert(0x0d, 0, &state, 0).1[0], 10);
    assert_eq!(convert(0x09, 0, &state, 0), (0, [0xcccc; 3]));
    state[0x10] = 0x80;
    assert_eq!(convert(0x36, 0, &state, 0).1[0], 30);
    assert_eq!(convert(0xbd, 0, &state, 0).1[0], 31);
    assert_eq!(convert(0x0d, 0, &state, 0).0, 0);
    state[0x12] = 0x80;
    assert_eq!(convert(0x41, 0, &state, 0).0, 0);
    state = [0; 256];
    assert_eq!(convert(0x41, 0x801e, &state, 0).0, 0);
    assert_eq!(convert(0x41, 0x801e, &state, 2).1[0], b'a' as u16);
    assert_eq!(convert(0x70, 0, &state, 0).0, 0);
}

#[test]
fn unicode_conversion_and_layout_list_probe_only_the_required_user_buffers() {
    let _serial = SERVICES.lock().unwrap();
    let state = [0; 256];
    let short = [0u8; 255];
    let input = user_pointer(&state, false);
    let mut output = [0xccccu16; 2];
    let pointer = user_pointer(&mut output, true);
    assert_eq!(
        call(nt_user_to_unicode_ex, &[0x41, 0, input, pointer, 0, 0, 0]),
        0
    );
    assert_eq!(
        call(
            nt_user_to_unicode_ex,
            &[0x41, 0, user_pointer(&short, false), pointer, 2, 0, 0]
        ),
        0
    );
    assert_eq!(output, [0xcccc; 2]);
    assert_eq!(
        call(nt_user_to_unicode_ex, &[0x41, 0, input, 1, 2, 0, 0]),
        0
    );
    assert_eq!(
        call(nt_user_to_unicode_ex, &[0x41, 0, input, pointer, 1, 0, 0]),
        1
    );
    assert_eq!(output, [b'a' as u16, 0xcccc]);
    let mut layouts = [0xaaaa_u64; 2];
    let pointer = user_pointer(&mut layouts, true);
    assert_eq!(call(nt_user_get_keyboard_layout_list, &[0, 1]), 1);
    assert_eq!(call(nt_user_get_keyboard_layout_list, &[2, 0]), 1);
    assert_eq!(call(nt_user_get_keyboard_layout_list, &[2, pointer]), 1);
    assert_eq!(layouts, [US_LAYOUT, 0xaaaa]);
    assert_eq!(
        call(nt_user_get_keyboard_layout_list, &[u64::MAX, pointer]),
        0
    );
    assert_eq!(call(nt_user_get_keyboard_layout_list, &[1, 1]), 0);
    assert_eq!(user32_input::get_keyboard_layout(0) as u64, US_LAYOUT);
}

#[test]
fn translate_message_preserves_repeat_and_scan_bits_and_uses_the_callers_queue() {
    let _serial = SERVICES.lock().unwrap();
    let _identity = Identity::enter(970, 970);
    let mut state = [0u8; 256];
    let pointer = user_pointer(&state, false);
    assert_eq!(call(nt_user_set_keyboard_state, &[pointer]), 1);
    let mut key = message::Msg {
        hwnd: 123,
        message: 0x100,
        wparam: 0x41,
        lparam: 0x001e_0003,
        time: 78,
        pt_x: 10,
        pt_y: 20,
    };
    let pointer = user_pointer(&key, false);
    assert_eq!(call(nt_user_translate_message, &[pointer, 0]), 1);
    let translated =
        message::with_current_queue(|queue| queue.peek_message(0, 0, 0, true)).unwrap();
    assert_eq!(
        (translated.message, translated.wparam),
        (0x102, b'a' as u64)
    );
    assert_eq!(
        (
            translated.lparam,
            translated.time,
            translated.pt_x,
            translated.pt_y
        ),
        (key.lparam, key.time, 10, 20)
    );
    state[0x10] = 0x80;
    assert_eq!(
        call(nt_user_set_keyboard_state, &[user_pointer(&state, false)]),
        1
    );
    key.message = 0x104;
    assert_eq!(
        call(nt_user_translate_message, &[user_pointer(&key, false), 0]),
        1
    );
    let translated =
        message::with_current_queue(|queue| queue.peek_message(0, 0, 0, true)).unwrap();
    assert_eq!(
        (translated.message, translated.wparam),
        (0x106, b'A' as u64)
    );
    for (kind, vk, result) in [(0x101, 0x41, 1), (0x100, 0x70, 1), (0x200, 0, 0)] {
        key.message = kind;
        key.wparam = vk;
        assert_eq!(
            call(nt_user_translate_message, &[user_pointer(&key, false), 0]),
            result
        );
        assert!(message::with_current_queue(|queue| queue.peek_message(0, 0, 0, true)).is_none());
    }
    key.message = 0x100;
    key.wparam = 0xe7;
    key.lparam = 0x4e2d_0001;
    assert_eq!(
        call(nt_user_translate_message, &[user_pointer(&key, false), 0]),
        1
    );
    let translated =
        message::with_current_queue(|queue| queue.peek_message(0, 0, 0, true)).unwrap();
    assert_eq!(translated.wparam, 0x4e2d);
    assert_eq!(call(nt_user_translate_message, &[0, 0]), 0);
    message::release_thread(970);
}
