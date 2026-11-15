//! Retain the composed scene and the layers below the foremost window. A
//! pointer hover can repaint that window without rerasterizing every covered
//! application's text, controls and Aero shadows.
use crate::graphics::{Rect, Surface};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Repaint {
    None,
    All,
    Window(u32),
}

pub struct WindowScene {
    composed: Surface,
    beneath: Surface,
    foremost: Option<u32>,
    valid: bool,
    damage: Option<Rect>,
}
impl WindowScene {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            composed: Surface::new(width, height),
            beneath: Surface::new(width, height),
            foremost: None,
            valid: false,
            damage: None,
        }
    }
    pub fn begin(&mut self, frame: &mut Surface, all: bool, window: Option<u32>) -> Repaint {
        self.begin_region(frame, all, window, None)
    }
    pub fn begin_region(
        &mut self,
        frame: &mut Surface,
        all: bool,
        window: Option<u32>,
        damage: Option<Rect>,
    ) -> Repaint {
        frame.set_draw_clip(None);
        self.damage = None;
        if all || !self.valid {
            self.foremost = None;
            return Repaint::All;
        }
        // Damage on a covered window still needs the layers above it, but
        // only inside that rectangle. Keep the rest of the retained scene.
        if window.is_some_and(|id| Some(id) != self.foremost) {
            if let Some(region) = damage {
                self.damage = Some(region);
                frame.set_draw_clip(Some(region));
            } else {
                self.foremost = None;
            }
            return Repaint::All;
        }
        frame.copy_from(&self.composed);
        if let Some(id) = window {
            self.damage = damage;
            frame.set_draw_clip(damage);
            frame.copy_from(&self.beneath);
            Repaint::Window(id)
        } else {
            Repaint::None
        }
    }
    /// Called immediately before the last visible window is drawn, including
    /// its translucent shadow. Restoring this avoids accumulating alpha.
    pub fn save_beneath(&mut self, frame: &Surface, window: u32) {
        frame_copy(&mut self.beneath, frame, self.damage);
        self.foremost = Some(window);
    }
    pub fn damage(&self) -> Option<Rect> {
        self.damage
    }
    pub fn finish(&mut self, frame: &Surface) {
        frame_copy(&mut self.composed, frame, self.damage);
        self.valid = true;
    }
}

fn frame_copy(target: &mut Surface, source: &Surface, damage: Option<Rect>) {
    if let Some(rect) = damage {
        target.copy_region(source, rect, rect.x, rect.y);
    } else {
        target.copy_from(source);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn draw(
        cache: &mut WindowScene,
        frame: &mut Surface,
        full: bool,
        window: Option<u32>,
        color: u32,
    ) -> usize {
        let plan = cache.begin(frame, full, window);
        if plan == Repaint::None {
            return 0;
        }
        let mut draws = 0;
        if plan == Repaint::All {
            frame.fill_rect(0, 0, 32, 24, 0xff102030);
        }
        for id in 1..=6 {
            if matches!(plan,Repaint::Window(w) if w!=id) {
                continue;
            }
            if id == 6 && plan == Repaint::All {
                cache.save_beneath(frame, id);
            }
            frame.shadow_rect(crate::graphics::Rect::new(id as i32, 2, 18, 14), 2);
            frame.fill_rect(
                id as i32,
                2,
                18,
                14,
                if id == 6 { color } else { 0xff405060 + id },
            );
            draws += 1;
        }
        cache.finish(frame);
        draws
    }
    #[test]
    fn six_overlapping_windows_repaint_only_changed_foremost_layer() {
        let mut cached = WindowScene::new(32, 24);
        let mut frame = Surface::new(32, 24);
        assert_eq!(draw(&mut cached, &mut frame, true, None, 0xffabcdef), 6);
        for color in [0xff123456, 0xffabcdef, 0xff123456] {
            assert_eq!(draw(&mut cached, &mut frame, false, Some(6), color), 1);
            let mut oracle = Surface::new(32, 24);
            let mut full = WindowScene::new(32, 24);
            draw(&mut full, &mut oracle, true, None, color);
            assert_eq!(
                frame, oracle,
                "cached alpha/shadows must match complete composition"
            );
        }
        assert_eq!(draw(&mut cached, &mut frame, false, None, 0), 0);
        assert_eq!(
            draw(&mut cached, &mut frame, false, Some(2), 0xff123456),
            6,
            "a change beneath the foremost layer invalidates the stack"
        );
    }
    #[test]
    fn covered_window_damage_matches_full_replay_and_retains_foreground_backdrop() {
        fn layers(
            cache: &mut WindowScene,
            frame: &mut Surface,
            all: bool,
            dirty: Option<u32>,
            region: Option<Rect>,
            color: u32,
        ) {
            let plan = cache.begin_region(frame, all, dirty, region);
            if plan == Repaint::All {
                frame.fill_rect(0, 0, 32, 24, 0xff112233);
            }
            if !matches!(plan, Repaint::Window(_)) {
                frame.fill_rect(2, 2, 24, 18, color);
                cache.save_beneath(frame, 2);
            }
            frame.blend_rect(Rect::new(12, 4, 16, 14), 0x80708090);
            cache.finish(frame);
        }
        let mut cache = WindowScene::new(32, 24);
        let mut frame = Surface::new(32, 24);
        layers(&mut cache, &mut frame, true, None, None, 0xff445566);
        // The entire lower window changes, including pixels covered by alpha.
        let damage = Rect::new(2, 2, 24, 18);
        layers(
            &mut cache,
            &mut frame,
            false,
            Some(1),
            Some(damage),
            0xffaabbcc,
        );
        let mut oracle = Surface::new(32, 24);
        let mut full = WindowScene::new(32, 24);
        layers(&mut full, &mut oracle, true, None, None, 0xffaabbcc);
        assert_eq!(frame.pixels(), oracle.pixels());
        // A later foremost hover must restore the updated lower layer.
        layers(
            &mut cache,
            &mut frame,
            false,
            Some(2),
            Some(Rect::new(12, 4, 16, 14)),
            0xffaabbcc,
        );
        assert_eq!(frame.pixels(), oracle.pixels());
    }

    #[test]
    fn topology_changes_require_fresh_beneath_even_when_window_ids_recur() {
        let mut cached = WindowScene::new(32, 24);
        let mut frame = Surface::new(32, 24);
        draw(&mut cached, &mut frame, true, None, 0xffabcdef);
        assert_eq!(cached.begin(&mut frame, true, Some(6)), Repaint::All);
        frame.fill_rect(0, 0, 32, 24, 0xff999999);
        cached.finish(&frame);
        assert_eq!(
            cached.begin(&mut frame, false, Some(6)),
            Repaint::All,
            "removed window has no valid retained backdrop"
        );
    }
}
