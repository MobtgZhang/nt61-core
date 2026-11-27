//! Hierarchical common control with stable item handles and visible-row input.
use super::{common::*, PointerAction};
use crate::applications::shared::scrollbar::Scrollbar;
use crate::graphics::{Rect, Surface};
use alloc::{rc::Rc, string::String, vec::Vec};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeItem {
    pub id: u64,
    pub parent: Option<u64>,
    pub text: String,
    pub data: isize,
    pub expanded: bool,
    pub checked: bool,
    /// Show an expand glyph before children have been loaded.
    pub has_children: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeView {
    pub id: u32,
    pub enabled: bool,
    pub focused: bool,
    pub check_boxes: bool,
    items: Vec<TreeItem>,
    selected: Option<u64>,
    offset: usize,
    next_id: u64,
    folder_icons: Option<Rc<[Option<crate::png::Image>; 6]>>,
    scroll_grab: Option<i32>,
}

impl TreeView {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            enabled: true,
            focused: false,
            check_boxes: false,
            items: Vec::new(),
            selected: None,
            offset: 0,
            next_id: 1,
            folder_icons: None,
            scroll_grab: None,
        }
    }
    pub fn for_folder_picker(id: u32) -> Self {
        let mut tree = Self::new(id);
        tree.folder_icons = Some(Rc::new([
            include_bytes!("../../assets/folder-picker/folder.png").as_slice(),
            include_bytes!("../../assets/folder-picker/user-desktop.png").as_slice(),
            include_bytes!("../../assets/folder-picker/computer.png").as_slice(),
            include_bytes!("../../assets/folder-picker/drive-harddisk.png").as_slice(),
            include_bytes!("../../assets/folder-picker/user-home.png").as_slice(),
            include_bytes!("../../assets/folder-picker/x-office-address-book.png").as_slice(),
        ].map(|bytes| crate::png::decode_png(bytes).ok())));
        tree
    }
    pub fn items(&self) -> &[TreeItem] {
        &self.items
    }
    pub fn item(&self, id: u64) -> Option<&TreeItem> {
        self.items.iter().find(|item| item.id == id)
    }
    pub fn selected(&self) -> Option<u64> {
        self.selected
    }
    pub fn offset(&self) -> usize {
        self.offset
    }
    pub fn insert(&mut self, parent: Option<u64>, text: &str, data: isize) -> Option<u64> {
        if parent.is_some_and(|parent| self.item(parent).is_none()) {
            return None;
        }
        let id = self.next_id;
        self.next_id = id.checked_add(1)?;
        self.items.push(TreeItem {
            id,
            parent,
            text: text.into(),
            data,
            expanded: false,
            checked: false,
            has_children: false,
        });
        Some(id)
    }
    pub fn set_text(&mut self, id: u64, text: &str) -> bool {
        let Some(item) = self.items.iter_mut().find(|item| item.id == id) else {
            return false;
        };
        item.text = text.into();
        true
    }
    pub fn set_has_children(&mut self, id: u64, has_children: bool) -> bool {
        let Some(item) = self.items.iter_mut().find(|item| item.id == id) else {
            return false;
        };
        item.has_children = has_children;
        true
    }
    fn has_children(&self, id: u64) -> bool {
        self.item(id).is_some_and(|item| item.has_children)
            || self.items.iter().any(|item| item.parent == Some(id))
    }
    fn descendant(&self, mut id: u64, ancestor: u64) -> bool {
        while let Some(parent) = self.item(id).and_then(|item| item.parent) {
            if parent == ancestor {
                return true;
            }
            id = parent;
        }
        false
    }
    /// Visible preorder and indentation; collapsing preserves descendants.
    pub fn visible_items(&self) -> Vec<(u64, usize)> {
        let mut visible = Vec::new();
        let mut pending: Vec<_> = self
            .items
            .iter()
            .rev()
            .filter(|item| item.parent.is_none())
            .map(|item| (item.id, 0usize))
            .collect();
        while let Some((id, depth)) = pending.pop() {
            visible.push((id, depth));
            if self.item(id).is_some_and(|item| item.expanded) {
                pending.extend(
                    self.items
                        .iter()
                        .rev()
                        .filter(|item| item.parent == Some(id))
                        .map(|item| (item.id, depth + 1)),
                );
            }
        }
        visible
    }
    pub fn remove(&mut self, id: u64) -> bool {
        let Some(parent) = self.item(id).map(|item| item.parent) else {
            return false;
        };
        let removed: Vec<_> = self
            .items
            .iter()
            .filter(|item| item.id == id || self.descendant(item.id, id))
            .map(|item| item.id)
            .collect();
        if self
            .selected
            .is_some_and(|selected| removed.contains(&selected))
        {
            self.selected = parent;
        }
        self.items.retain(|item| !removed.contains(&item.id));
        self.offset = self
            .offset
            .min(self.visible_items().len().saturating_sub(1));
        true
    }
    pub fn clear(&mut self) {
        self.items.clear();
        self.selected = None;
        self.offset = 0;
    }
    pub fn expand(&mut self, id: u64, expanded: bool) -> Option<CommonNotification> {
        if !self.enabled || !self.has_children(id) {
            return None;
        }
        let item = self.items.iter_mut().find(|item| item.id == id)?;
        if item.expanded == expanded {
            return None;
        }
        item.expanded = expanded;
        if !expanded
            && self
                .selected
                .is_some_and(|selected| self.descendant(selected, id))
        {
            self.selected = Some(id);
        }
        self.offset = self
            .offset
            .min(self.visible_items().len().saturating_sub(1));
        Some(CommonNotification::Expanded {
            control: self.id,
            item: id,
            expanded,
        })
    }
    pub fn select(&mut self, id: u64) -> Option<CommonNotification> {
        if !self.enabled || self.item(id).is_none() {
            return None;
        }
        // Selecting a hidden descendant makes its ancestor chain visible.
        let mut ancestor = self.item(id).and_then(|item| item.parent);
        while let Some(parent) = ancestor {
            let item = self.items.iter_mut().find(|item| item.id == parent)?;
            item.expanded = true;
            ancestor = item.parent;
        }
        if self.selected == Some(id) {
            return None;
        }
        self.selected = Some(id);
        Some(CommonNotification::SelectionChanged {
            control: self.id,
            item: id,
        })
    }
    pub fn ensure_visible(&mut self, id: u64, visible_rows: usize) -> bool {
        let Some(index) = self.visible_items().iter().position(|item| item.0 == id) else {
            return false;
        };
        if index < self.offset {
            self.offset = index;
        } else if index >= self.offset.saturating_add(visible_rows.max(1)) {
            self.offset = index + 1 - visible_rows.max(1);
        }
        true
    }
    pub fn scroll(&mut self, rows: i32, visible_rows: usize) {
        self.offset = self.offset.saturating_add_signed(rows as isize).min(
            self.visible_items()
                .len()
                .saturating_sub(visible_rows.max(1)),
        );
    }
    pub fn rows_in(bounds: Rect) -> usize {
        (bounds.height / ROW_HEIGHT).max(1) as usize
    }

    pub(crate) fn folder_rows_in(bounds: Rect) -> usize {
        (bounds.height.saturating_sub(2) / ROW_HEIGHT).max(1) as usize
    }

    fn folder_bounds(&self, bounds: Rect) -> (Rect, Option<Scrollbar>) {
        let inner = Rect::new(bounds.x + 1, bounds.y + 1,
            bounds.width.saturating_sub(2), bounds.height.saturating_sub(2));
        if inner.width <= 16 {
            return (inner, None);
        }
        let content = Rect::new(inner.x, inner.y, inner.width - 16, inner.height);
        let track = Rect::new(inner.x + content.width as i32, inner.y, 16, inner.height);
        let scrollbar = Scrollbar::new(track, self.visible_items().len(), Self::folder_rows_in(bounds), self.offset);
        (if scrollbar.is_some() { content } else { inner }, scrollbar)
    }

    pub(crate) fn folder_pointer(
        &mut self,
        bounds: Rect,
        x: i32,
        y: i32,
        action: PointerAction,
        double_click: bool,
    ) -> Option<CommonNotification> {
        if matches!(action, PointerAction::Up | PointerAction::Cancel) {
            self.scroll_grab = None;
            return None;
        }
        if self.scroll_grab.is_none() && action != PointerAction::Down {
            return None;
        }
        let (content, scrollbar) = self.folder_bounds(bounds);
        if let Some(grab) = self.scroll_grab {
            if let Some(bar) = scrollbar {
                self.offset = bar.drag_offset(y, grab);
            }
            return None;
        }
        if let Some(bar) = scrollbar.filter(|bar| bar.bounds.contains(x, y)) {
            let page = Self::folder_rows_in(bounds);
            if let Some(down) = bar.arrow(y) {
                self.scroll(if down { 1 } else { -1 }, page);
            } else if bar.thumb.contains(x, y) {
                self.scroll_grab = Some(y - bar.thumb.y);
            } else {
                self.scroll(if y < bar.thumb.y { -(page as i32) } else { page as i32 }, page);
            }
            return None;
        }
        let event = self.click(content, x, y, double_click);
        if matches!(event, Some(CommonNotification::Expanded { expanded: false, .. })) {
            self.scroll(0, Self::folder_rows_in(bounds));
        }
        event
    }
    pub fn key(&mut self, key: CommonKey, visible_rows: usize) -> Option<CommonNotification> {
        if !self.enabled {
            return None;
        }
        let visible = self.visible_items();
        if visible.is_empty() {
            return None;
        }
        let index = self
            .selected
            .and_then(|id| visible.iter().position(|item| item.0 == id));
        let current = index.unwrap_or(0);
        let next = match key {
            CommonKey::Up => current.saturating_sub(1),
            CommonKey::Down => index.map_or(0, |i| (i + 1).min(visible.len() - 1)),
            CommonKey::Home => 0,
            CommonKey::End => visible.len() - 1,
            CommonKey::PageUp => current.saturating_sub(visible_rows.max(1)),
            CommonKey::PageDown => current
                .saturating_add(visible_rows.max(1))
                .min(visible.len() - 1),
            CommonKey::Right => {
                let id = self.selected?;
                if !self.item(id)?.expanded {
                    return self.expand(id, true);
                }
                let child = self
                    .items
                    .iter()
                    .find(|item| item.parent == Some(id))
                    .map(|item| item.id)?;
                let event = self.select(child);
                self.ensure_visible(child, visible_rows);
                return event;
            }
            CommonKey::Left => {
                let id = self.selected?;
                if self.item(id)?.expanded {
                    return self.expand(id, false);
                }
                let parent = self.item(id)?.parent?;
                let event = self.select(parent);
                self.ensure_visible(parent, visible_rows);
                return event;
            }
            CommonKey::Enter => {
                return self.selected.map(|item| CommonNotification::Activated {
                    control: self.id,
                    item,
                })
            }
            CommonKey::Space if self.check_boxes => {
                let item = self
                    .items
                    .iter_mut()
                    .find(|item| Some(item.id) == self.selected)?;
                item.checked = !item.checked;
                return Some(CommonNotification::Checked {
                    control: self.id,
                    item: item.id,
                    checked: item.checked,
                });
            }
            _ => return None,
        };
        let id = visible[next].0;
        let event = self.select(id);
        self.ensure_visible(id, visible_rows);
        event
    }
    pub fn click(
        &mut self,
        bounds: Rect,
        x: i32,
        y: i32,
        double_click: bool,
    ) -> Option<CommonNotification> {
        if !self.enabled || !bounds.contains(x, y) {
            return None;
        }
        let index = self
            .offset
            .saturating_add(((y - bounds.y) as u32 / ROW_HEIGHT) as usize);
        let (id, depth) = *self.visible_items().get(index)?;
        self.focused = true;
        let start = bounds.x.saturating_add((depth.min(1024) as i32) * 18);
        if x < start + 18 && self.has_children(id) {
            return self.expand(id, !self.item(id)?.expanded);
        }
        if self.check_boxes && x < start + 38 {
            let item = self.items.iter_mut().find(|item| item.id == id)?;
            item.checked = !item.checked;
            return Some(CommonNotification::Checked {
                control: self.id,
                item: id,
                checked: item.checked,
            });
        }
        let event = self.select(id);
        if double_click && self.has_children(id) {
            self.expand(id, !self.item(id)?.expanded)
        } else if double_click {
            Some(CommonNotification::Activated {
                control: self.id,
                item: id,
            })
        } else {
            event
        }
    }
    pub fn draw<F: Fn(&mut Surface, &str, Rect, u32)>(
        &self,
        frame: &mut Surface,
        bounds: Rect,
        text: &F,
    ) {
        self.draw_items(frame, bounds, self.focused, false, text);
        border(frame, bounds, BORDER);
    }

    pub fn draw_folders<F: Fn(&mut Surface, &str, Rect, u32)>(
        &self, frame: &mut Surface, bounds: Rect, focused: bool, text: &F,
    ) {
        let (content, scrollbar) = self.folder_bounds(bounds);
        self.draw_items(frame, content, focused, true, text);
        if let Some(bar) = scrollbar {
            bar.draw(frame, false, self.scroll_grab.is_some());
        }
        border(frame, bounds, BORDER);
    }

    fn draw_items<F: Fn(&mut Surface, &str, Rect, u32)>(
        &self, frame: &mut Surface, bounds: Rect, focused: bool, folders: bool, text: &F,
    ) {
        let old_clip = frame.drawing_bounds();
        frame.set_draw_clip(Some(
            old_clip
                .intersection(&bounds)
                .unwrap_or(Rect::new(0, 0, 0, 0)),
        ));
        fill(frame, bounds, 0xffffffff);
        for (row, (id, depth)) in self
            .visible_items()
            .iter()
            .skip(self.offset)
            .take((bounds.height / ROW_HEIGHT) as usize)
            .enumerate()
        {
            let Some(item) = self.item(*id) else {
                continue;
            };
            let y = bounds.y + row as i32 * ROW_HEIGHT as i32;
            let x = bounds.x.saturating_add(((*depth).min(1024) as i32) * 18);
            let selected = self.selected == Some(*id);
            let inset = if self.check_boxes { 40 } else if folders { 39 } else { 20 };
            let right = i64::from(bounds.x) + i64::from(bounds.width);
            let width = (right - i64::from(x) - inset).clamp(0, i64::from(u32::MAX)) as u32;
            if selected {
                fill(
                    frame,
                    if folders {
                        Rect::new(x + inset as i32 - 2, y + 2, (item.text.chars().count() as u32 * 8 + 6).min(width), ROW_HEIGHT - 4)
                    } else {
                        Rect::new(bounds.x, y, bounds.width, ROW_HEIGHT)
                    },
                    if folders && focused && self.enabled {
                        0xffd5eaf6
                    } else if focused && self.enabled {
                        SELECTED
                    } else {
                        INACTIVE
                    },
                );
            }
            let color = if !self.enabled {
                DISABLED
            } else if selected && focused && !folders {
                0xffffffff
            } else {
                INK
            };
            if folders && self.has_children(*id) {
                for step in 0..4 {
                    if item.expanded {
                        frame.fill_rect(x + 4 + step, y + 9 + step, 7 - step as u32 * 2, 1, 0xff53636f);
                    } else {
                        frame.fill_rect(x + 6 + step, y + 7 + step, 1, 7 - step as u32 * 2, 0xff53636f);
                    }
                }
            } else if self.has_children(*id) {
                // Windows 7's compact boxed expansion glyph.
                let r = Rect::new(x + 4, y + 7, 9, 9);
                fill(frame, r, 0xffffffff);
                border(frame, r, BORDER);
                frame.fill_rect(x + 6, y + 11, 5, 1, INK);
                if !item.expanded {
                    frame.fill_rect(x + 8, y + 9, 1, 5, INK);
                }
            }
            if let Some(image) = self.folder_icons.as_ref().and_then(|icons| {
                icons.get(item.data as usize).and_then(Option::as_ref)
            }) {
                for (index, rgba) in image.pixels.chunks_exact(4).enumerate() {
                    let column = x + 19 + (index % image.width as usize) as i32;
                    let row = y + 3 + (index / image.width as usize) as i32;
                    let color = u32::from_be_bytes([rgba[3], rgba[0], rgba[1], rgba[2]]);
                    frame.blend_pixel(column as u32, row as u32, color);
                }
            }
            if self.check_boxes {
                check_box(
                    frame,
                    Rect::new(x + 21, y + 5, 13, 13),
                    item.checked,
                    self.enabled,
                );
            }
            label(
                frame,
                text,
                &item.text,
                Rect::new(x + inset as i32, y + 4, width, 20),
                bounds,
                color,
            );
        }
        restore_clip(frame, old_clip);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    #[test]
    fn collapsed_nodes_hide_descendants_and_arrow_keys_traverse_hierarchy() {
        let mut tree = TreeView::new(10);
        let root = tree.insert(None, "Computer", 1).unwrap();
        let disk = tree.insert(Some(root), "Local Disk (C:)", 2).unwrap();
        let folder = tree.insert(Some(disk), "Windows", 3).unwrap();
        assert_eq!(tree.visible_items(), vec![(root, 0)]);
        tree.select(root);
        tree.key(CommonKey::Right, 5);
        assert_eq!(tree.visible_items(), vec![(root, 0), (disk, 1)]);
        tree.key(CommonKey::Right, 5);
        assert_eq!(tree.selected(), Some(disk));
        tree.key(CommonKey::Right, 5);
        tree.key(CommonKey::Right, 5);
        assert_eq!(tree.selected(), Some(folder));
        tree.key(CommonKey::Left, 5);
        assert_eq!(tree.selected(), Some(disk));
        tree.expand(root, false);
        assert_eq!(tree.selected(), Some(root));
        assert_eq!(tree.items().len(), 3);
    }
    #[test]
    fn deleting_subtree_removes_handles_without_retargeting_and_clears_selection() {
        let mut tree = TreeView::new(10);
        let root = tree.insert(None, "Root", 0).unwrap();
        let child = tree.insert(Some(root), "Child", 0).unwrap();
        let leaf = tree.insert(Some(child), "Leaf", 0).unwrap();
        tree.select(leaf);
        tree.remove(child);
        assert_eq!(tree.selected(), Some(root));
        assert_eq!(tree.items().len(), 1);
        assert!(tree.item(leaf).is_none());
        assert!(tree.insert(Some(leaf), "Invalid", 0).is_none());
        let new = tree.insert(Some(root), "New", 0).unwrap();
        assert!(new > leaf);
        tree.clear();
        assert!(tree.visible_items().is_empty());
        assert_eq!(tree.selected(), None);
    }
    #[test]
    fn selecting_hidden_item_expands_ancestors_and_scrolls_into_view() {
        let mut tree = TreeView::new(10);
        let root = tree.insert(None, "Root", 0).unwrap();
        let mut last = root;
        for _ in 0..20 {
            last = tree.insert(Some(last), "Child", 0).unwrap();
        }
        tree.select(last);
        tree.ensure_visible(last, 5);
        assert_eq!(tree.visible_items().len(), 21);
        assert_eq!(tree.offset(), 16);
        tree.enabled = false;
        assert_eq!(tree.key(CommonKey::Home, 5), None);
        assert_eq!(tree.selected(), Some(last));
    }
    #[test]
    fn glyph_hit_toggles_expansion_without_activating_item() {
        let mut tree = TreeView::new(10);
        let root = tree.insert(None, "Root", 0).unwrap();
        tree.insert(Some(root), "Child", 0);
        assert_eq!(
            tree.click(Rect::new(0, 0, 200, 100), 8, 10, false),
            Some(CommonNotification::Expanded {
                control: 10,
                item: root,
                expanded: true
            })
        );
        assert_eq!(tree.selected(), None);
    }
}
