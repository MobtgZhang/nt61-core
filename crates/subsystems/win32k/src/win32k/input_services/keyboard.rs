//! Keyboard state for the installed US set-1 layout. Hardware state is shared
//! by the interactive station; synchronous state belongs to a message queue.
//! Only hardware messages carry snapshots. Posting WM_KEYDOWN cannot forge one.

use super::INPUT;
use crate::arch::common::syscall_frame::SyscallFrame;
use crate::mm::user_copy::{copy_to_user, read_user_object};

/// Compact copy of the meaningful bits of all 256 virtual keys. Kept inline in
/// bounded hardware/message rings so interrupt producers never allocate.
#[derive(Clone, Copy)]
pub struct KeyboardSnapshot {
    down: [u8; 32],
    toggled: [u8; 32],
}

impl KeyboardSnapshot {
    pub const fn empty() -> Self {
        Self {
            down: [0; 32],
            toggled: [0; 32],
        }
    }

    pub fn from_bytes(bytes: &[u8; 256]) -> Self {
        let mut snapshot = Self::empty();
        for (key, &value) in bytes.iter().enumerate() {
            snapshot.down[key / 8] |= ((value >> 7) & 1) << (key % 8);
            snapshot.toggled[key / 8] |= (value & 1) << (key % 8);
        }
        snapshot
    }

    pub fn key(&self, key: u8) -> u8 {
        let index = usize::from(key);
        ((self.down[index / 8] >> (index % 8) & 1) << 7)
            | (self.toggled[index / 8] >> (index % 8) & 1)
    }

    pub fn bytes(&self) -> [u8; 256] {
        core::array::from_fn(|key| self.key(key as u8))
    }
}

pub(super) struct KeyboardState {
    bytes: [u8; 256],
    pressed: [bool; 256],
    // Remember the key chosen on make so releasing Shift/NumLock while a
    // keypad key is held cannot turn its break into a different virtual key.
    scan_keys: [u8; 256],
}

impl KeyboardState {
    pub(super) const fn new() -> Self {
        Self {
            bytes: [0; 256],
            pressed: [false; 256],
            scan_keys: [0; 256],
        }
    }

    fn update(&mut self, key: u8, down: bool) -> bool {
        let index = usize::from(key);
        let was_down = self.bytes[index] & 0x80 != 0;
        if down && !was_down {
            self.pressed[index] = true;
            // The toggle bit is only meaningful for lock keys.
            if matches!(key, 0x14 | 0x90 | 0x91) {
                self.bytes[index] ^= 1;
            }
        }
        self.bytes[index] = (self.bytes[index] & 1) | if down { 0x80 } else { 0 };
        was_down
    }

    fn modifier(&mut self, key: u8) {
        let (generic, left, right) = match key {
            0xa0 | 0xa1 => (0x10, 0xa0, 0xa1),
            0xa2 | 0xa3 => (0x11, 0xa2, 0xa3),
            0xa4 | 0xa5 => (0x12, 0xa4, 0xa5),
            _ => return,
        };
        let down = (self.bytes[left] | self.bytes[right]) & 0x80 != 0;
        self.update(generic, down);
    }
}

#[derive(Clone, Copy)]
pub struct KeyboardEvent {
    pub snapshot: KeyboardSnapshot,
    pub virtual_key: u16,
    pub previous: bool,
    pub system: bool,
    pub alt_context: bool,
}

/// Called once per class-driver event, before raw-input registration filtering.
/// E0 is supplied separately; E1 Pause sequences need a decoder in the port.
pub fn capture_scan(scan: u8, down: bool, extended: bool) -> Option<KeyboardEvent> {
    if scan == 0 || scan >= 0x80 {
        return None;
    }
    let mut input = INPUT.lock();
    if input.blocker.is_some() {
        return None;
    }
    let state = &mut input.keyboard;
    let index = usize::from(scan) + if extended { 128 } else { 0 };
    let key = if state.scan_keys[index] != 0 {
        state.scan_keys[index]
    } else {
        scan_to_key(scan, extended, &state.bytes)?
    };
    let alt_context = state.bytes[0x12] & 0x80 != 0;
    let previous = state.update(key, down);
    state.scan_keys[index] = if down { key } else { 0 };
    state.modifier(key);
    let virtual_key = match key {
        0xa0 | 0xa1 => 0x10,
        0xa2 | 0xa3 => 0x11,
        0xa4 | 0xa5 => 0x12,
        _ => u16::from(key),
    };
    let system =
        (alt_context && state.bytes[0x11] & 0x80 == 0) || matches!(virtual_key, 0x12 | 0x79);
    Some(KeyboardEvent {
        snapshot: KeyboardSnapshot::from_bytes(&state.bytes),
        virtual_key,
        previous,
        system,
        alt_context,
    })
}

