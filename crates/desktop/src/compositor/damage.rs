use alloc::vec::Vec;
use core::ops::Sub;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DamageRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Sub for DamageRect {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        Self {
            x: self.x - other.x,
            y: self.y - other.y,
            width: self.width.saturating_sub(other.width),
            height: self.height.saturating_sub(other.height),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DamageTracker {
    rects: Vec<DamageRect>,
}

impl DamageTracker {
    pub fn new() -> Self {
        Self { rects: Vec::new() }
    }

    pub fn mark(&mut self, rect: DamageRect) {
        self.rects.push(rect);
    }

    pub fn take(&mut self) -> Vec<DamageRect> {
        core::mem::take(&mut self.rects)
    }

    pub fn is_empty(&self) -> bool {
        self.rects.is_empty()
    }
}

impl Default for DamageTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn damage_tracker_accumulates_and_drains() {
        let mut tracker = DamageTracker::new();
        assert!(tracker.is_empty());
        tracker.mark(DamageRect {
            x: 1,
            y: 2,
            width: 3,
            height: 4,
        });
        assert!(!tracker.is_empty());
        assert_eq!(
            tracker.take(),
            vec![DamageRect {
                x: 1,
                y: 2,
                width: 3,
                height: 4,
            }]
        );
        assert!(tracker.is_empty());
    }
}
