use super::types::{ControlKind, ControlVisualState};
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControlWindow {
    pub version: u32,
    pub controls: Vec<Control>,
}

impl ControlWindow {
    pub fn common_controls_v6() -> Self {
        Self {
            version: 6,
            controls: Vec::new(),
        }
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
            text: items.join("|").into(),
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
            control.value = value;
            if control.kind == ControlKind::Progress || control.kind == ControlKind::TrackBar {
                return ControlNotification::ValueChanged;
            }
        }
        ControlNotification::Notify
    }

    pub fn select_item(&mut self, id: u32, index: usize) -> ControlNotification {
        if let Some(control) = self.controls.iter_mut().find(|control| control.id == id) {
            control.selected = true;
            control.value = index as i32;
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
        let control = self.controls.iter_mut().find(|control| control.id == id)?;
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
}
