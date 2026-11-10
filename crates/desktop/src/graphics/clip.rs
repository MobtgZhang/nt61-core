#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub fn intersection(&self, other: &Rect) -> Option<Rect> {
        let left = self.x.max(other.x);
        let top = self.y.max(other.y);
        let right = self
            .x
            .checked_add(self.width as i32)?
            .min(other.x.checked_add(other.width as i32)?);
        let bottom = self
            .y
            .checked_add(self.height as i32)?
            .min(other.y.checked_add(other.height as i32)?);
        if right <= left || bottom <= top {
            return None;
        }
        Some(Rect::new(
            left,
            top,
            (right - left) as u32,
            (bottom - top) as u32,
        ))
    }

    pub fn translate(&self, dx: i32, dy: i32) -> Rect {
        Rect::new(
            self.x.saturating_add(dx),
            self.y.saturating_add(dy),
            self.width,
            self.height,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_intersection_clips_and_rejects_disjoint_rects() {
        let a = Rect::new(-4, -4, 10, 10);
        let b = Rect::new(0, 0, 100, 100);
        assert_eq!(a.intersection(&b), Some(Rect::new(0, 0, 6, 6)),);
        assert_eq!(a.intersection(&Rect::new(10, 10, 4, 4)), None);
    }

    #[test]
    fn rect_translation_keeps_width_and_height() {
        assert_eq!(
            Rect::new(0, 0, 2, 2).translate(-10, -10),
            Rect::new(-10, -10, 2, 2)
        );
    }
}

impl Rect {
    pub fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x
            && x < self.x.saturating_add(self.width as i32)
            && y >= self.y
            && y < self.y.saturating_add(self.height as i32)
    }
}