/// Track physical button order independently of logical button swapping.
pub fn capture_buttons(buttons: u8) {
    let mut input = INPUT.lock();
    if input.blocker.is_some() {
        return;
    }
    for (bit, key) in [(0, 0x01), (1, 0x02), (2, 0x04), (3, 0x05), (4, 0x06)] {
        input.keyboard.update(key, buttons & (1 << bit) != 0);
    }
}

pub(super) fn scan_to_key(scan: u8, extended: bool, state: &[u8; 256]) -> Option<u8> {
    if extended {
        return Some(match scan {
            0x1c => 0x0d,
            0x1d => 0xa3,
            0x35 => 0x6f,
            0x37 => 0x2c,
            0x38 => 0xa5,
            0x47 => 0x24,
            0x48 => 0x26,
            0x49 => 0x21,
            0x4b => 0x25,
            0x4d => 0x27,
            0x4f => 0x23,
            0x50 => 0x28,
            0x51 => 0x22,
            0x52 => 0x2d,
            0x53 => 0x2e,
            0x5b => 0x5b,
            0x5c => 0x5c,
            0x5d => 0x5d,
            _ => return None,
        });
    }
    if (0x47..=0x53).contains(&scan) && !matches!(scan, 0x4a | 0x4e) {
        let number = (state[0x90] & 1 != 0) ^ (state[0x10] & 0x80 != 0);
        const NUMBERS: [u8; 13] = [
            0x67, 0x68, 0x69, 0, 0x64, 0x65, 0x66, 0, 0x61, 0x62, 0x63, 0x60, 0x6e,
        ];
        const NAVIGATION: [u8; 13] = [
            0x24, 0x26, 0x21, 0, 0x25, 0x0c, 0x27, 0, 0x23, 0x28, 0x22, 0x2d, 0x2e,
        ];
        return Some(if number { NUMBERS } else { NAVIGATION }[usize::from(scan - 0x47)]);
    }
    Some(match scan {
        1 => 0x1b,
        14 => 8,
        15 => 9,
        28 => 13,
        29 => 0xa2,
        42 => 0xa0,
        54 => 0xa1,
        55 => 0x6a,
        56 => 0xa4,
        57 => 0x20,
        58 => 0x14,
        59..=68 => scan + 53,
        69 => 0x90,
        70 => 0x91,
        74 => 0x6d,
        78 => 0x6b,
        86 => 0xe2,
        87 => 0x7a,
        88 => 0x7b,
        12 => 0xbd,
        13 => 0xbb,
        26 => 0xdb,
        27 => 0xdd,
        39 => 0xba,
        40 => 0xde,
        41 => 0xc0,
        43 => 0xdc,
        51 => 0xbc,
        52 => 0xbe,
        53 => 0xbf,
        _ => {
            const SCANS: &[u8] = b"\x02\x03\x04\x05\x06\x07\x08\x09\x0a\x0b\x10\x11\x12\x13\x14\x15\x16\x17\x18\x19\x1e\x1f\x20\x21\x22\x23\x24\x25\x26\x2c\x2d\x2e\x2f\x30\x31\x32";
            const KEYS: &[u8] = b"1234567890QWERTYUIOPASDFGHJKLZXCVBNM";
            KEYS[SCANS.iter().position(|&value| value == scan)?]
        }
    })
}

pub fn async_key_state(key: i32) -> i16 {
    let Some(key) = usize::try_from(key).ok().filter(|key| *key < 256) else {
        return 0;
    };
    let mut input = INPUT.lock();
    let down = input.keyboard.bytes[key] & 0x80 != 0;
    let pressed = core::mem::take(&mut input.keyboard.pressed[key]);
    ((u16::from(down) << 15) | u16::from(pressed)) as i16
}

pub extern "C" fn nt_user_get_async_key_state(frame: &SyscallFrame) -> u64 {
    u64::from(async_key_state(frame.args[0] as i32) as u16)
}

pub extern "C" fn nt_user_get_key_state(frame: &SyscallFrame) -> u64 {
    let key = frame.args[0] as u8;
    let byte = super::super::message::with_current_queue(|queue| queue.keyboard_state().key(key));
    // GetKeyState sign-extends the state byte to SHORT, unlike the async API.
    u64::from(i16::from(byte as i8) as u16)
}

pub extern "C" fn nt_user_get_keyboard_state(frame: &SyscallFrame) -> u64 {
    let bytes = super::super::message::with_current_queue(|queue| queue.keyboard_state().bytes());
    if copy_to_user(frame.args[0], &bytes).is_err() {
        super::set_last_error(998);
        return 0;
    }
    1
}

pub extern "C" fn nt_user_set_keyboard_state(frame: &SyscallFrame) -> u64 {
    let Ok(bytes) = read_user_object::<[u8; 256]>(frame.args[0]) else {
        super::set_last_error(998);
        return 0;
    };
    let snapshot = KeyboardSnapshot::from_bytes(&bytes);
    super::super::message::with_current_queue(|queue| queue.set_keyboard_state(snapshot));
    1
}
