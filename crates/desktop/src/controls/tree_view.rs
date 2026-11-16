//! Hierarchical common control with stable item handles and visible-row input.
use super::common::*;
use crate::graphics::{Rect, Surface};
use alloc::{string::String, vec::Vec};

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
        }
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
            if selected {
                fill(
                    frame,
                    Rect::new(bounds.x, y, bounds.width, ROW_HEIGHT),
                    if self.focused && self.enabled {
                        SELECTED
                    } else {
                        INACTIVE
                    },
                );
            }
            let color = if !self.enabled {
                DISABLED
            } else if selected && self.focused {
                0xffffffff
            } else {
                INK
            };
            if self.has_children(*id) {
                // Windows 7's compact boxed expansion glyph.
                let r = Rect::new(x + 4, y + 7, 9, 9);
                fill(frame, r, 0xffffffff);
                border(frame, r, BORDER);
                frame.fill_rect(x + 6, y + 11, 5, 1, INK);
                if !item.expanded {
                    frame.fill_rect(x + 8, y + 9, 1, 5, INK);
                }
            }
            let inset = if self.check_boxes { 40 } else { 20 };
            if self.check_boxes {
                check_box(
                    frame,
                    Rect::new(x + 21, y + 5, 13, 13),
                    item.checked,
                    self.enabled,
                );
            }
            let right = i64::from(bounds.x) + i64::from(bounds.width);
            let width = (right - i64::from(x) - inset).clamp(0, i64::from(u32::MAX)) as u32;
            label(
                frame,
                text,
                &item.text,
                Rect::new(x + inset as i32, y + 4, width, 20),
                bounds,
                color,
            );
        }
        border(frame, bounds, BORDER);
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
