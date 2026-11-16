//! Report-mode list view with stable rows, range selection and keyboard input.
use super::common::*;
use crate::graphics::{Rect, Surface};
use alloc::{collections::BTreeSet, string::String, vec::Vec};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListColumn {
    pub text: String,
    pub width: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListRow {
    pub id: u64,
    pub cells: Vec<String>,
    pub data: isize,
    pub checked: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListView {
    pub id: u32,
    pub enabled: bool,
    pub focused: bool,
    pub multi_select: bool,
    pub check_boxes: bool,
    pub columns: Vec<ListColumn>,
    rows: Vec<ListRow>,
    selected: BTreeSet<u64>,
    focus: Option<u64>,
    anchor: Option<u64>,
    offset: usize,
    next_id: u64,
}

impl ListView {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            enabled: true,
            focused: false,
            multi_select: false,
            check_boxes: false,
            columns: Vec::new(),
            rows: Vec::new(),
            selected: BTreeSet::new(),
            focus: None,
            anchor: None,
            offset: 0,
            next_id: 1,
        }
    }

    pub fn rows(&self) -> &[ListRow] {
        &self.rows
    }
    pub fn selected(&self) -> impl Iterator<Item = &ListRow> {
        self.rows
            .iter()
            .filter(|row| self.selected.contains(&row.id))
    }
    pub fn focused_item(&self) -> Option<u64> {
        self.focus
    }
    pub fn offset(&self) -> usize {
        self.offset
    }
    pub fn is_selected(&self, id: u64) -> bool {
        self.selected.contains(&id)
    }

    pub fn add_column(&mut self, text: &str, width: u32) {
        self.columns.push(ListColumn {
            text: text.into(),
            width: width.max(16),
        });
    }

    /// Insert at an index; the returned identity survives sorting and insertion.
    pub fn insert(&mut self, index: usize, cells: Vec<String>, data: isize) -> Option<u64> {
        if index > self.rows.len() {
            return None;
        }
        let id = self.next_id;
        self.next_id = id.checked_add(1)?;
        self.rows.insert(
            index,
            ListRow {
                id,
                cells,
                data,
                checked: false,
            },
        );
        Some(id)
    }

    pub fn set_text(&mut self, id: u64, column: usize, text: &str) -> bool {
        let Some(row) = self.rows.iter_mut().find(|row| row.id == id) else {
            return false;
        };
        if column >= self.columns.len().max(1) {
            return false;
        }
        if row.cells.len() <= column {
            row.cells.resize_with(column + 1, String::new);
        }
        row.cells[column] = text.into();
        true
    }

    pub fn remove(&mut self, id: u64) -> bool {
        let Some(index) = self.rows.iter().position(|row| row.id == id) else {
            return false;
        };
        self.rows.remove(index);
        self.selected.remove(&id);
        if self.focus == Some(id) {
            self.focus = self
                .rows
                .get(index.min(self.rows.len().saturating_sub(1)))
                .map(|row| row.id);
        }
        if self.anchor == Some(id) {
            self.anchor = self.focus;
        }
        self.offset = self.offset.min(self.rows.len().saturating_sub(1));
        true
    }

    pub fn clear(&mut self) {
        self.rows.clear();
        self.selected.clear();
        self.focus = None;
        self.anchor = None;
        self.offset = 0;
    }

    /// Stable row identities and selection survive application-defined ordering.
    pub fn sort_by(&mut self, compare: impl FnMut(&ListRow, &ListRow) -> core::cmp::Ordering) {
        self.rows.sort_by(compare);
    }

    pub fn sort_by_column(&mut self, column: usize, descending: bool) {
        self.rows.sort_by(|a, b| {
            let a = a.cells.get(column).map_or("", String::as_str);
            let b = b.cells.get(column).map_or("", String::as_str);
            let order = a.to_lowercase().cmp(&b.to_lowercase());
            if descending {
                order.reverse()
            } else {
                order
            }
        });
    }

    pub fn select(&mut self, id: u64, modifiers: Modifiers) -> Option<CommonNotification> {
        if !self.enabled {
            return None;
        }
        let index = self.rows.iter().position(|row| row.id == id)?;
        if self.multi_select && modifiers.shift {
            let anchor = self
                .anchor
                .and_then(|anchor| self.rows.iter().position(|row| row.id == anchor))
                .unwrap_or(index);
            if !modifiers.control {
                self.selected.clear();
            }
            for row in &self.rows[anchor.min(index)..=anchor.max(index)] {
                self.selected.insert(row.id);
            }
        } else if self.multi_select && modifiers.control {
            if !self.selected.remove(&id) {
                self.selected.insert(id);
            }
            self.anchor = Some(id);
        } else {
            self.selected.clear();
            self.selected.insert(id);
            self.anchor = Some(id);
        }
        self.focus = Some(id);
        Some(CommonNotification::SelectionChanged {
            control: self.id,
            item: id,
        })
    }

    pub fn ensure_visible(&mut self, id: u64, visible_rows: usize) -> bool {
        let Some(index) = self.rows.iter().position(|row| row.id == id) else {
            return false;
        };
        let visible_rows = visible_rows.max(1);
        if index < self.offset {
            self.offset = index;
        } else if index >= self.offset.saturating_add(visible_rows) {
            self.offset = index + 1 - visible_rows;
        }
        true
    }

    /// Set the first visible row without changing focus or selection.
    pub fn set_offset(&mut self, offset: usize, visible_rows: usize) {
        self.offset = offset.min(self.rows.len().saturating_sub(visible_rows.max(1)));
    }

    pub fn scroll(&mut self, rows: i32, visible_rows: usize) {
        self.offset = self
            .offset
            .saturating_add_signed(rows as isize)
            .min(self.rows.len().saturating_sub(visible_rows.max(1)));
    }

    pub fn key(
        &mut self,
        key: CommonKey,
        modifiers: Modifiers,
        visible_rows: usize,
    ) -> Option<CommonNotification> {
        if !self.enabled || self.rows.is_empty() {
            return None;
        }
        let current = self
            .focus
            .and_then(|id| self.rows.iter().position(|row| row.id == id));
        let index = current.unwrap_or(0);
        let next = match key {
            CommonKey::Home => 0,
            CommonKey::End => self.rows.len() - 1,
            CommonKey::Up => index.saturating_sub(1),
            CommonKey::Down => current.map_or(0, |i| (i + 1).min(self.rows.len() - 1)),
            CommonKey::PageUp => index.saturating_sub(visible_rows.max(1)),
            CommonKey::PageDown => index
                .saturating_add(visible_rows.max(1))
                .min(self.rows.len() - 1),
            CommonKey::Enter => {
                return self.focus.map(|item| CommonNotification::Activated {
                    control: self.id,
                    item,
                })
            }
            CommonKey::Space if self.check_boxes => {
                let row = self
                    .rows
                    .iter_mut()
                    .find(|row| Some(row.id) == self.focus)?;
                row.checked = !row.checked;
                return Some(CommonNotification::Checked {
                    control: self.id,
                    item: row.id,
                    checked: row.checked,
                });
            }
            CommonKey::Space => return self.focus.and_then(|id| self.select(id, modifiers)),
            _ => return None,
        };
        let id = self.rows[next].id;
        let event = if self.multi_select && modifiers.control && !modifiers.shift {
            self.focus = Some(id);
            None
        } else {
            self.select(id, modifiers)
        };
        self.ensure_visible(id, visible_rows);
        event
    }

    pub fn click(
        &mut self,
        bounds: Rect,
        x: i32,
        y: i32,
        modifiers: Modifiers,
        double_click: bool,
    ) -> Option<CommonNotification> {
        if !self.enabled || !bounds.contains(x, y) {
            return None;
        }
        self.focused = true;
        let header = if self.columns.is_empty() {
            0
        } else {
            ROW_HEIGHT
        };
        let relative_y = (y - bounds.y) as u32;
        if relative_y < header {
            let mut left = bounds.x;
            for (column, spec) in self.columns.iter().enumerate() {
                if x >= left && i64::from(x) < i64::from(left) + i64::from(spec.width) {
                    return Some(CommonNotification::ColumnClick {
                        control: self.id,
                        column,
                    });
                }
                left = left.saturating_add(spec.width.min(i32::MAX as u32) as i32);
            }
            return None;
        }
        let index = self
            .offset
            .saturating_add(((relative_y - header) / ROW_HEIGHT) as usize);
        let Some(row) = self.rows.get(index) else {
            if !modifiers.control && !modifiers.shift {
                self.selected.clear();
            }
            return None;
        };
        let id = row.id;
        if self.check_boxes && x < bounds.x + 22 {
            let row = &mut self.rows[index];
            row.checked = !row.checked;
            return Some(CommonNotification::Checked {
                control: self.id,
                item: id,
                checked: row.checked,
            });
        }
        let event = self.select(id, modifiers);
        if double_click {
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
        let header = if self.columns.is_empty() {
            0
        } else {
            ROW_HEIGHT
        };
        if header != 0 {
            let h = Rect::new(
                bounds.x,
                bounds.y,
                bounds.width,
                ROW_HEIGHT.min(bounds.height),
            );
            fill(frame, h, 0xfff7f8fa);
            let mut x = bounds.x;
            for column in &self.columns {
                let r = Rect::new(x, bounds.y, column.width, h.height);
                label(
                    frame,
                    text,
                    &column.text,
                    Rect::new(
                        x + 6,
                        bounds.y + 4,
                        column.width.saturating_sub(12),
                        h.height.saturating_sub(4),
                    ),
                    bounds,
                    INK,
                );
                if let Some(r) = r.intersection(&bounds) {
                    border(frame, r, 0xffe3e6eb);
                }
                x = x.saturating_add(column.width.min(i32::MAX as u32) as i32);
            }
        }
        let count = bounds.height.saturating_sub(header) / ROW_HEIGHT;
        for (visible, row) in self
            .rows
            .iter()
            .skip(self.offset)
            .take(count as usize)
            .enumerate()
        {
            let y = bounds.y + header as i32 + visible as i32 * ROW_HEIGHT as i32;
            let r = Rect::new(bounds.x, y, bounds.width, ROW_HEIGHT);
            let selected = self.selected.contains(&row.id);
            if selected {
                fill(
                    frame,
                    r,
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
            let mut x = bounds.x;
            for (column, cell) in row.cells.iter().enumerate() {
                let width = self
                    .columns
                    .get(column)
                    .map_or(bounds.width, |column| column.width);
                let inset = if column == 0 && self.check_boxes {
                    24
                } else {
                    6
                };
                if column == 0 && self.check_boxes {
                    check_box(
                        frame,
                        Rect::new(x + 4, y + 5, 13, 13),
                        row.checked,
                        self.enabled,
                    );
                }
                label(
                    frame,
                    text,
                    cell,
                    Rect::new(x + inset as i32, y + 4, width.saturating_sub(inset + 4), 20),
                    r,
                    color,
                );
                x = x.saturating_add(width.min(i32::MAX as u32) as i32);
            }
            if self.focused && self.focus == Some(row.id) {
                border(frame, r, 0xff8abbe9);
            }
        }
        border(frame, bounds, BORDER);
        restore_clip(frame, old_clip);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample() -> ListView {
        let mut list = ListView::new(10);
        list.multi_select = true;
        list.add_column("Name", 100);
        for name in ["Charlie", "Alpha", "Bravo", "Delta"] {
            list.insert(list.rows.len(), alloc::vec![name.into()], 0);
        }
        list
    }
    #[test]
    fn sorting_and_removal_preserve_selection_identity() {
        let mut list = sample();
        list.select(1, Modifiers::default());
        list.sort_by_column(0, false);
        assert_eq!(list.selected().next().unwrap().cells[0], "Charlie");
        list.remove(2);
        assert!(list.is_selected(1));
        list.remove(1);
        assert_eq!(list.selected().count(), 0);
        assert_eq!(list.focused_item(), Some(4));
    }
    #[test]
    fn range_selection_uses_anchor_and_control_moves_focus_only() {
        let mut list = sample();
        list.select(2, Modifiers::default());
        list.key(
            CommonKey::Down,
            Modifiers {
                shift: true,
                control: false,
            },
            2,
        );
        assert_eq!(
            list.selected().map(|row| row.id).collect::<Vec<_>>(),
            [2, 3]
        );
        list.key(
            CommonKey::Down,
            Modifiers {
                control: true,
                shift: false,
            },
            2,
        );
        assert_eq!(list.focused_item(), Some(4));
        assert!(!list.is_selected(4));
        assert_eq!(list.offset(), 2);
    }
    #[test]
    fn hit_testing_respects_header_offset_and_disabled_state() {
        let mut list = sample();
        let b = Rect::new(10, 20, 160, 72);
        list.scroll(2, 2);
        assert_eq!(
            list.click(b, 15, 25, Modifiers::default(), false),
            Some(CommonNotification::ColumnClick {
                control: 10,
                column: 0
            })
        );
        list.click(b, 15, 48, Modifiers::default(), false);
        assert!(list.is_selected(3));
        list.enabled = false;
        assert_eq!(list.key(CommonKey::Down, Modifiers::default(), 2), None);
        assert_eq!(list.click(b, 15, 70, Modifiers::default(), true), None);
    }
    #[test]
    fn draw_keeps_pixels_outside_bounds_untouched() {
        let list = sample();
        let mut frame = Surface::new(200, 100);
        fill(&mut frame, Rect::new(0, 0, 200, 100), 0xff123456);
        list.draw(&mut frame, Rect::new(20, 10, 100, 72), &|_, _, _, _| {});
        assert_eq!(frame.pixel(19, 10), Some(0xff123456));
        assert_eq!(frame.pixel(120, 40), Some(0xff123456));
        assert_eq!(frame.pixel(30, 40), Some(0xffffffff));
    }
    #[test]
    fn changing_first_column_preserves_other_column_text() {
        let mut list = ListView::new(1);
        list.add_column("Name", 100);
        list.add_column("Type", 80);
        let id = list
            .insert(0, alloc::vec!["Old".into(), "Text file".into()], 0)
            .unwrap();
        assert!(list.set_text(id, 0, "New"));
        assert_eq!(list.rows()[0].cells, alloc::vec!["New", "Text file"]);
    }
}
