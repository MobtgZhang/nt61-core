use super::events::ControlEvent;
use super::layout::ControlLayout;
use super::types::{ControlKind, ControlVisualState, PointerAction, CONTROL_NONE};
use crate::graphics::Rect;
use alloc::vec::Vec;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControlTree {
    controls: Vec<ControlLayout>,
    focus: u32,
    hot: u32,
    pressed: u32,
    capture: u32,
}

impl ControlTree {
    pub fn new() -> Self {
        Self {
            controls: Vec::new(),
            focus: CONTROL_NONE,
            hot: CONTROL_NONE,
            pressed: CONTROL_NONE,
            capture: CONTROL_NONE,
        }
    }

    pub fn add(&mut self, id: u32, kind: ControlKind, rect: Rect) {
        self.controls.push(ControlLayout {
            id,
            kind,
            rect,
            visible: true,
            enabled: true,
        });
        if self.focus == CONTROL_NONE && self.can_focus(kind) {
            self.focus = id;
        }
    }

    /// Reflow without losing pointer capture between mouse-down and mouse-up.
    pub fn replace_layout(&mut self, mut next: Self) {
        for (old, slot) in [
            (self.focus, &mut next.focus),
            (self.hot, &mut next.hot),
            (self.pressed, &mut next.pressed),
            (self.capture, &mut next.capture),
        ] {
            if next
                .controls
                .iter()
                .any(|c| c.id == old && c.visible && c.enabled)
            {
                *slot = old;
            }
        }
        *self = next;
    }

    pub fn controls(&self) -> &[ControlLayout] {
        &self.controls
    }

    pub fn control(&self, id: u32) -> Option<&ControlLayout> {
        self.controls.iter().find(|control| control.id == id)
    }

    pub fn add_hidden(&mut self, id: u32, kind: ControlKind, rect: Rect) {
        self.add(id, kind, rect);
        if let Some(control) = self.controls.last_mut() {
            control.visible = false;
        }
        if self.focus == id {
            self.focus = CONTROL_NONE;
        }
    }

    pub fn add_disabled(&mut self, id: u32, kind: ControlKind, rect: Rect) {
        self.add(id, kind, rect);
        if let Some(control) = self.controls.last_mut() {
            control.enabled = false;
        }
        if self.focus == id {
            self.focus = CONTROL_NONE;
        }
    }

    #[must_use]
    pub fn is_enabled(&self, id: u32) -> bool {
        self.control(id).map(|control| control.enabled) == Some(true)
    }

    #[must_use]
    pub fn is_visible(&self, id: u32) -> bool {
        self.control(id).map(|control| control.visible) == Some(true)
    }

    pub fn set_enabled(&mut self, id: u32, enabled: bool) -> bool {
        let Some(control) = self.controls.iter_mut().find(|control| control.id == id) else {
            return false;
        };
        control.enabled = enabled;
        if !enabled && self.focus == id {
            self.focus = CONTROL_NONE;
        }
        true
    }

    pub fn set_visible(&mut self, id: u32, visible: bool) -> bool {
        let Some(control) = self.controls.iter_mut().find(|control| control.id == id) else {
            return false;
        };
        control.visible = visible;
        if !visible && self.focus == id {
            self.focus = CONTROL_NONE;
        }
        true
    }

    pub fn visual_state(&self, id: u32) -> ControlVisualState {
        let Some(control) = self.control(id) else {
            return ControlVisualState::Disabled;
        };
        if !control.enabled {
            return ControlVisualState::Disabled;
        }
        if self.pressed == id {
            ControlVisualState::Pressed
        } else if self.hot == id {
            ControlVisualState::Hot
        } else if self.focus == id {
            ControlVisualState::Focused
        } else {
            ControlVisualState::Normal
        }
    }

    pub fn focus(&self) -> u32 {
        self.focus
    }

    pub fn hot(&self) -> u32 {
        self.hot
    }

    pub fn pressed(&self) -> u32 {
        self.pressed
    }

