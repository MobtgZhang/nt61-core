//! Completed pointer reports, retained while userspace paints a frame.
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

const CAPACITY: usize = 256;

/// A single producer (the serialized VirtIO poller) and a single desktop
/// consumer. Publishing a report must not overwrite an unread button edge.
pub struct PointerQueue {
    reports: [AtomicU64; CAPACITY],
    read: AtomicUsize,
    write: AtomicUsize,
}

impl PointerQueue {
    pub const fn new() -> Self {
        Self {
            reports: [const { AtomicU64::new(0) }; CAPACITY],
            read: AtomicUsize::new(0),
            write: AtomicUsize::new(0),
        }
    }

    pub fn push(&self, x: i16, y: i16, buttons: u8) -> bool {
        let write = self.write.load(Ordering::Relaxed);
        let next = (write + 1) % CAPACITY;
        if next == self.read.load(Ordering::Acquire) {
            return false;
        }
        let report = u64::from(x as u16) | (u64::from(y as u16) << 16) | (u64::from(buttons) << 32);
        self.reports[write].store(report, Ordering::Relaxed);
        self.write.store(next, Ordering::Release);
        true
    }

    pub fn pop(&self) -> Option<(i16, i16, u8)> {
        let read = self.read.load(Ordering::Relaxed);
        if read == self.write.load(Ordering::Acquire) {
            return None;
        }
        let report = self.reports[read].load(Ordering::Relaxed);
        self.read.store((read + 1) % CAPACITY, Ordering::Release);
        Some((
            report as u16 as i16,
            (report >> 16) as u16 as i16,
            (report >> 32) as u8,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn click_during_repaint_retains_press_and_release() {
        let queue = PointerQueue::new();
        // The device has already received the entire click before Explorer polls.
        assert!(queue.push(416, 704, 0));
        assert!(queue.push(416, 704, 1));
        assert!(queue.push(416, 704, 0));
        assert_eq!(queue.pop(), Some((416, 704, 0)));
        assert_eq!(queue.pop(), Some((416, 704, 1)));
        assert_eq!(queue.pop(), Some((416, 704, 0)));
        assert_eq!(queue.pop(), None);
    }

    #[test]
    fn full_queue_preserves_unread_reports_and_wraps() {
        let queue = PointerQueue::new();
        for round in 0..3 {
            for index in 0..CAPACITY - 1 {
                assert!(queue.push(index as i16, -7, round));
            }
            assert!(!queue.push(999, 999, 7));
            for index in 0..CAPACITY - 1 {
                assert_eq!(queue.pop(), Some((index as i16, -7, round)));
            }
            assert_eq!(queue.pop(), None);
        }
    }
}
