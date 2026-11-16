//! Shared popup interaction for menus and drop-down option lists.
use crate::graphics::{Rect, Surface};
use alloc::{string::String, vec::Vec};

const ROW: u32 = 24;

/// A command or a nonselectable separator. Command IDs must be unique.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PopupItem {
    pub id: u32,
    pub text: String,
    pub enabled: bool,
    pub separator: bool,
}
impl PopupItem {
    pub fn new(id: u32, text: &str) -> Self {
        Self {
            id,
            text: text.into(),
            enabled: true,
            separator: false,
        }
    }
    pub fn disabled(id: u32, text: &str) -> Self {
        Self {
            enabled: false,
            ..Self::new(id, text)
        }
    }
    pub fn separator() -> Self {
        Self {
            id: 0,
            text: String::new(),
            enabled: false,
            separator: true,
        }
    }
    fn selectable(&self) -> bool {
        self.enabled && !self.separator
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PopupKey {
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    Enter,
    Space,
    Escape,
    Tab,
}

/// Cancellation and acceptance are distinct, so previews never commit on Escape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PopupResult {
    None,
    Cancelled,
    Selected(u32),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PopupMenu {
    items: Vec<PopupItem>,
    bounds: Rect,
    selected: Option<usize>,
    offset: usize,
    open: bool,
}
impl PopupMenu {
    pub fn new(items: Vec<PopupItem>) -> Self {
        Self {
            items,
            bounds: Rect::new(0, 0, 0, 0),
            selected: None,
            offset: 0,
            open: false,
        }
    }
    pub fn items(&self) -> &[PopupItem] {
        &self.items
    }
    pub fn bounds(&self) -> Rect {
        self.bounds
    }
    pub fn is_open(&self) -> bool {
        self.open
    }
    pub fn selected(&self) -> Option<u32> {
        self.selected
            .and_then(|i| self.items.get(i))
            .filter(|item| item.selectable())
            .map(|item| item.id)
    }
    pub fn offset(&self) -> usize {
        self.offset
    }
    pub fn set_items(&mut self, items: Vec<PopupItem>) {
        self.items = items;
        self.close();
        self.selected = None;
    }
    /// Place below the anchor, or above it when that offers more space.
    /// The entire popup stays within the supplied viewport, including its border.
    pub fn open(&mut self, anchor: Rect, viewport: Rect, selected: Option<u32>) {
        let left = i64::from(viewport.x);
        let top = i64::from(viewport.y);
        let right = (left + i64::from(viewport.width)).min(i64::from(i32::MAX));
        let bottom = (top + i64::from(viewport.height)).min(i64::from(i32::MAX));
        let width = i64::from(anchor.width.max(120)).min((right - left).max(0));
        let desired = self.items.len().min(12) as i64 * i64::from(ROW) + 2;
        let below_y = (i64::from(anchor.y) + i64::from(anchor.height)).clamp(top, bottom);
        let above_y = i64::from(anchor.y).clamp(top, bottom);
        let below = bottom - below_y;
        let above = above_y - top;
        let (y, height) = if below >= desired || below >= above {
            (below_y, desired.min(below))
        } else {
            let h = desired.min(above);
            (above_y - h, h)
        };
        self.bounds = Rect::new(
            i64::from(anchor.x).clamp(left, right - width) as i32,
            y as i32,
            width as u32,
            height as u32,
        );
        self.offset = 0;
        self.selected = selected
            .and_then(|id| {
                self.items
                    .iter()
                    .position(|item| item.id == id && item.selectable())
            })
            .or_else(|| self.items.iter().position(PopupItem::selectable));
        self.open = width >= 2 && height >= i64::from(ROW + 2) && !self.items.is_empty();
        self.ensure_visible();
    }
    pub fn close(&mut self) {
        self.open = false;
    }
    fn page(&self) -> usize {
        (self.bounds.height.saturating_sub(2) / ROW).max(1) as usize
    }
    fn ensure_visible(&mut self) {
        let page = self.page();
        if let Some(index) = self.selected {
            if index < self.offset {
                self.offset = index;
            }
            if index >= self.offset + page {
                self.offset = index + 1 - page;
            }
        }
        self.offset = self.offset.min(self.items.len().saturating_sub(page));
    }
    fn step(&mut self, forward: bool) {
        if self.items.is_empty() {
            return;
        }
        let count = self.items.len();
        let first = self
            .selected
            .map_or(if forward { 0 } else { count - 1 }, |i| {
                if forward {
                    (i + 1) % count
                } else {
                    (i + count - 1) % count
                }
            });
        for distance in 0..count {
            let index = if forward {
                (first + distance) % count
            } else {
                (first + count - distance) % count
            };
            if self.items[index].selectable() {
                self.selected = Some(index);
                self.ensure_visible();
                return;
            }
        }
        self.selected = None;
    }
    pub fn key(&mut self, key: PopupKey) -> PopupResult {
        if !self.open {
            return PopupResult::None;
        }
        match key {
            PopupKey::Escape | PopupKey::Tab => {
                self.close();
                return PopupResult::Cancelled;
            }
            PopupKey::Enter | PopupKey::Space => {
                if let Some(id) = self.selected() {
                    self.close();
                    return PopupResult::Selected(id);
                }
            }
            PopupKey::Up => self.step(false),
            PopupKey::Down => self.step(true),
            PopupKey::Home => {
                self.selected = self.items.iter().position(PopupItem::selectable);
                self.ensure_visible();
            }
            PopupKey::End => {
                self.selected = self.items.iter().rposition(PopupItem::selectable);
                self.ensure_visible();
            }
            PopupKey::PageUp | PopupKey::PageDown => {
                // Page navigation moves by visual rows, skipping unavailable commands.
                let forward = key == PopupKey::PageDown;
                let current = self.selected.unwrap_or(0);
                let target = if forward {
                    current
                        .saturating_add(self.page())
                        .min(self.items.len() - 1)
                } else {
                    current.saturating_sub(self.page())
                };
                let candidate = if forward {
                    (target..self.items.len())
                        .find(|&i| self.items[i].selectable())
                        .or_else(|| {
                            (current..=target)
                                .rev()
                                .find(|&i| self.items[i].selectable())
                        })
                } else {
                    (0..=target)
                        .rev()
                        .find(|&i| self.items[i].selectable())
                        .or_else(|| (target..=current).find(|&i| self.items[i].selectable()))
                };
                self.selected = candidate;
                self.ensure_visible();
            }
        }
        PopupResult::None
    }
    /// Move highlights with the pointer; activate only on an explicit click.
    /// An outside click is consumed as cancellation by the owning dialog.
    pub fn pointer(&mut self, x: i32, y: i32, activate: bool) -> PopupResult {
        if !self.open {
            return PopupResult::None;
        }
        if !self.bounds.contains(x, y) {
            if activate {
                self.close();
                return PopupResult::Cancelled;
            }
            return PopupResult::None;
        }
        let inside = Rect::new(
            self.bounds.x + 1,
            self.bounds.y + 1,
            self.bounds.width - 2,
            self.bounds.height - 2,
        );
        if !inside.contains(x, y) {
            return PopupResult::None;
        }
        let row = (i64::from(y) - i64::from(self.bounds.y) - 1) / i64::from(ROW);
        let index = self.offset.saturating_add(row.max(0) as usize);
        if row >= 0
            && (row as usize) < self.page()
            && self.items.get(index).is_some_and(PopupItem::selectable)
        {
            self.selected = Some(index);
            if activate {
                let id = self.items[index].id;
                self.close();
                return PopupResult::Selected(id);
            }
        } else {
            self.selected = None;
        }
        PopupResult::None
    }
    /// Positive wheel deltas move toward the start of the list.
    pub fn wheel(&mut self, delta: i32) -> bool {
        if !self.open {
            return false;
        }
        let previous = self.offset;
        self.offset = (self.offset as i64 - i64::from(delta) * 3)
            .clamp(0, self.items.len().saturating_sub(self.page()) as i64)
            as usize;
        self.offset != previous
    }
    pub fn draw<F: Fn(&mut Surface, &str, Rect, u32)>(&self, surface: &mut Surface, text: &F) {
        if !self.open {
            return;
        }
        let b = self.bounds;
        let old_clip = surface.drawing_bounds();
        surface.set_draw_clip(Some(
            old_clip.intersection(&b).unwrap_or(Rect::new(0, 0, 0, 0)),
        ));
        surface.fill_rect(b.x, b.y, b.width, b.height, 0xFF7A7A7A);
        surface.fill_rect(
            b.x + 1,
            b.y + 1,
            b.width.saturating_sub(2),
            b.height.saturating_sub(2),
            0xFFFAFAFA,
        );
        for (row, item) in self
            .items
            .iter()
            .skip(self.offset)
            .take(self.page())
            .enumerate()
        {
            let r = Rect::new(b.x + 1, b.y + 1 + row as i32 * ROW as i32, b.width - 2, ROW);
            if item.separator {
                surface.fill_rect(r.x + 6, r.y + 11, r.width.saturating_sub(12), 1, 0xFFD6D6D6);
                continue;
            }
            if self.selected == Some(row + self.offset) && item.selectable() {
                surface.fill_rect(r.x, r.y, r.width, r.height, 0xFF3399FF);
            }
            let color = if !item.enabled {
                0xFF868686
            } else if self.selected == Some(row + self.offset) {
                0xFFFFFFFF
            } else {
                0xFF101010
            };
            text(
                surface,
                &item.text,
                Rect::new(r.x + 6, r.y + 3, r.width.saturating_sub(12), 18),
                color,
            );
        }
        if self.offset != 0 {
            surface.fill_rect(b.x + b.width as i32 - 5, b.y + 2, 3, 3, 0xFF686868);
        }
        if self.offset + self.page() < self.items.len() {
            surface.fill_rect(
                b.x + b.width as i32 - 5,
                b.y + b.height as i32 - 5,
                3,
                3,
                0xFF686868,
            );
        }
        let full = Rect::new(0, 0, surface.width(), surface.height());
        surface.set_draw_clip(if old_clip == full {
            None
        } else {
            Some(old_clip)
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::{format, vec};
    #[test]
    fn keyboard_skips_unavailable_commands_and_escape_never_accepts() {
        let mut menu = PopupMenu::new(vec![
            PopupItem::disabled(1, "Disabled"),
            PopupItem::new(2, "Open"),
            PopupItem::separator(),
            PopupItem::new(3, "Save"),
        ]);
        menu.open(Rect::new(10, 10, 120, 24), Rect::new(0, 0, 640, 480), None);
        assert_eq!(menu.selected(), Some(2));
        menu.key(PopupKey::Down);
        assert_eq!(menu.selected(), Some(3));
        menu.key(PopupKey::Down);
        assert_eq!(menu.selected(), Some(2));
        assert_eq!(menu.key(PopupKey::Escape), PopupResult::Cancelled);
        assert_eq!(menu.key(PopupKey::Enter), PopupResult::None);
    }
    #[test]
    fn bottom_edge_popup_scrolls_and_accepts_visible_pointer_row() {
        let mut menu = PopupMenu::new(
            (0..30)
                .map(|i| PopupItem::new(i, &format!("Item {i}")))
                .collect(),
        );
        let viewport = Rect::new(20, 30, 180, 150);
        menu.open(Rect::new(190, 165, 200, 24), viewport, Some(29));
        let b = menu.bounds();
        assert_eq!(b.intersection(&viewport), Some(b));
        assert_eq!(menu.selected(), Some(29));
        assert!(menu.offset() > 0);
        menu.key(PopupKey::Home);
        assert_eq!(menu.offset(), 0);
        assert!(menu.wheel(-1));
        assert_eq!(
            menu.pointer(b.x + 12, b.y + 12, true),
            PopupResult::Selected(3)
        );
        assert!(!menu.is_open());
    }
    #[test]
    fn outside_click_and_disabled_row_cannot_dispatch() {
        let mut menu = PopupMenu::new(vec![
            PopupItem::disabled(1, "Disabled"),
            PopupItem::new(2, "Enabled"),
        ]);
        menu.open(Rect::new(0, 0, 120, 24), Rect::new(0, 0, 300, 300), None);
        let b = menu.bounds();
        assert_eq!(menu.pointer(b.x + 10, b.y + 12, true), PopupResult::None);
        assert!(menu.is_open());
        assert_eq!(menu.pointer(299, 299, true), PopupResult::Cancelled);
    }
    #[test]
    fn empty_disabled_and_tiny_popups_are_safe() {
        let mut menu = PopupMenu::new(vec![]);
        menu.open(Rect::new(0, 0, 0, 0), Rect::new(0, 0, 0, 0), None);
        assert!(!menu.is_open());
        menu.set_items(vec![PopupItem::disabled(7, "No commands")]);
        menu.open(Rect::new(0, 0, 120, 24), Rect::new(0, 0, 300, 300), None);
        menu.key(PopupKey::Down);
        assert_eq!(menu.key(PopupKey::Enter), PopupResult::None);
        assert_eq!(menu.selected(), None);
    }
}
