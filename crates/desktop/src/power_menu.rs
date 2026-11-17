//! Logon and secure-desktop power popup, with shared hit and paint geometry.
use crate::{
    compositor::aero,
    graphics::{Rect, Surface},
    shell::PowerAction,
};

const NO_SELECTION: usize = usize::MAX;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PowerMenu {
    pub open: bool,
    pub selected: usize,
}

impl Default for PowerMenu {
    fn default() -> Self {
        Self {
            open: false,
            selected: NO_SELECTION,
        }
    }
}

impl PowerMenu {
    pub fn bounds(anchor: Rect) -> Rect {
        Rect::new(
            (anchor.x + anchor.width as i32 - 144).max(0),
            (anchor.y - 78).max(0),
            144,
            78,
        )
    }

    pub fn row(anchor: Rect, index: usize) -> Rect {
        let r = Self::bounds(anchor);
        // The power-saving choice is separated from restart/shutdown, as in AuthUX.
        Rect::new(
            r.x + 3,
            r.y + 3 + index as i32 * 22 + if index > 0 { 6 } else { 0 },
            r.width - 6,
            22,
        )
    }

    /// Includes the translucent shadow and both halves of the split button.
    pub fn damage_bounds(anchor: Rect) -> Rect {
        let r = Self::bounds(anchor);
        Rect::new(
            r.x - 5,
            r.y - 5,
            r.width + 10,
            (anchor.y + anchor.height as i32 + 5 - (r.y - 5)) as u32,
        )
    }

    pub fn toggle(&mut self) {
        self.open = !self.open;
        // Mouse opening must not preselect a destructive action.
        self.selected = NO_SELECTION;
    }

    pub fn action(index: usize) -> Option<PowerAction> {
        match index {
            1 => Some(PowerAction::Restart),
            2 => Some(PowerAction::ShutDown),
            _ => None, // Sleep remains disabled until the platform supports suspend.
        }
    }

    pub fn pointer(&mut self, anchor: Rect, x: i32, y: i32, activate: bool) -> Option<PowerAction> {
        if !self.open {
            return None;
        }
        self.selected = (0..3)
            .find(|&i| Self::row(anchor, i).contains(x, y))
            .filter(|&i| Self::action(i).is_some())
            .unwrap_or(NO_SELECTION);
        if activate {
            let action = Self::action(self.selected);
            if action.is_some() || !Self::bounds(anchor).contains(x, y) {
                self.open = false;
            }
            return action;
        }
        None
    }

    pub fn key(&mut self, key: u8) -> Option<PowerAction> {
        if !self.open {
            return None;
        }
        match key {
            27 => {
                self.open = false;
                self.selected = NO_SELECTION;
            }
            0x81 => self.selected = if self.selected == 2 { 1 } else { 2 },
            0x82 => self.selected = if self.selected == 1 { 2 } else { 1 },
            b'r' | b'R' => {
                self.open = false;
                return Some(PowerAction::Restart);
            }
            b'u' | b'U' => {
                self.open = false;
                return Some(PowerAction::ShutDown);
            }
            13 | 10 | b' ' => {
                let action = Self::action(self.selected);
                if action.is_some() {
                    self.open = false;
                }
                return action;
            }
            _ => {}
        }
        None
    }

    pub fn draw(
        &self,
        frame: &mut Surface,
        anchor: Rect,
        text: impl Fn(&mut Surface, &str, Rect, u32),
    ) {
        if !self.open {
            return;
        }
        let bounds = Self::bounds(anchor);
        aero::menu_surface(frame, bounds);
        frame.fill_rect(
            bounds.x + 31,
            bounds.y + 27,
            bounds.width - 35,
            1,
            0xFFE0E0E0,
        );
        frame.fill_rect(
            bounds.x + 31,
            bounds.y + 28,
            bounds.width - 35,
            1,
            0xFFFFFFFF,
        );
        for (i, label) in ["Sleep", "Restart", "Shut down"].iter().enumerate() {
            let row = Self::row(anchor, i);
            if self.selected == i && Self::action(i).is_some() {
                aero::menu_item(frame, row, true);
            }
            text(
                frame,
                label,
                Rect::new(bounds.x + 33, row.y + 4, bounds.width - 38, 18),
                if i == 0 { 0xFF888888 } else { 0xFF111111 },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const ANCHOR: Rect = Rect {
        x: 980,
        y: 711,
        width: 20,
        height: 28,
    };

    #[test]
    fn disabled_sleep_separator_and_outside_click_never_shutdown() {
        let mut menu = PowerMenu::default();
        menu.toggle();
        assert_eq!(menu.key(13), None);
        let r = PowerMenu::row(ANCHOR, 0);
        assert_eq!(menu.pointer(ANCHOR, r.x + 5, r.y + 5, true), None);
        assert!(menu.open);
        assert_eq!(menu.pointer(ANCHOR, r.x + 40, r.y + 23, true), None);
        assert!(menu.open);
        assert_eq!(menu.pointer(ANCHOR, 1, 1, true), None);
        assert!(!menu.open);
    }

    #[test]
    fn navigation_skips_disabled_rows_wraps_and_escape_does_not_activate() {
        let mut menu = PowerMenu::default();
        menu.toggle();
        menu.key(0x82);
        assert_eq!(menu.selected, 1);
        menu.key(0x82);
        assert_eq!(menu.selected, 2);
        menu.key(0x82);
        assert_eq!(menu.selected, 1);
        menu.key(0x81);
        assert_eq!(menu.key(13), Some(PowerAction::ShutDown));
        menu.toggle();
        menu.key(0x81);
        menu.key(27);
        assert_eq!(menu.key(13), None);
        assert!(!menu.open);
    }

    #[test]
    fn unchanged_hover_is_equal_and_damage_restores_entire_shadow() {
        let mut menu = PowerMenu::default();
        menu.toggle();
        let r = PowerMenu::row(ANCHOR, 1);
        menu.pointer(ANCHOR, r.x + 40, r.y + 5, false);
        let old = menu.clone();
        menu.pointer(ANCHOR, r.x + 60, r.y + 5, false);
        assert_eq!(menu, old);
        let mut background = Surface::new(1024, 768);
        background.fill_rect(0, 0, 1024, 768, 0xFF3289B6);
        let mut frame = background.clone();
        menu.draw(&mut frame, ANCHOR, |_, _, _, _| {});
        assert_ne!(frame, background);
        let r = PowerMenu::damage_bounds(ANCHOR);
        frame.copy_region(&background, r, r.x, r.y);
        assert_eq!(frame, background);
    }
}