    pub fn set_focus(&mut self, id: u32) -> Option<ControlEvent> {
        let next_id = self.control(id)?.id;
        let focusable = self
            .control(next_id)
            .map(|control| control.visible && control.enabled && self.can_focus(control.kind))
            .unwrap_or(false);
        if !focusable {
            return None;
        }
        if self.focus == next_id {
            return None;
        }
        self.focus = next_id;
        Some(ControlEvent::FocusChanged(next_id))
    }

    pub fn focus_next(&mut self) -> Option<ControlEvent> {
        let focusable: Vec<&ControlLayout> = self
            .controls
            .iter()
            .filter(|control| control.visible && control.enabled && self.can_focus(control.kind))
            .collect();
        let next = match focusable
            .iter()
            .position(|control| control.id == self.focus)
        {
            Some(index) => focusable.get((index + 1) % focusable.len())?,
            None => focusable.first()?,
        };
        self.focus = next.id;
        Some(ControlEvent::FocusChanged(next.id))
    }

    pub fn hit_test(&self, x: i32, y: i32) -> u32 {
        self.controls
            .iter()
            .rev()
            .find(|control| control.visible && control.enabled && control.rect.contains(x, y))
            .map_or(CONTROL_NONE, |control| control.id)
    }

    pub fn route_pointer(&mut self, action: PointerAction, x: i32, y: i32) -> Option<ControlEvent> {
        if action == PointerAction::Cancel {
            let active = self.hot != CONTROL_NONE
                || self.pressed != CONTROL_NONE
                || self.capture != CONTROL_NONE;
            self.hot = CONTROL_NONE;
            self.pressed = CONTROL_NONE;
            self.capture = CONTROL_NONE;
            return active.then_some(ControlEvent::Cancel);
        }

        let hit = self.hit_test(x, y);
        if action == PointerAction::Move {
            let previous_hot = self.hot;
            self.hot = if self.capture != CONTROL_NONE {
                self.capture
            } else {
                hit
            };
            return if previous_hot == self.hot {
                None
            } else {
                Some(ControlEvent::HoverChanged(self.hot))
            };
        }

        if action == PointerAction::Down {
            if hit == CONTROL_NONE {
                self.pressed = CONTROL_NONE;
                self.capture = CONTROL_NONE;
                return None;
            }
            let kind = self.control(hit)?.kind;
            if self.can_focus(kind) {
                self.focus = hit;
            }
            self.hot = hit;
            self.pressed = hit;
            self.capture = hit;
            return Some(ControlEvent::Pressed(hit));
        }

        let target = if self.capture != CONTROL_NONE {
            self.capture
        } else {
            self.pressed
        };
        self.capture = CONTROL_NONE;
        self.pressed = CONTROL_NONE;
        if hit == target && target != CONTROL_NONE {
            Some(ControlEvent::Activated(target))
        } else {
            None
        }
    }

    pub fn route_key(&mut self, byte: u8) -> Option<ControlEvent> {
        if byte == b'\t' {
            return self.focus_next();
        }
        if byte == b'\r' || byte == b'\n' {
            return match self.control(self.focus).map(|control| control.kind) {
                Some(ControlKind::Edit) => Some(ControlEvent::Submit),
                Some(_) => Some(ControlEvent::Activated(self.focus)),
                None => Some(ControlEvent::Submit),
            };
        }
        if byte == 27 {
            return Some(ControlEvent::Cancel);
        }
        if self.focus == CONTROL_NONE {
            return None;
        }
        let kind = self.control(self.focus).map(|control| control.kind)?;
        if byte == b' ' && kind == ControlKind::Button {
            return Some(ControlEvent::Activated(self.focus));
        }
        let editable = byte == 8 || byte == 127 || byte == b' ' || byte.is_ascii_graphic();
        if kind == ControlKind::Edit && editable {
            return Some(ControlEvent::TextChanged(self.focus));
        }
        None
    }

    fn can_focus(&self, kind: ControlKind) -> bool {
        matches!(
            kind,
            ControlKind::Button | ControlKind::Edit | ControlKind::Image
        )
    }
}

impl Default for ControlTree {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_tree_tracks_hit_focus_and_pointer_notifications() {
        let mut tree = ControlTree::new();
        tree.add(1001, ControlKind::Edit, Rect::new(10, 10, 100, 24));
        tree.add(1002, ControlKind::Button, Rect::new(120, 10, 24, 24));

