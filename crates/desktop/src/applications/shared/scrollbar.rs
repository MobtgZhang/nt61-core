//! Shared vertical scrollbar geometry for shell lists and common dialogs.
use crate::graphics::{Rect, Surface};

#[derive(Clone, Copy, Debug)]
pub struct Scrollbar {
    pub bounds: Rect,
    pub thumb: Rect,
    pub maximum: usize,
    track_y: i32,
    travel: u32,
}
impl Scrollbar {
    pub fn new(bounds: Rect, count: usize, page: usize, offset: usize) -> Option<Self> {
        let maximum = count.saturating_sub(page.max(1));
        if maximum == 0 || bounds.height <= 34 || bounds.width < 4 {
            return None;
        }
        let track = bounds.height - 34;
        let height = ((u128::from(track) * page.max(1) as u128 / count as u128) as u32)
            .max(12)
            .min(track);
        let travel = track - height;
        let track_y = bounds.y + 17;
        let y =
            track_y + (u128::from(travel) * offset.min(maximum) as u128 / maximum as u128) as i32;
        Some(Self {
            bounds,
            thumb: Rect::new(bounds.x + 1, y, bounds.width - 2, height),
            maximum,
            track_y,
            travel,
        })
    }
    pub fn drag_offset(self, y: i32, grab: i32) -> usize {
        let top = (i64::from(y) - i64::from(grab) - i64::from(self.track_y))
            .clamp(0, i64::from(self.travel));
        if self.travel == 0 {
            return 0;
        }
        ((top as u128 * self.maximum as u128 + u128::from(self.travel / 2))
            / u128::from(self.travel)) as usize
    }
    pub fn arrow(self, y: i32) -> Option<bool> {
        if y < self.bounds.y + 17 {
            Some(false)
        } else if y >= self.bounds.y + self.bounds.height as i32 - 17 {
            Some(true)
        } else {
            None
        }
    }
    pub fn draw(self, frame: &mut Surface, hot: bool, held: bool) {
        let b = self.bounds;
        frame.fill_rect(b.x, b.y, b.width, b.height, 0xFFF0F0F0);
        for down in [false, true] {
            let y = if down {
                b.y + b.height as i32 - 17
            } else {
                b.y
            };
            frame.stroke_rounded_rect(Rect::new(b.x, y, b.width, 17), 0, 1, 0xFFBBBBBB);
            for row in 0..4 {
                let line_y = y + if down { 7 + row } else { 9 - row };
                frame.fill_rect(
                    b.x + b.width as i32 / 2 - 3 + row,
                    line_y,
                    (7 - 2 * row) as u32,
                    1,
                    0xFF555555,
                );
            }
        }
        let t = self.thumb;
        frame.fill_rect(
            t.x,
            t.y,
            t.width,
            t.height,
            if held {
                0xFFABD6F1
            } else if hot {
                0xFFC5E6FA
            } else {
                0xFFD8E3EC
            },
        );
        frame.fill_rect(
            t.x + 1,
            t.y + 1,
            t.width / 2,
            t.height.saturating_sub(2),
            0x40FFFFFF,
        );
        frame.stroke_rounded_rect(t, 0, 1, if hot || held { 0xFF3C7FB1 } else { 0xFF9DAFBD });
        if t.height >= 20 {
            for dy in [-2, 0, 2] {
                frame.fill_rect(
                    t.x + 4,
                    t.y + t.height as i32 / 2 + dy,
                    t.width.saturating_sub(8),
                    1,
                    0xFF8299AA,
                );
            }
        }
    }
}
