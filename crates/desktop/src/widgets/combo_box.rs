//! Drop-down selection with a committed value separate from popup highlight.
use super::popup_menu::{PopupItem, PopupKey, PopupMenu, PopupResult};
use crate::graphics::{Rect, Surface};
use alloc::vec::Vec;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComboBox {
    pub id: u32,
    pub enabled: bool,
    pub focused: bool,
    selected: Option<u32>,
    popup: PopupMenu,
}
impl ComboBox {
    pub fn new(id: u32, items: Vec<PopupItem>) -> Self {
        let selected = items
            .iter()
            .find(|item| item.enabled && !item.separator)
            .map(|item| item.id);
        Self {
            id,
            enabled: true,
            focused: false,
            selected,
            popup: PopupMenu::new(items),
        }
    }
    pub fn clear_selection(&mut self){self.selected=None;}
    pub fn selected(&self) -> Option<u32> {
        self.selected
    }
    pub fn selected_text(&self) -> Option<&str> {
        self.popup
            .items()
            .iter()
            .find(|item| Some(item.id) == self.selected)
            .map(|item| item.text.as_str())
    }
    pub fn select(&mut self, id: u32) -> bool {
        if !self
            .popup
            .items()
            .iter()
            .any(|item| item.id == id && item.enabled && !item.separator)
        {
            return false;
        }
        self.selected = Some(id);
        true
    }
    pub fn popup(&self) -> &PopupMenu {
        &self.popup
    }
    pub fn open(&mut self, anchor: Rect, viewport: Rect) {
        if self.enabled {
            self.focused = true;
            self.popup.open(anchor, viewport, self.selected);
        }
    }
    pub fn close(&mut self) {
        self.popup.close();
    }
    fn commit(&mut self, result: PopupResult) -> PopupResult {
        if let PopupResult::Selected(id) = result {
            self.selected = Some(id);
        }
        result
    }
    pub fn key(&mut self, key: PopupKey, anchor: Rect, viewport: Rect) -> PopupResult {
        if !self.enabled {
            self.close();
            return PopupResult::None;
        }
        if self.popup.is_open() {
            let result = self.popup.key(key);
            return self.commit(result);
        }
        if self.focused
            && matches!(
                key,
                PopupKey::Enter | PopupKey::Space | PopupKey::Up | PopupKey::Down
            )
        {
            self.open(anchor, viewport);
        }
        PopupResult::None
    }
    pub fn pointer(
        &mut self,
        x: i32,
        y: i32,
        activate: bool,
        anchor: Rect,
        viewport: Rect,
    ) -> PopupResult {
        if !self.enabled {
            self.close();
            return PopupResult::None;
        }
        if self.popup.is_open() {
            let result = self.popup.pointer(x, y, activate);
            return self.commit(result);
        }
        if activate && anchor.contains(x, y) {
            self.open(anchor, viewport);
        } else if activate {
            self.focused = false;
        }
        PopupResult::None
    }
    pub fn wheel(&mut self, delta: i32) -> bool {
        self.enabled && self.popup.wheel(delta)
    }
    /// Draw popups after sibling controls so they overlay the parent dialog.
    pub fn draw_popup<F: Fn(&mut Surface, &str, Rect, u32)>(
        &self,
        surface: &mut Surface,
        text: &F,
    ) {
        if self.enabled {
            self.popup.draw(surface, text);
        }
    }
    pub fn draw<F: Fn(&mut Surface, &str, Rect, u32)>(
        &self,
        surface: &mut Surface,
        bounds: Rect,
        text: &F,
    ) {
        let edge = if self.enabled && self.focused {
            0xFF3C7FB1
        } else {
            0xFFABADB3
        };
        surface.fill_rect(bounds.x, bounds.y, bounds.width, bounds.height, edge);
        surface.fill_rect(
            bounds.x + 1,
            bounds.y + 1,
            bounds.width.saturating_sub(2),
            bounds.height.saturating_sub(2),
            if self.enabled { 0xFFFFFFFF } else { 0xFFF0F0F0 },
        );
        if let Some(value) = self.selected_text() {
            text(
                surface,
                value,
                Rect::new(
                    bounds.x + 5,
                    bounds.y + 3,
                    bounds.width.saturating_sub(28),
                    bounds.height.saturating_sub(6),
                ),
                if self.enabled { 0xFF101010 } else { 0xFF868686 },
            );
        }
        if bounds.width >= 20 && bounds.height >= 12 {
            let x = bounds.x + bounds.width as i32 - 14;
            let y = bounds.y + bounds.height as i32 / 2 - 1;
            for row in 0..4 {
                surface.fill_rect(x + row, y + row, (7 - row * 2) as u32, 1, 0xFF555555);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    #[test]
    fn popup_selection_is_transactional_and_disabled_controls_do_not_open() {
        let mut combo = ComboBox::new(
            1,
            vec![PopupItem::new(10, "Text"), PopupItem::new(20, "All files")],
        );
        let anchor = Rect::new(10, 10, 180, 25);
        let viewport = Rect::new(0, 0, 640, 480);
        combo.open(anchor, viewport);
        combo.key(PopupKey::Down, anchor, viewport);
        assert_eq!(combo.selected(), Some(10));
        assert_eq!(
            combo.key(PopupKey::Escape, anchor, viewport),
            PopupResult::Cancelled
        );
        assert_eq!(combo.selected(), Some(10));
        combo.open(anchor, viewport);
        combo.key(PopupKey::Down, anchor, viewport);
        assert_eq!(
            combo.key(PopupKey::Enter, anchor, viewport),
            PopupResult::Selected(20)
        );
        assert_eq!(combo.selected_text(), Some("All files"));
        combo.enabled = false;
        combo.open(anchor, viewport);
        assert!(!combo.popup().is_open());
    }
}
