use super::types::{ControlKind, ControlVisualState};
use super::{CommonKey, CommonNotification, ListView, Modifiers, Toolbar, TreeView};
use crate::graphics::{Rect, Surface};
use alloc::string::String;
use alloc::vec::Vec;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Control {
    pub id: u32,
    pub kind: ControlKind,
    pub text: String,
    pub enabled: bool,
    pub checked: bool,
    pub selected: bool,
    pub visual_state: ControlVisualState,
    pub value: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlNotification {
    Command,
    Notify,
    SelectionChanged,
    TextChanged,
    ValueChanged,
}

/// Stateful control payloads owned by a native control window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommonControl {
    ListView(ListView),
    TreeView(TreeView),
    Toolbar(Toolbar),
}
impl CommonControl {
    pub fn id(&self) -> u32 {
        match self {
            Self::ListView(control) => control.id,
            Self::TreeView(control) => control.id,
            Self::Toolbar(control) => control.id,
        }
    }
    pub fn kind(&self) -> ControlKind {
        match self {
            Self::ListView(_) => ControlKind::ListView,
            Self::TreeView(_) => ControlKind::Tree,
            Self::Toolbar(_) => ControlKind::Toolbar,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControlWindow {
    pub version: u32,
    pub controls: Vec<Control>,
    common: Vec<CommonControl>,
}

impl ControlWindow {
    pub fn common_controls_v6() -> Self {
        Self {
            version: 6,
            controls: Vec::new(),
            common: Vec::new(),
        }
    }

    /// Install a live toolbar/list/tree model. IDs are unique within a window.
    pub fn add_common(&mut self, control: CommonControl) -> bool {
        let id = control.id();
        if id == 0 || self.controls.iter().any(|control| control.id == id) {
            return false;
        }
        self.controls.push(Control {
            id,
            kind: control.kind(),
            text: String::new(),
            enabled: true,
            checked: false,
            selected: false,
            visual_state: ControlVisualState::Normal,
            value: 0,
        });
        self.common.push(control);
        true
    }

    pub fn common(&self, id: u32) -> Option<&CommonControl> {
        self.common.iter().find(|control| control.id() == id)
    }

    pub fn common_mut(&mut self, id: u32) -> Option<&mut CommonControl> {
        self.common.iter_mut().find(|control| control.id() == id)
    }

    pub fn common_key(
        &mut self,
        id: u32,
        key: CommonKey,
        modifiers: Modifiers,
        visible_rows: usize,
    ) -> Option<CommonNotification> {
        if !self
            .controls
            .iter()
            .any(|control| control.id == id && control.enabled)
        {
            return None;
        }
        match self.common_mut(id)? {
            CommonControl::ListView(control) => control.key(key, modifiers, visible_rows),
            CommonControl::TreeView(control) => control.key(key, visible_rows),
            CommonControl::Toolbar(control) => control.key(key),
        }
    }

    pub fn common_click(
        &mut self,
        id: u32,
        bounds: Rect,
        x: i32,
        y: i32,
        modifiers: Modifiers,
        double_click: bool,
    ) -> Option<CommonNotification> {
        if !self
            .controls
            .iter()
            .any(|control| control.id == id && control.enabled)
        {
            return None;
        }
        match self.common_mut(id)? {
            CommonControl::ListView(control) => {
                control.click(bounds, x, y, modifiers, double_click)
            }
            CommonControl::TreeView(control) => control.click(bounds, x, y, double_click),
            // Toolbar capture must be routed through Toolbar::pointer.
            CommonControl::Toolbar(_) => None,
        }
    }

    /// Route captured toolbar pointer transitions through the same window API.
    /// List/tree selection is emitted on release; use `common_click` for a
    /// double-click gesture recognized by the window's input dispatcher.
    pub fn common_pointer(
        &mut self,
        id: u32,
        action: super::PointerAction,
        bounds: Rect,
        x: i32,
        y: i32,
        modifiers: Modifiers,
    ) -> Option<CommonNotification> {
        if !self
            .controls
            .iter()
            .any(|control| control.id == id && control.enabled)
        {
            return None;
        }
        match self.common_mut(id)? {
            CommonControl::Toolbar(control) => control.pointer(action, bounds, x, y),
            CommonControl::ListView(control) if action == super::PointerAction::Up => {
                control.click(bounds, x, y, modifiers, false)
            }
            CommonControl::TreeView(control) if action == super::PointerAction::Up => {
                control.click(bounds, x, y, false)
            }
            _ => None,
        }
    }

    pub fn common_scroll(&mut self, id: u32, rows: i32, visible_rows: usize) -> bool {
        if !self
            .controls
            .iter()
            .any(|control| control.id == id && control.enabled)
        {
            return false;
        }
        match self.common_mut(id) {
            Some(CommonControl::ListView(control)) => control.scroll(rows, visible_rows),
            Some(CommonControl::TreeView(control)) => control.scroll(rows, visible_rows),
            _ => return false,
        }
        true
    }

    pub fn draw_common<F: Fn(&mut Surface, &str, Rect, u32)>(
        &self,
        id: u32,
        frame: &mut Surface,
        bounds: Rect,
        text: &F,
    ) -> bool {
        let Some(control) = self.common(id) else {
            return false;
        };
        match control {
            CommonControl::ListView(control) => control.draw(frame, bounds, text),
            CommonControl::TreeView(control) => control.draw(frame, bounds, text),
            CommonControl::Toolbar(control) => control.draw(frame, bounds, text),
        }
        true
    }

    pub fn add_button(&mut self, id: u32, text: &str) {
        self.controls.push(Control {
            id,
            kind: ControlKind::Button,
            text: String::from(text),
            enabled: true,
            checked: false,
            selected: false,
            visual_state: ControlVisualState::Normal,
            value: 0,
        });
    }

    pub fn add_edit(&mut self, id: u32, text: &str) {
        self.controls.push(Control {
            id,
            kind: ControlKind::Edit,
            text: String::from(text),
            enabled: true,
            checked: false,
            selected: false,
            visual_state: ControlVisualState::Normal,
            value: 0,
        });
    }

    pub fn add_list(&mut self, id: u32, items: &[&str]) {
        self.controls.push(Control {
            id,
            kind: ControlKind::List,
            text: items.join("|"),
            enabled: true,
            checked: false,
            selected: false,
            visual_state: ControlVisualState::Normal,
            value: 0,
        });
    }

    pub fn add_progress(&mut self, id: u32, value: i32) {
        self.controls.push(Control {
            id,
            kind: ControlKind::Progress,
            text: String::new(),
            enabled: true,
            checked: false,
            selected: false,
            visual_state: ControlVisualState::Normal,
            value: value.clamp(0, 100),
        });
    }

    pub fn set_enabled(&mut self, id: u32, enabled: bool) {
        if let Some(control) = self.common_mut(id) {
            match control {
                CommonControl::ListView(control) => control.enabled = enabled,
                CommonControl::TreeView(control) => control.enabled = enabled,
                CommonControl::Toolbar(control) => {
                    control.enabled = enabled;
                    if !enabled {
                        control.pointer(super::PointerAction::Cancel, Rect::new(0, 0, 0, 0), 0, 0);
                    }
                }
            }
        }
        if let Some(control) = self.controls.iter_mut().find(|control| control.id == id) {
            control.enabled = enabled;
            control.visual_state = if enabled {
                ControlVisualState::Normal
            } else {
                ControlVisualState::Disabled
            };
        }
    }

    pub fn set_visual_state(&mut self, id: u32, state: ControlVisualState) {
        if let Some(control) = self.common_mut(id) {
            let focused = state == ControlVisualState::Focused;
            match control {
                CommonControl::ListView(control) => control.focused = focused,
                CommonControl::TreeView(control) => control.focused = focused,
                CommonControl::Toolbar(control) => control.focused = focused,
            }
        }
        if let Some(control) = self.controls.iter_mut().find(|control| control.id == id) {
            if control.enabled || state == ControlVisualState::Disabled {
                control.visual_state = state;
            }
        }
    }

    pub fn set_text(&mut self, id: u32, text: &str) -> ControlNotification {
        if let Some(control) = self.controls.iter_mut().find(|control| control.id == id) {
            control.text = String::from(text);
            if control.kind == ControlKind::Edit {
                return ControlNotification::TextChanged;
            }
        }
        ControlNotification::Notify
    }

    pub fn set_value(&mut self, id: u32, value: i32) -> ControlNotification {
        if let Some(control) = self.controls.iter_mut().find(|control| control.id == id) {
            control.value = if control.kind == ControlKind::Progress {
                value.clamp(0, 100)
            } else {
                value
            };
            if control.kind == ControlKind::Progress || control.kind == ControlKind::TrackBar {
                return ControlNotification::ValueChanged;
            }
        }
        ControlNotification::Notify
    }

    pub fn select_item(&mut self, id: u32, index: usize) -> ControlNotification {
        let Some(control) = self
            .controls
            .iter()
            .find(|control| control.id == id && control.enabled)
        else {
            return ControlNotification::Notify;
        };
        if control.kind == ControlKind::List && index >= control.text.split('|').count() {
            return ControlNotification::Notify;
        }
        let Ok(value) = i32::try_from(index) else {
            return ControlNotification::Notify;
        };
        if let Some(common) = self.common_mut(id) {
            let changed = match common {
                CommonControl::ListView(control) => {
                    let Some(item) = control.rows().get(index).map(|row| row.id) else {
                        return ControlNotification::Notify;
                    };
                    control.select(item, Modifiers::default()).is_some()
                }
                CommonControl::TreeView(control) => {
                    let Some((item, _)) = control.visible_items().get(index).copied() else {
                        return ControlNotification::Notify;
                    };
                    control.select(item).is_some()
                }
                CommonControl::Toolbar(_) => false,
            };
            if !changed {
                return ControlNotification::Notify;
            }
        }
        if let Some(control) = self.controls.iter_mut().find(|control| control.id == id) {
            control.selected = true;
            control.value = value;
            return ControlNotification::SelectionChanged;
        }
        ControlNotification::Notify
    }

    pub fn activate(&mut self, id: u32) -> Option<ControlNotification> {
        let control = self.controls.iter_mut().find(|control| control.id == id)?;
        if !control.enabled {
            return None;
        }
        control.selected = true;
        control.visual_state = ControlVisualState::Pressed;
        Some(if control.kind == ControlKind::Button {
            ControlNotification::Command
        } else {
            ControlNotification::SelectionChanged
        })
    }

    pub fn command(&mut self, id: u32) -> Option<ControlKind> {
        let control = self
            .controls
            .iter_mut()
            .find(|control| control.id == id && control.enabled)?;
        control.selected = true;
        Some(control.kind)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v6_controls_support_button_commands() {
        let mut controls = ControlWindow::common_controls_v6();
        controls.add_button(1001, "OK");
        assert_eq!(controls.command(1001), Some(ControlKind::Button));
        assert!(controls.controls[0].selected);
    }

    #[test]
    fn v6_controls_handle_states_text_values_and_lists() {
        let mut controls = ControlWindow::common_controls_v6();
        controls.add_edit(2001, "search");
        controls.add_progress(2002, 20);
        controls.add_list(2003, &["Details", "Icons"]);

        assert_eq!(
            controls.set_text(2001, "explorer"),
            ControlNotification::TextChanged
        );
        assert_eq!(
            controls.set_value(2002, 75),
            ControlNotification::ValueChanged
        );
        assert_eq!(
            controls.select_item(2003, 1),
            ControlNotification::SelectionChanged
        );

        controls.set_visual_state(2001, ControlVisualState::Hot);
        controls.set_enabled(2002, false);

        assert_eq!(
            controls.controls[1].visual_state,
            ControlVisualState::Disabled
        );
        assert_eq!(controls.activate(2002), None);
        assert_eq!(
            controls.activate(2001),
            Some(ControlNotification::SelectionChanged)
        );
    }
    #[test]
    fn native_control_window_routes_list_tree_input_and_draws_live_models() {
        let mut window = ControlWindow::common_controls_v6();
        let mut list = ListView::new(40);
        list.insert(0, alloc::vec!["Document.txt".into()], 100);
        assert!(window.add_common(CommonControl::ListView(list)));
        let mut tree = TreeView::new(41);
        tree.insert(None, "Computer", 200);
        assert!(window.add_common(CommonControl::TreeView(tree)));
        assert!(!window.add_common(CommonControl::TreeView(TreeView::new(40))));
        assert_eq!(
            window.select_item(40, 0),
            ControlNotification::SelectionChanged
        );
        assert_eq!(window.select_item(40, 1), ControlNotification::Notify);
        assert_eq!(
            window.common_key(41, CommonKey::Down, Modifiers::default(), 5),
            Some(CommonNotification::SelectionChanged {
                control: 41,
                item: 1
            })
        );
        window.set_enabled(41, false);
        assert_eq!(
            window.common_key(41, CommonKey::Enter, Modifiers::default(), 5),
            None
        );
        let mut frame = Surface::new(200, 100);
        assert!(window.draw_common(40, &mut frame, Rect::new(0, 0, 100, 80), &|_, _, _, _| {}));
        assert_eq!(frame.pixel(10, 10), Some(super::super::common::INACTIVE));
    }

    #[test]
    fn disabled_controls_and_invalid_list_indices_do_not_issue_commands() {
        let mut window = ControlWindow::common_controls_v6();
        window.add_button(1, "OK");
        window.add_list(2, &["One"]);
        window.set_enabled(1, false);
        assert_eq!(window.command(1), None);
        assert_eq!(window.select_item(2, 1), ControlNotification::Notify);
        window.set_value(2, 500);
        window.add_progress(3, 5);
        window.set_value(3, 500);
        assert_eq!(window.controls[2].value, 100);
    }
    #[test]
    fn toolbar_mouse_capture_routes_through_common_control_window() {
        let mut window = ControlWindow::common_controls_v6();
        let mut toolbar = Toolbar::new(50);
        toolbar.add(super::super::ToolbarButton::new(
            100,
            "Open",
            super::super::ToolbarKind::Button,
        ));
        window.add_common(CommonControl::Toolbar(toolbar));
        let b = Rect::new(0, 0, 200, 24);
        window.common_pointer(
            50,
            super::super::PointerAction::Down,
            b,
            5,
            5,
            Modifiers::default(),
        );
        window.set_enabled(50, false);
        window.set_enabled(50, true);
        assert_eq!(
            window.common_pointer(
                50,
                super::super::PointerAction::Up,
                b,
                5,
                5,
                Modifiers::default()
            ),
            None
        );
        window.common_pointer(
            50,
            super::super::PointerAction::Down,
            b,
            5,
            5,
            Modifiers::default(),
        );
        assert_eq!(
            window.common_pointer(
                50,
                super::super::PointerAction::Up,
                b,
                5,
                5,
                Modifiers::default()
            ),
            Some(CommonNotification::Command {
                control: 50,
                command: 100
            })
        );
    }
}
