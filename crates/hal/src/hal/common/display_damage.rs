//! Atomic damage mailbox shared by framebuffer writers and the display owner.
//! One packed rectangle makes taking damage race-free with concurrent writers.
use core::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DamageRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

pub struct DisplayDamage(AtomicU64);

impl DisplayDamage {
    pub const fn new() -> Self {
        Self(AtomicU64::new(0))
    }

    pub fn mark(&self, x: u32, y: u32, width: u32, height: u32) {
        let right = x.saturating_add(width).min(u16::MAX as u32);
        let bottom = y.saturating_add(height).min(u16::MAX as u32);
        if x >= right || y >= bottom {
            return;
        }
        let mut old = self.0.load(Ordering::Acquire);
        loop {
            let (left, top, right, bottom) = if old == 0 {
                (x, y, right, bottom)
            } else {
                (x.min((old & 0xffff) as u32), y.min(((old >> 16) & 0xffff) as u32),
                 right.max(((old >> 32) & 0xffff) as u32), bottom.max((old >> 48) as u32))
            };
            let next = left as u64 | ((top as u64) << 16)
                | ((right as u64) << 32) | ((bottom as u64) << 48);
            // Release even when already covered: the consumer must observe
            // framebuffer writes made since the previous mark.
            match self.0.compare_exchange_weak(old, next, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => return,
                Err(current) => old = current,
            }
        }
    }

    pub fn take(&self) -> Option<DamageRect> {
        let packed = self.0.swap(0, Ordering::AcqRel);
        if packed == 0 { return None; }
        let x = (packed & 0xffff) as u32;
        let y = ((packed >> 16) & 0xffff) as u32;
        Some(DamageRect { x, y, width: ((packed >> 32) & 0xffff) as u32 - x,
                          height: (packed >> 48) as u32 - y })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn idle_has_no_damage_and_pointer_updates_stay_small() {
        let damage = DisplayDamage::new();
        assert_eq!(damage.take(), None);
        damage.mark(400, 300, 32, 32);
        damage.mark(404, 302, 32, 32);
        assert_eq!(damage.take(), Some(DamageRect { x:400, y:300, width:36, height:34 }));
        assert_eq!(damage.take(), None);
    }
    #[test]
    fn update_after_take_is_retained_and_coordinates_do_not_wrap() {
        let damage = DisplayDamage::new();
        damage.mark(0, 0, 1024, 768);
        damage.take();
        damage.mark(10, 20, 1, 1);
        assert_eq!(damage.take(), Some(DamageRect { x:10,y:20,width:1,height:1 }));
        damage.mark(u32::MAX, 0, 5, 5);
        damage.mark(1, 1, 0, 20);
        assert_eq!(damage.take(), None);
    }
}
