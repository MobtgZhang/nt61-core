//! Out-of-band desktop shortcuts. Never mix privileged shortcuts with text bytes.
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

static PENDING: AtomicU32 = AtomicU32::new(0);
static WINDOW_EVENTS: WindowEvents = WindowEvents(AtomicU64::new(0));

/// Fifteen four-bit entries plus a four-bit length. CAS avoids taking an IRQ
/// lock and preserves repeated shortcuts and their order between desktop polls.
struct WindowEvents(AtomicU64);
impl WindowEvents {
    fn push(&self, event: u32) {
        let code = u64::from(event.trailing_zeros() - 3);
        let mut previous = self.0.load(Ordering::Relaxed);
        loop {
            let length = previous & 15;
            // Bounded input: retain already queued events when saturated.
            if length == 15 { return; }
            let next = ((previous & !15) | (code << (4 + length * 4))) | (length + 1);
            match self.0.compare_exchange_weak(previous, next, Ordering::AcqRel, Ordering::Relaxed) {
                Ok(_) => return,
                Err(actual) => previous = actual,
            }
        }
    }
    fn pop(&self) -> u32 {
        let mut previous = self.0.load(Ordering::Acquire);
        loop {
            let length = previous & 15;
            if length == 0 { return 0; }
            let code = (previous >> 4) & 15;
            let next = ((previous >> 4) & !15) | (length - 1);
            match self.0.compare_exchange_weak(previous, next, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => return 1 << (code + 3),
                Err(actual) => previous = actual,
            }
        }
    }
}

/// HID usage and USB modifier bitmap; shared by PS/2, USB and virtio decoders.
pub fn intercept(usage: u8, modifiers: u8) -> bool {
    let ctrl = modifiers & 0x11 != 0;
    let alt = modifiers & 0x44 != 0;
    let shift = modifiers & 0x22 != 0;
    let gui = modifiers & 0x88 != 0;
    let event = match usage {
        0x4c if ctrl && alt => nt61_abi::session_bridge::SECURITY,
        0x3d if alt => nt61_abi::session_bridge::CLOSE,
        0x29 if ctrl && shift => nt61_abi::session_bridge::TASK_MANAGER,
        0x0f if gui => nt61_abi::session_bridge::LOCK,
        0x50 if gui && !ctrl && !alt && !shift => nt61_abi::session_bridge::WINDOW_LEFT,
        0x4f if gui && !ctrl && !alt && !shift => nt61_abi::session_bridge::WINDOW_RIGHT,
        0x52 if gui && !ctrl && !alt && !shift => nt61_abi::session_bridge::WINDOW_UP,
        0x51 if gui && !ctrl && !alt && !shift => nt61_abi::session_bridge::WINDOW_DOWN,
        _ => return false,
    };
    if event >= nt61_abi::session_bridge::WINDOW_LEFT {
        WINDOW_EVENTS.push(event);
    } else {
        PENDING.fetch_or(event, Ordering::Release);
    }
    true
}

pub fn take() -> u32 {
    let privileged = PENDING.swap(0, Ordering::AcqRel);
    if privileged != 0 {
        WINDOW_EVENTS.0.store(0, Ordering::Release);
        privileged
    } else {
        WINDOW_EVENTS.pop()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nt61_abi::session_bridge as key;

    #[test]
    fn preserves_repeats_order_and_bounded_overflow() {
        let queue = WindowEvents(AtomicU64::new(0));
        let events = [key::WINDOW_RIGHT, key::WINDOW_LEFT, key::WINDOW_LEFT,
            key::WINDOW_DOWN, key::WINDOW_UP];
        for event in events { queue.push(event); }
        for event in events { assert_eq!(queue.pop(), event); }
        assert_eq!(queue.pop(), 0);
        for _ in 0..20 { queue.push(key::WINDOW_LEFT); }
        for _ in 0..15 { assert_eq!(queue.pop(), key::WINDOW_LEFT); }
        assert_eq!(queue.pop(), 0);
    }

    #[test]
    fn hid_shortcuts_and_security_priority() {
        assert!(!intercept(0x50, 0));
        assert!(!intercept(0x50, 8 | 2));
        assert!(intercept(0x50, 8));
        assert!(intercept(0x4f, 128));
        assert_eq!(take(), key::WINDOW_LEFT);
        assert_eq!(take(), key::WINDOW_RIGHT);
        assert_eq!(take(), 0);
        assert!(intercept(0x52, 8));
        assert!(intercept(0x4c, 1 | 4));
        assert_eq!(take(), key::SECURITY);
        assert_eq!(take(), 0);
    }
}