        assert_eq!(tree.focus(), 1001);
        assert_eq!(tree.hit_test(125, 15), 1002);
        assert_eq!(tree.focus_next(), Some(ControlEvent::FocusChanged(1002)));
        assert_eq!(
            tree.route_pointer(PointerAction::Down, 125, 15),
            Some(ControlEvent::Pressed(1002))
        );
        assert_eq!(
            tree.route_pointer(PointerAction::Up, 125, 15),
            Some(ControlEvent::Activated(1002))
        );
        assert_eq!(tree.route_pointer(PointerAction::Up, 125, 15), None);
        assert_eq!(
            tree.route_pointer(PointerAction::Cancel, 0, 0),
            Some(ControlEvent::Cancel)
        );
    }

    #[test]
    fn control_tree_routes_keyboard_to_focused_edit() {
        let mut tree = ControlTree::new();
        tree.add(2001, ControlKind::Edit, Rect::new(0, 0, 80, 20));
        tree.add(2002, ControlKind::Button, Rect::new(90, 0, 20, 20));

        assert_eq!(
            tree.route_key(b'\t'),
            Some(ControlEvent::FocusChanged(2002))
        );
        assert_eq!(
            tree.route_key(b'\t'),
            Some(ControlEvent::FocusChanged(2001))
        );
        assert_eq!(tree.route_key(b'a'), Some(ControlEvent::TextChanged(2001)));
        assert_eq!(tree.route_key(8), Some(ControlEvent::TextChanged(2001)));
        assert_eq!(tree.route_key(b'\r'), Some(ControlEvent::Submit));
        assert_eq!(tree.route_key(27), Some(ControlEvent::Cancel));
    }

    #[test]
    fn control_tree_activates_focused_button_with_keyboard() {
        let mut tree = ControlTree::new();
        tree.add(3001, ControlKind::Button, Rect::new(0, 0, 80, 20));
        assert_eq!(tree.route_key(b'\r'), Some(ControlEvent::Activated(3001)));
        assert_eq!(tree.route_key(b' '), Some(ControlEvent::Activated(3001)));
    }

    #[test]
    fn control_tree_separates_hover_focus_and_keyboard_targets() {
        let mut tree = ControlTree::new();
        tree.add(3001, ControlKind::Edit, Rect::new(0, 0, 80, 20));
        tree.add(3002, ControlKind::Button, Rect::new(90, 0, 20, 20));

        assert_eq!(
            tree.route_pointer(PointerAction::Move, 95, 10),
            Some(ControlEvent::HoverChanged(3002))
        );
        assert_eq!(tree.focus(), 3001);
        assert_eq!(tree.hot(), 3002);
        assert_eq!(tree.route_key(b'a'), Some(ControlEvent::TextChanged(3001)));
        assert_eq!(
            tree.route_pointer(PointerAction::Down, 95, 10),
            Some(ControlEvent::Pressed(3002))
        );
        assert_eq!(tree.focus(), 3002);
    }

    #[test]
    fn control_tree_ignores_hidden_and_disabled_controls() {
        let mut tree = ControlTree::new();
        tree.add_hidden(4001, ControlKind::Button, Rect::new(0, 0, 80, 20));
        tree.add_disabled(4002, ControlKind::Edit, Rect::new(90, 0, 80, 20));

        assert_eq!(tree.hit_test(10, 10), CONTROL_NONE);
        assert_eq!(tree.hit_test(100, 10), CONTROL_NONE);
        assert_eq!(tree.focus_next(), None);
        assert_eq!(tree.visual_state(4001), ControlVisualState::Normal);
        assert_eq!(tree.visual_state(4002), ControlVisualState::Disabled);
        assert!(tree.set_enabled(4002, true));
        assert_eq!(tree.visual_state(4002), ControlVisualState::Normal);
        assert_eq!(tree.set_focus(4002), Some(ControlEvent::FocusChanged(4002)));
        assert_eq!(tree.visual_state(4002), ControlVisualState::Focused);
    }
}
