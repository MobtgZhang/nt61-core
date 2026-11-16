//! Common toolbar: hit testing, capture, radio/check buttons and dropdowns.
use super::{common::*, PointerAction};
use crate::graphics::{Rect, Surface};
use alloc::{string::String, vec::Vec};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolbarKind {
    Button,
    Check,
    CheckGroup,
    DropDown,
    Separator,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolbarButton {
    pub command: u32,
    pub text: String,
    pub tooltip: String,
    pub kind: ToolbarKind,
    pub image: Option<u32>,
    pub enabled: bool,
    pub hidden: bool,
    pub checked: bool,
    pub width: u32,
}
impl ToolbarButton {
    pub fn new(command: u32, text: &str, kind: ToolbarKind) -> Self {
        Self {
            command,
            text: text.into(),
            tooltip: text.into(),
            kind,
            image: None,
            enabled: true,
            hidden: false,
            checked: false,
            width: 0,
        }
    }
    fn width(&self) -> u32 {
        if self.kind == ToolbarKind::Separator {
            return if self.width == 0 { 8 } else { self.width };
        }
        if self.width != 0 {
            return self.width.max(16);
        }
        let label = (self.text.chars().count().min(512) as u32) * 7;
        (label
            + 16
            + if self.image.is_some() { 20 } else { 0 }
            + if self.kind == ToolbarKind::DropDown {
                14
            } else {
                0
            })
        .max(24)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Toolbar {
    pub id: u32,
    pub enabled: bool,
    pub focused: bool,
    buttons: Vec<ToolbarButton>,
    hot: Option<u32>,
    pressed: Option<u32>,
    focus: Option<u32>,
    dropdown_pressed: bool,
}
impl Toolbar {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            enabled: true,
            focused: false,
            buttons: Vec::new(),
            hot: None,
            pressed: None,
            focus: None,
            dropdown_pressed: false,
        }
    }
    pub fn buttons(&self) -> &[ToolbarButton] {
        &self.buttons
    }
    pub fn button(&self, command: u32) -> Option<&ToolbarButton> {
        self.buttons
            .iter()
            .find(|button| button.command == command && button.kind != ToolbarKind::Separator)
    }
    pub fn hot_item(&self) -> Option<u32> {
        self.hot
    }
    pub fn focused_item(&self) -> Option<u32> {
        self.focus
    }
    /// Command IDs must be unique; separators use command ID zero.
    pub fn add(&mut self, button: ToolbarButton) -> bool {
        if button.kind != ToolbarKind::Separator
            && (button.command == 0 || self.button(button.command).is_some())
        {
            return false;
        }
        self.buttons.push(button);
        true
    }
    pub fn remove(&mut self, command: u32) -> bool {
        let Some(index) = self
            .buttons
            .iter()
            .position(|button| button.command == command && button.kind != ToolbarKind::Separator)
        else {
            return false;
        };
        self.buttons.remove(index);
        self.clear_target(command);
        true
    }
    fn clear_target(&mut self, command: u32) {
        if self.pressed == Some(command) {
            self.pressed = None;
            self.dropdown_pressed = false;
        }
        if self.hot == Some(command) {
            self.hot = None;
        }
        if self.focus == Some(command) {
            self.focus = None;
        }
    }
    pub fn set_enabled(&mut self, command: u32, enabled: bool) -> bool {
        let Some(button) = self
            .buttons
            .iter_mut()
            .find(|button| button.command == command)
        else {
            return false;
        };
        button.enabled = enabled;
        if !enabled {
            self.clear_target(command);
        }
        true
    }
    pub fn set_hidden(&mut self, command: u32, hidden: bool) -> bool {
        let Some(button) = self
            .buttons
            .iter_mut()
            .find(|button| button.command == command)
        else {
            return false;
        };
        button.hidden = hidden;
        if hidden {
            self.clear_target(command);
        }
        true
    }
    pub fn set_checked(&mut self, command: u32, checked: bool) -> bool {
        let Some(index) = self.buttons.iter().position(|button| {
            button.command == command
                && matches!(button.kind, ToolbarKind::Check | ToolbarKind::CheckGroup)
        }) else {
            return false;
        };
        if checked && self.buttons[index].kind == ToolbarKind::CheckGroup {
            let mut first = index;
            while first > 0 && self.buttons[first - 1].kind == ToolbarKind::CheckGroup {
                first -= 1;
            }
            let mut last = index + 1;
            while last < self.buttons.len() && self.buttons[last].kind == ToolbarKind::CheckGroup {
                last += 1;
            }
            for button in &mut self.buttons[first..last] {
                button.checked = false;
            }
        }
        self.buttons[index].checked = checked;
        true
    }
    /// Only complete buttons fit; clipped trailing buttons never receive input.
    pub fn layout(&self, bounds: Rect) -> Vec<(usize, Rect)> {
        let mut result = Vec::new();
        let mut x = bounds.x;
        let right = i64::from(bounds.x) + i64::from(bounds.width);
        for (index, button) in self
            .buttons
            .iter()
            .enumerate()
            .filter(|(_, button)| !button.hidden)
        {
            let width = button.width();
            if i64::from(x) + i64::from(width) > right {
                break;
            }
            result.push((index, Rect::new(x, bounds.y, width, bounds.height)));
            x = x.saturating_add(width.min(i32::MAX as u32) as i32);
        }
        result
    }
    fn hit(&self, bounds: Rect, x: i32, y: i32) -> Option<(u32, bool)> {
        if !self.enabled {
            return None;
        }
        self.layout(bounds).iter().find_map(|(index, rect)| {
            let button = &self.buttons[*index];
            (button.enabled && button.kind != ToolbarKind::Separator && rect.contains(x, y))
                .then_some((
                    button.command,
                    button.kind == ToolbarKind::DropDown && x >= rect.x + rect.width as i32 - 14,
                ))
        })
    }
    pub fn tooltip(&self, bounds: Rect, x: i32, y: i32) -> Option<&str> {
        let (command, _) = self.hit(bounds, x, y)?;
        self.button(command).map(|button| button.tooltip.as_str())
    }
    pub fn pointer(
        &mut self,
        action: PointerAction,
        bounds: Rect,
        x: i32,
        y: i32,
    ) -> Option<CommonNotification> {
        if action == PointerAction::Cancel {
            self.pressed = None;
            self.hot = None;
            self.dropdown_pressed = false;
            return None;
        }
        let hit = self.hit(bounds, x, y);
        self.hot = hit.map(|(command, _)| command);
        match action {
            PointerAction::Move => None,
            PointerAction::Down => {
                self.pressed = hit.map(|(command, _)| command);
                self.dropdown_pressed = hit.is_some_and(|(_, dropdown)| dropdown);
                if self.pressed.is_some() {
                    self.focus = self.pressed;
                }
                None
            }
            PointerAction::Up => {
                let pressed = self.pressed.take();
                let dropdown = core::mem::take(&mut self.dropdown_pressed);
                let (command, arrow) = hit?;
                if pressed != Some(command) || dropdown != arrow {
                    return None;
                }
                if dropdown {
                    Some(CommonNotification::DropDown {
                        control: self.id,
                        command,
                    })
                } else {
                    self.activate(command)
                }
            }
            PointerAction::Cancel => None,
        }
    }
    pub fn activate(&mut self, command: u32) -> Option<CommonNotification> {
        if !self.enabled {
            return None;
        }
        let button = self.button(command)?;
        if !button.enabled || button.hidden {
            return None;
        }
        match button.kind {
            ToolbarKind::Check => {
                self.set_checked(command, !button.checked);
            }
            ToolbarKind::CheckGroup => {
                self.set_checked(command, true);
            }
            ToolbarKind::Separator => return None,
            _ => {}
        }
        Some(CommonNotification::Command {
            control: self.id,
            command,
        })
    }
    pub fn key(&mut self, key: CommonKey) -> Option<CommonNotification> {
        if !self.enabled {
            return None;
        }
        let enabled: Vec<_> = self
            .buttons
            .iter()
            .filter(|button| {
                button.enabled && !button.hidden && button.kind != ToolbarKind::Separator
            })
            .map(|button| button.command)
            .collect();
        if enabled.is_empty() {
            self.focus = None;
            return None;
        }
        let at = self
            .focus
            .and_then(|command| enabled.iter().position(|id| *id == command));
        match key {
            CommonKey::Left | CommonKey::Up => {
                self.focus = Some(
                    enabled[at.map_or(enabled.len() - 1, |i| {
                        (i + enabled.len() - 1) % enabled.len()
                    })],
                )
            }
            CommonKey::Right => {
                self.focus = Some(enabled[at.map_or(0, |i| (i + 1) % enabled.len())])
            }
            CommonKey::Home => self.focus = enabled.first().copied(),
            CommonKey::End => self.focus = enabled.last().copied(),
            CommonKey::Down => {
                let command = self.focus?;
                if self.button(command)?.kind == ToolbarKind::DropDown {
                    return Some(CommonNotification::DropDown {
                        control: self.id,
                        command,
                    });
                }
            }
            CommonKey::Space | CommonKey::Enter => return self.activate(self.focus?),
            _ => {}
        }
        None
    }
    pub fn draw<F: Fn(&mut Surface, &str, Rect, u32)>(
        &self,
        frame: &mut Surface,
        bounds: Rect,
        text: &F,
    ) {
        self.draw_with_images(frame, bounds, text, &|_, _, _, _| {});
    }
    /// `image` receives the image-list index, target rectangle and enabled state.
    pub fn draw_with_images<F, I>(&self, frame: &mut Surface, bounds: Rect, text: &F, image: &I)
    where
        F: Fn(&mut Surface, &str, Rect, u32),
        I: Fn(&mut Surface, u32, Rect, bool),
    {
        let old_clip = frame.drawing_bounds();
        frame.set_draw_clip(Some(
            old_clip
                .intersection(&bounds)
                .unwrap_or(Rect::new(0, 0, 0, 0)),
        ));
        fill(frame, bounds, 0xfff0f0f0);
        for (index, rect) in self.layout(bounds) {
            let button = &self.buttons[index];
            if button.kind == ToolbarKind::Separator {
                frame.fill_rect(
                    rect.x + rect.width as i32 / 2,
                    rect.y + 4,
                    1,
                    rect.height.saturating_sub(8),
                    0xffd0d0d0,
                );
                continue;
            }
            let enabled = self.enabled && button.enabled;
            let hot = enabled && self.hot == Some(button.command);
            let pressed = enabled && self.pressed == Some(button.command) && hot;
            if hot || button.checked || pressed {
                fill(
                    frame,
                    rect,
                    if pressed || button.checked {
                        0xffc4e4fc
                    } else {
                        0xffe5f3ff
                    },
                );
                border(frame, rect, 0xff7da2ce);
            }
            if self.focused && self.focus == Some(button.command) {
                border(frame, rect, BORDER);
            }
            let mut x = rect.x + 6;
            if let Some(index) = button.image {
                image(
                    frame,
                    index,
                    Rect::new(
                        x,
                        rect.y + (rect.height.saturating_sub(16) / 2) as i32,
                        16,
                        16,
                    ),
                    enabled,
                );
                x += 20;
            }
            let arrow_width = if button.kind == ToolbarKind::DropDown {
                14
            } else {
                0
            };
            label(
                frame,
                text,
                &button.text,
                Rect::new(
                    x,
                    rect.y + (rect.height.saturating_sub(18) / 2) as i32,
                    rect.width
                        .saturating_sub((x - rect.x) as u32 + 4 + arrow_width),
                    18,
                ),
                rect,
                if enabled { INK } else { DISABLED },
            );
            if button.kind == ToolbarKind::DropDown {
                let center = rect.x + rect.width as i32 - 7;
                for row in 0..3 {
                    frame.fill_rect(
                        center - 2 + row,
                        rect.y + rect.height as i32 / 2 - 1 + row,
                        (5 - row * 2) as u32,
                        1,
                        if enabled { INK } else { DISABLED },
                    );
                }
            }
        }
        restore_clip(frame, old_clip);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dragging_outside_and_cancellation_do_not_issue_commands() {
        let mut toolbar = Toolbar::new(1);
        toolbar.add(ToolbarButton::new(10, "Save", ToolbarKind::Button));
        let r = Rect::new(0, 0, 300, 24);
        toolbar.pointer(PointerAction::Down, r, 5, 5);
        assert_eq!(toolbar.pointer(PointerAction::Up, r, 200, 5), None);
        toolbar.pointer(PointerAction::Down, r, 5, 5);
        toolbar.pointer(PointerAction::Cancel, r, 5, 5);
        assert_eq!(toolbar.pointer(PointerAction::Up, r, 5, 5), None);
        toolbar.pointer(PointerAction::Down, r, 5, 5);
        assert_eq!(
            toolbar.pointer(PointerAction::Up, r, 5, 5),
            Some(CommonNotification::Command {
                control: 1,
                command: 10
            })
        );
    }
    #[test]
    fn radio_groups_check_exclusively_and_disabled_buttons_never_activate() {
        let mut toolbar = Toolbar::new(1);
        for id in 10..13 {
            toolbar.add(ToolbarButton::new(id, "Align", ToolbarKind::CheckGroup));
        }
        toolbar.activate(10);
        toolbar.activate(11);
        assert!(!toolbar.button(10).unwrap().checked);
        assert!(toolbar.button(11).unwrap().checked);
        toolbar.set_enabled(12, false);
        assert_eq!(toolbar.activate(12), None);
        toolbar.key(CommonKey::Home);
        toolbar.key(CommonKey::Right);
        toolbar.key(CommonKey::Right);
        assert_eq!(toolbar.focused_item(), Some(10));
    }
    #[test]
    fn split_dropdown_arrow_and_main_button_have_distinct_notifications() {
        let mut toolbar = Toolbar::new(1);
        toolbar.add(ToolbarButton::new(10, "Open", ToolbarKind::DropDown));
        let r = Rect::new(0, 0, 300, 24);
        let width = toolbar.layout(r)[0].1.width as i32;
        toolbar.pointer(PointerAction::Down, r, width - 4, 10);
        assert_eq!(
            toolbar.pointer(PointerAction::Up, r, width - 4, 10),
            Some(CommonNotification::DropDown {
                control: 1,
                command: 10
            })
        );
        toolbar.pointer(PointerAction::Down, r, 5, 10);
        assert_eq!(
            toolbar.pointer(PointerAction::Up, r, 5, 10),
            Some(CommonNotification::Command {
                control: 1,
                command: 10
            })
        );
    }
    #[test]
    fn duplicate_commands_rejected_and_trailing_buttons_cannot_be_clicked() {
        let mut toolbar = Toolbar::new(1);
        toolbar.add(ToolbarButton::new(10, "Save", ToolbarKind::Button));
        assert!(!toolbar.add(ToolbarButton::new(10, "Other", ToolbarKind::Button)));
        assert!(toolbar.layout(Rect::new(0, 0, 12, 24)).is_empty());
        assert_eq!(toolbar.tooltip(Rect::new(0, 0, 12, 24), 5, 5), None);
    }
}
