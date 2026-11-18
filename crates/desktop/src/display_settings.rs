//! Driver-backed Screen Resolution with confirmation, cancellation and timed rollback.
use crate::control_browser::strings;
use crate::{
    graphics::{Rect, Surface},
    shell::ShellModel,
};
use alloc::{format, string::String, vec::Vec};
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Settings {
    pub now: u64,
    pub modes: Vec<(u32, u32)>,
    pub selected: usize,
    pub open: bool,
    pub first_visible: usize,
    pub previous: Option<(u32, u32)>,
    pub deadline: u64,
    pub error: String,
    pub active: (u32, u32),
    pub close_requested: bool,
    pub close_on_keep: bool,
    pub identified_until: u64,
}
impl Settings {
    pub fn choose(&mut self, index: usize) {
        if index < self.modes.len() {
            self.selected = index;
            self.open = false;
        }
    }
    pub fn reveal_selection(&mut self) {
        if self.selected < self.first_visible {
            self.first_visible = self.selected;
        }
        if self.selected >= self.first_visible + 8 {
            self.first_visible = self.selected - 7;
        }
    }
    pub fn expired(&self, ticks: u64) -> bool {
        self.previous.is_some() && ticks >= self.deadline
    }
    pub fn dirty(&self) -> bool {
        self.modes
            .get(self.selected)
            .is_some_and(|m| *m != self.active)
    }
}
pub fn monitor(b: Rect) -> Rect {
    Rect::new(b.x + 60, b.y + 44, b.width.saturating_sub(110), 96)
}
pub fn dropdown(b: Rect) -> Rect {
    Rect::new(b.x + 134, b.y + 184, 110, 23)
}
pub fn footer_button(b: Rect, index: u32) -> Rect {
    Rect::new(
        b.x + b.width as i32 - 244 + index as i32 * 78,
        b.y + b.height as i32 - 34,
        72,
        23,
    )
}
pub fn advanced_link(b: Rect) -> Rect {
    Rect::new(b.x + b.width as i32 - 148, b.y + 218, 124, 22)
}
pub fn text_size_link(b: Rect) -> Rect {
    Rect::new(b.x + 60, b.y + 246, b.width.saturating_sub(100), 22)
}
pub fn help_link(b: Rect) -> Rect {
    Rect::new(b.x + 60, b.y + 267, b.width.saturating_sub(100), 22)
}
pub fn confirmation(b: Rect) -> Rect {
    Rect::new(
        b.x + (b.width.saturating_sub(370) / 2) as i32,
        b.y + 100,
        370.min(b.width),
        160,
    )
}
pub fn confirm_button(b: Rect, keep: bool) -> Rect {
    let r = confirmation(b);
    Rect::new(r.x + if keep { 30 } else { 194 }, r.y + 112, 145, 26)
}
pub fn list_rows(b: Rect) -> usize {
    let d = dropdown(b);
    let room = (b.y + b.height as i32 - 45 - (d.y + 23)).max(88) as u32;
    (room / 22).saturating_sub(2).clamp(2, 8) as usize
}
impl ShellModel {
    pub fn load_display_modes(&mut self) {
        if self.display.modes.is_empty() {
            self.display.modes = crate::platform::services::display_modes()
                .unwrap_or_else(|_| alloc::vec![self.viewport]);
            // Preserve a valid current selection even if the driver omits its boot mode.
            if !self.display.modes.contains(&self.viewport) {
                self.display.modes.push(self.viewport);
            }
            self.display.selected = self
                .display
                .modes
                .iter()
                .position(|m| *m == self.viewport)
                .unwrap_or(0);
        }
        self.display.active = self.viewport;
    }
    pub fn display_tick(&mut self, ticks: u64) -> bool {
        let was_identified = self.display.now < self.display.identified_until;
        self.display.now = ticks;
        if self.display.expired(ticks) {
            self.revert_display();
            return true;
        }
        was_identified && ticks >= self.display.identified_until
    }
    pub fn revert_display(&mut self) {
        if let Some((w, h)) = self.display.previous {
            match crate::platform::services::display_set(w, h) {
                Ok(()) => {
                    self.display.previous = None;
                    self.display.active = (w, h);
                    self.display.selected = self
                        .display
                        .modes
                        .iter()
                        .position(|m| *m == (w, h))
                        .unwrap_or(0);
                    self.display.error.clear();
                    self.display.close_on_keep = false;
                }
                Err(s) => self.display.error = strings::restore_failed(s),
            }
        }
    }
    fn keep_display(&mut self) {
        match crate::platform::services::display_preference("keep") {
            Ok(()) => {
                self.display.previous = None;
                self.display.error.clear();
                self.display.close_requested = self.display.close_on_keep;
                self.display.close_on_keep = false;
            }
            Err(s) => self.display.error = strings::save_failed(s),
        }
    }
    fn apply_display(&mut self, close: bool) {
        let Some(&(w, h)) = self.display.modes.get(self.display.selected) else {
            return;
        };
        if (w, h) == self.viewport {
            self.display.close_requested = close;
            return;
        }
        match crate::platform::services::display_set(w, h) {
            Ok(()) => {
                self.display.previous = Some(self.viewport);
                self.display.active = (w, h);
                self.display.deadline = self.display.now.saturating_add(15000);
                self.display.error.clear();
                self.display.close_on_keep = close;
            }
            Err(s) => self.display.error = strings::mode_rejected(s),
        }
    }
    pub fn display_key(&mut self, key: u8) -> bool {
        self.load_display_modes();
        match key {
            27 => {
                if self.display.open {
                    self.display.open = false;
                } else if self.display.previous.is_some() {
                    self.revert_display();
                } else {
                    self.display.selected = self
                        .display
                        .modes
                        .iter()
                        .position(|m| *m == self.viewport)
                        .unwrap_or(0);
                    self.display.close_requested = true;
                }
            }
            0x26 | 0x28 if self.display.previous.is_none() => {
                if key == 0x26 {
                    self.display.selected = self.display.selected.saturating_sub(1);
                } else {
                    self.display.selected =
                        (self.display.selected + 1).min(self.display.modes.len().saturating_sub(1));
                }
                self.display.open = true;
                self.display.reveal_selection();
            }
            13 | 10 if self.display.previous.is_some() => self.keep_display(),
            13 | 10 if self.display.open => self.display.open = false,
            13 | 10 if self.display.previous.is_none() => self.apply_display(true),
            _ => return false,
        }
        true
    }
    pub fn display_click(&mut self, b: Rect, x: i32, y: i32, ticks: u64) -> bool {
        self.load_display_modes();
        self.display.now = ticks;
        if self.display.previous.is_some() {
            if confirm_button(b, true).contains(x, y) {
                self.keep_display();
            } else if confirm_button(b, false).contains(x, y) {
                self.revert_display();
            }
            return true;
        }
        let d = dropdown(b);
        let rows = list_rows(b);
        if self.display.open {
            let list = Rect::new(d.x, d.y + 23, d.width, (rows as u32 + 2) * 22);
            if list.contains(x, y) {
                let row = ((y - list.y) / 22) as usize;
                if row == 0 {
                    self.display.first_visible = self.display.first_visible.saturating_sub(1);
                } else if row == rows + 1 {
                    self.display.first_visible = (self.display.first_visible + 1)
                        .min(self.display.modes.len().saturating_sub(rows));
                } else {
                    self.display.choose(self.display.first_visible + row - 1);
                }
            } else {
                self.display.open = false;
            }
            return true;
        }
        if d.contains(x, y) {
            self.display.open = true;
            self.display.reveal_selection();
            self.display.first_visible = self
                .display
                .first_visible
                .max(self.display.selected.saturating_add(1).saturating_sub(rows));
            return true;
        }
        let panel = monitor(b);
        if Rect::new(panel.x + panel.width as i32 - 83, panel.y + 10, 70, 24).contains(x, y) {
            match crate::platform::services::display_modes() {
                Ok(modes) if !modes.is_empty() => {
                    let selected = self
                        .display
                        .modes
                        .get(self.display.selected)
                        .copied()
                        .unwrap_or(self.viewport);
                    self.display.modes = modes;
                    if !self.display.modes.contains(&self.viewport) {
                        self.display.modes.push(self.viewport);
                    }
                    self.display.selected = self
                        .display
                        .modes
                        .iter()
                        .position(|m| *m == selected)
                        .unwrap_or(0);
                    self.display.error = strings::DETECTED_ONE_MONITOR.into();
                }
                Ok(_) => self.display.error = strings::NO_ADDITIONAL_MONITORS_WERE_DETECTED.into(),
                Err(s) => self.display.error = strings::detect_failed(s),
            };
            return true;
        }
        if Rect::new(panel.x + panel.width as i32 - 83, panel.y + 42, 70, 24).contains(x, y) {
            self.display.identified_until = ticks.saturating_add(2500);
            return true;
        }
        if footer_button(b, 0).contains(x, y) {
            self.apply_display(true);
            return true;
        }
        if footer_button(b, 1).contains(x, y) {
            self.display.selected = self
                .display
                .modes
                .iter()
                .position(|m| *m == self.viewport)
                .unwrap_or(0);
            self.display.error.clear();
            self.display.close_requested = true;
            return true;
        }
        if footer_button(b, 2).contains(x, y) {
            if self.display.dirty() {
                self.apply_display(false);
            }
            return true;
        }
        false
    }
}
pub fn draw(
    s: &Settings,
    f: &mut Surface,
    b: Rect,
    ticks: u64,
    font: &crate::graphics::font::FontLib,
    mut text: impl FnMut(&mut Surface, &str, Rect, u32),
) {
    const BLUE: u32 = 0xFF0066CC;
    const BLACK: u32 = 0xFF111111;
    f.fill_rect(b.x, b.y, b.width, b.height, 0xFFFFFFFF);
    text(
        f,
        strings::CHANGE_THE_APPEARANCE_OF_YOUR_DISPLAY,
        Rect::new(b.x + 60, b.y + 16, b.width.saturating_sub(90), 26),
        0xFF003399,
    );
    let panel = monitor(b);
    f.fill_rect(panel.x, panel.y, panel.width, panel.height, 0xFFB5B5B5);
    f.fill_rect(
        panel.x + 1,
        panel.y + 1,
        panel.width.saturating_sub(2),
        panel.height - 2,
        0xFFFFFFFF,
    );
    let m = Rect::new(
        panel.x + (panel.width.saturating_sub(185) / 2) as i32,
        panel.y + 18,
        94,
        64,
    );
    f.fill_rect(m.x - 3, m.y - 3, m.width + 6, m.height + 6, 0xFFC6E3F1);
    f.fill_rect(m.x, m.y, m.width, m.height, 0xFF598EA7);
    f.fill_rect(m.x + 3, m.y + 3, m.width - 6, m.height - 6, 0xFF244E89);
    f.fill_rect(m.x + 9, m.y + 8, 72, 39, 0xFFC7D6E7);
    f.fill_rect(m.x + 14, m.y + 13, 63, 31, 0xFF5F87BB);
    f.fill_rect(m.x + 9, m.y + 8, 72, 5, 0xFFF8F8FC);
    f.blend_rounded_rect(Rect::new(m.x + 23, m.y + 10, 48, 48), 24, 0xFF344DA0);
    f.stroke_rounded_rect(Rect::new(m.x + 23, m.y + 10, 48, 48), 24, 2, 0xFFFFFFFF);
    {
        crate::control_browser::paint::label(
            &font,
            f,
            "1",
            Rect::new(m.x + 37, m.y + 17, 28, 40),
            0xFFFFFFFF,
            32,
        );
    }
    {
        for (i, label) in [strings::DETECT, strings::IDENTIFY].iter().enumerate() {
            crate::control_browser::paint::button(
                f,
                Rect::new(
                    panel.x + panel.width as i32 - 83,
                    panel.y + 10 + i as i32 * 32,
                    70,
                    24,
                ),
                label,
                true,
                &font,
            );
        }
    }
    text(
        f,
        strings::DISPLAY_2,
        Rect::new(b.x + 60, b.y + 156, 90, 22),
        BLACK,
    );
    let monitor = Rect::new(
        b.x + 134,
        b.y + 151,
        250.min(b.width.saturating_sub(150)),
        23,
    );
    f.fill_rect(
        monitor.x,
        monitor.y,
        monitor.width,
        monitor.height,
        0xFF999999,
    );
    f.fill_rect(
        monitor.x + 1,
        monitor.y + 1,
        monitor.width - 2,
        monitor.height - 2,
        0xFFF3F3F3,
    );
    text(
        f,
        strings::LABEL_1_GENERIC_NON_PNP_MONITOR,
        Rect::new(monitor.x + 5, monitor.y + 4, monitor.width - 8, 20),
        BLACK,
    );
    text(
        f,
        strings::RESOLUTION,
        Rect::new(b.x + 60, b.y + 188, 90, 22),
        BLACK,
    );
    let d = dropdown(b);
    f.fill_rect(d.x, d.y, d.width, d.height, 0xFF8A8A8A);
    f.fill_rect(d.x + 1, d.y + 1, d.width - 2, d.height - 2, 0xFFF3F3F3);
    if let Some((w, h)) = s.modes.get(s.selected) {
        text(
            f,
            &format!("{w} × {h}"),
            Rect::new(d.x + 5, d.y + 4, d.width - 20, 20),
            BLACK,
        );
    }
    text(
        f,
        "▾",
        Rect::new(d.x + d.width as i32 - 15, d.y + 4, 14, 20),
        BLACK,
    );
    text(f, strings::ADVANCED_SETTINGS, advanced_link(b), BLUE);
    f.fill_rect(
        b.x + 60,
        b.y + 239,
        b.width.saturating_sub(95),
        1,
        0xFFD8D8D8,
    );
    text(
        f,
        strings::MAKE_TEXT_AND_OTHER_ITEMS_LARGER_OR_SMALLER,
        text_size_link(b),
        BLUE,
    );
    text(
        f,
        strings::WHAT_DISPLAY_SETTINGS_SHOULD_I_CHOOSE,
        help_link(b),
        BLUE,
    );
    let footer = Rect::new(b.x, b.y + b.height as i32 - 46, b.width, 46);
    f.fill_rect(footer.x, footer.y, footer.width, footer.height, 0xFFF7F7F7);
    f.fill_rect(footer.x, footer.y, footer.width, 1, 0xFFD0D0D0);
    {
        for (i, label) in [strings::OK, strings::CANCEL, strings::APPLY]
            .iter()
            .enumerate()
        {
            crate::control_browser::paint::button(
                f,
                footer_button(b, i as u32),
                label,
                i != 2 || s.dirty(),
                &font,
            );
        }
    }
    if !s.error.is_empty() {
        text(
            f,
            &s.error,
            Rect::new(
                b.x + 60,
                b.y + b.height as i32 - 73,
                b.width.saturating_sub(95),
                28,
            ),
            0xFF992222,
        );
    }
    if s.open {
        let rows = list_rows(b);
        let r = Rect::new(d.x, d.y + 23, d.width, (rows as u32 + 2) * 22);
        f.shadow_rect(r, 3);
        f.fill_rect(r.x, r.y, r.width, r.height, 0xFF7A9CBF);
        f.fill_rect(r.x + 1, r.y + 1, r.width - 2, r.height - 2, 0xFFFFFFFF);
        text(f, "▴", Rect::new(r.x + 48, r.y + 2, 16, 20), BLACK);
        text(
            f,
            "▾",
            Rect::new(r.x + 48, r.y + r.height as i32 - 21, 16, 20),
            BLACK,
        );
        for (i, (w, h)) in s.modes.iter().enumerate().skip(s.first_visible).take(rows) {
            let y = r.y + 22 + (i - s.first_visible) as i32 * 22;
            if i == s.selected {
                f.fill_rect(r.x + 1, y, r.width - 2, 22, 0xFF3399FF);
            }
            text(
                f,
                &format!("{w} × {h}"),
                Rect::new(r.x + 5, y + 3, r.width - 10, 20),
                if i == s.selected { 0xFFFFFFFF } else { BLACK },
            );
        }
    }
    if s.previous.is_some() {
        let r = confirmation(b);
        f.shadow_rect(r, 8);
        f.fill_rect(r.x, r.y, r.width, r.height, 0xFF707070);
        f.fill_rect(r.x + 1, r.y + 1, r.width - 2, r.height - 2, 0xFFF5FAFF);
        text(
            f,
            strings::DO_YOU_WANT_TO_KEEP_THESE_DISPLAY_SETTINGS,
            Rect::new(r.x + 20, r.y + 18, r.width - 40, 26),
            0xFF003399,
        );
        text(
            f,
            &strings::reverting_in(s.deadline.saturating_sub(ticks).div_ceil(1000)),
            Rect::new(r.x + 20, r.y + 58, r.width - 40, 28),
            BLACK,
        );
        {
            crate::control_browser::paint::button(
                f,
                confirm_button(b, true),
                strings::KEEP_CHANGES,
                true,
                &font,
            );
            crate::control_browser::paint::button(
                f,
                confirm_button(b, false),
                strings::REVERT,
                true,
                &font,
            );
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn confirmation_timeout_and_mode_selection() {
        let mut s = Settings {
            modes: alloc::vec![(800, 600), (1024, 768)],
            active: (1024, 768),
            ..Default::default()
        };
        s.choose(1);
        assert!(!s.dirty());
        s.choose(99);
        assert_eq!(s.selected, 1);
        s.choose(0);
        assert!(s.dirty());
        s.previous = Some((1024, 768));
        s.deadline = 15000;
        assert!(!s.expired(14999));
        assert!(s.expired(15000));
        s.previous = None;
        assert!(!s.expired(20000));
    }
}
