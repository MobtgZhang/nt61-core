//! Shared application modal dialogs. Changes are applied only after acceptance.
//! Printing submits complete jobs to the durable PostScript document backend.
use super::{explorer::input, notepad, AppAction, AppState, ApplicationSession};
use crate::{
    graphics::{Rect, Surface},
    widgets::popup_menu::{PopupItem, PopupKey, PopupMenu, PopupResult},
};
use alloc::string::{String, ToString};

mod color;
mod render;
pub use color::{ColorDialog, BASIC_COLORS};
pub use render::{bounds, draw};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorTarget {
    Paint,
    Text,
    Highlight,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontDialog {
    pub font: notepad::Font,
    pub points: String,
    pub underline: bool,
    pub strike: bool,
    pub focus: usize,
    pub select: bool,
    pub error: String,
}
impl FontDialog {
    pub fn new(font: notepad::Font) -> Self {
        Self {
            font,
            points: font.points.to_string(),
            underline: false,
            strike: false,
            focus: 0,
            select: true,
            error: String::new(),
        }
    }
    pub fn validate(&mut self) -> bool {
        match self.points.parse::<u32>() {
            Ok(size @ 1..=144) => {
                self.font.points = size;
                self.error.clear();
                true
            }
            _ => {
                self.error = "Enter a font size from 1 to 144 points.".into();
                false
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrintDialog {
    pub selection_available: bool,
    pub selection: bool,
    pub range: bool,
    pub from: String,
    pub to: String,
    pub copies: String,
    pub collate: bool,
    pub focus: usize,
    pub select: bool,
    pub error: String,
}
impl PrintDialog {
    fn new(selection_available: bool) -> Self {
        Self {
            selection_available,
            selection: false,
            range: false,
            from: "1".into(),
            to: "1".into(),
            copies: "1".into(),
            collate: false,
            focus: 0,
            select: true,
            error: String::new(),
        }
    }
    pub fn valid_options(&self) -> bool {
        let copies = self
            .copies
            .parse::<u16>()
            .is_ok_and(|n| (1..=9999).contains(&n));
        copies
            && (!self.range
                || matches!((self.from.parse::<u16>(), self.to.parse::<u16>()),
            (Ok(a), Ok(b)) if a > 0 && a <= b))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Dialog {
    Color {
        draft: ColorDialog,
        target: ColorTarget,
    },
    Font(FontDialog),
    Print(PrintDialog),
    Page {
        draft: notepad::Page,
        focus: usize,
        select: bool,
        error: String,
        popup: PopupMenu,
    },
}

pub fn open_color(app: &mut ApplicationSession, target: ColorTarget) {
    let color = match (&app.state, target) {
        (AppState::Paint(p), ColorTarget::Paint) => p.color,
        (AppState::Document(d), ColorTarget::Text) => d.current_style().color,
        (AppState::Document(d), ColorTarget::Highlight) => d.current_style().highlight,
        _ => return,
    };
    app.menu = false;
    app.common_scroll = (0, 0);
    app.common_dialog = Some(Dialog::Color {
        draft: ColorDialog::new(color, app.custom_colors),
        target,
    });
}
pub fn open_font(app: &mut ApplicationSession) {
    let AppState::Document(d) = &app.state else {
        return;
    };
    let style = d.current_style();
    let mut draft = FontDialog::new(if app.id == super::AppId::Notepad {
        app.notepad.font
    } else {
        notepad::Font {
            sans: !style.mono,
            style: u8::from(style.bold) | (u8::from(style.italic) << 1),
            points: u32::from(style.size).div_ceil(2),
        }
    });
    draft.underline = style.underline;
    draft.strike = style.strike;
    app.menu = false;
    app.common_scroll = (0, 0);
    app.common_dialog = Some(Dialog::Font(draft));
}
pub fn open_print(app: &mut ApplicationSession) {
    let selection = matches!(&app.state, AppState::Document(d) if d.selection().is_some());
    app.menu = false;
    app.common_scroll = (0, 0);
    app.common_dialog = Some(Dialog::Print(PrintDialog::new(selection)));
}
pub fn open_page(app: &mut ApplicationSession) {
    app.menu = false;
    app.common_scroll = (0, 0);
    app.common_dialog = Some(Dialog::Page {
        draft: app.notepad.page.clone(),
        focus: 0,
        select: true,
        error: String::new(),
        popup: PopupMenu::new(alloc::vec![
            PopupItem::new(0, "A4 (210 x 297 mm)"),
            PopupItem::new(1, "Letter (8.5 x 11 in)")
        ]),
    });
}

/// Validate margins in millimeters, including paper rotation and finite values.
pub fn valid_page(page: &notepad::Page) -> bool {
    let mut values = [0.0f32; 4];
    for (value, text) in values.iter_mut().zip(&page.margins) {
        let Ok(parsed) = text.parse::<f32>() else {
            return false;
        };
        if !parsed.is_finite() || parsed < 0.0 {
            return false;
        }
        *value = parsed;
    }
    let (width, height) = if page.letter {
        (215.9, 279.4)
    } else {
        (210.0, 297.0)
    };
    let (width, height) = if page.landscape {
        (height, width)
    } else {
        (width, height)
    };
    values[0] + values[1] < width && values[2] + values[3] < height
}

fn edit(text: &mut String, select: &mut bool, key: u8, decimal: bool) {
    if key == 1 {
        *select = true;
        return;
    }
    if !key.is_ascii_digit() && !matches!(key, 8 | input::DELETE) && !(decimal && key == b'.') {
        return;
    }
    if *select {
        text.clear();
        *select = false;
    }
    if key == 8 {
        text.pop();
    } else if key != input::DELETE && text.len() < 8 {
        text.push(char::from(key));
    }
}

/// Consume a modal keystroke without allowing it to edit the underlying document.
pub fn key(app: &mut ApplicationSession, key: u8) -> AppAction {
    let Some(mut dialog) = app.common_dialog.take() else {
        return AppAction::None;
    };
    if let Dialog::Page { draft, popup, .. } = &mut dialog {
        if popup.is_open() {
            if let Some(action) = popup_key(key) {
                if let PopupResult::Selected(id) = popup.key(action) {
                    draft.letter = id == 1;
                }
            }
            app.common_dialog = Some(dialog);
            return AppAction::None;
        }
    }
    if key == 27 {
        super::guest_dialog::finish(app, &dialog, false);
        if let Dialog::Color { draft, .. } = dialog {
            app.custom_colors = draft.custom;
        }
        return AppAction::None;
    }
    let mut accept = false;
    let effects = app.id != super::AppId::Notepad;
    match &mut dialog {
        Dialog::Color { draft, .. } => accept = draft.key(key),
        Dialog::Font(draft) => match key {
            13 => accept = draft.validate(),
            9 => {
                draft.focus = (draft.focus + 1) % if effects { 5 } else { 3 };
                draft.select = true;
            }
            input::UP | input::DOWN if draft.focus == 0 => draft.font.sans = !draft.font.sans,
            input::UP | input::DOWN if draft.focus == 1 => {
                draft.font.style = (draft.font.style + if key == input::UP { 3 } else { 1 }) % 4;
            }
            b' ' if effects && draft.focus == 3 => draft.underline = !draft.underline,
            b' ' if effects && draft.focus == 4 => draft.strike = !draft.strike,
            _ if draft.focus == 2 => {
                edit(&mut draft.points, &mut draft.select, key, false);
                if let Ok(size @ 1..=144) = draft.points.parse::<u32>() {
                    draft.font.points = size;
                }
            }
            _ => {}
        },
        Dialog::Print(draft) => match key {
            13 => {
                if !draft.valid_options() { draft.error="Enter a valid page range and 1 to 9999 copies.".into(); }
                else if app.guest_dialog.is_some() {accept=true;}
                else {match super::printing::print(app,draft){
                    Ok(job)=>{app.status=alloc::format!("Printed to Documents\\Print-{job:08}.ps");accept=true;},
                    Err(error)=>draft.error=error,
                }}
            }
            9 => {
                draft.focus = (draft.focus + 1) % 7;
                if draft.focus == 4 && !draft.selection_available {
                    draft.focus = 5;
                }
                draft.select = true;
            }
            b' ' if draft.focus == 3 => {
                draft.range = false;
                draft.selection = false;
            }
            b' ' if draft.focus == 4 && draft.selection_available => {
                draft.selection = true;
                draft.range = false;
            }
            b' ' if draft.focus == 5 => {
                draft.range = true;
                draft.selection = false;
            }
            b' ' if draft.focus == 6 => draft.collate = !draft.collate,
            _ if draft.focus >= 3 => {}
            _ => {
                let field = match draft.focus {
                    0 => &mut draft.from,
                    1 => &mut draft.to,
                    _ => &mut draft.copies,
                };
                edit(field, &mut draft.select, key, false);
            }
        },
        Dialog::Page {
            draft,
            focus,
            select,
            error,
            ..
        } => match key {
            13 => {
                accept = valid_page(draft);
                if !accept {
                    *error = "Enter margins inside the paper (millimeters).".into();
                }
            }
            9 => {
                *focus = (*focus + 1) % 9;
                *select = true;
            }
            _ if *focus < 4 => edit(&mut draft.margins[*focus], select, key, true),
            b' ' | input::LEFT | input::RIGHT if *focus == 6 => draft.letter = !draft.letter,
            b' ' | input::UP | input::DOWN if *focus >= 7 => draft.landscape = *focus == 8,
            _ if *focus >= 6 => {}
            _ => {
                let field = if *focus == 4 {
                    &mut draft.header
                } else {
                    &mut draft.footer
                };
                if key == 1 {
                    *select = true;
                } else if key == 8 || (0x20..=0x7e).contains(&key) {
                    if *select {
                        field.clear();
                        *select = false;
                    }
                    if key == 8 {
                        field.pop();
                    } else if field.len() < 256 {
                        field.push(char::from(key));
                    }
                }
            }
        },
    }
    if accept {
        super::guest_dialog::finish(app, &dialog, true);
        match dialog {
            Dialog::Color { draft, target } => {
                app.custom_colors = draft.custom;
                match (&mut app.state, target) {
                    (AppState::Paint(p), ColorTarget::Paint) => p.color = draft.color,
                    (AppState::Document(d), ColorTarget::Text) => {
                        d.format_selection(false, |s| s.color = draft.color)
                    }
                    (AppState::Document(d), ColorTarget::Highlight) => {
                        d.format_selection(false, |s| s.highlight = draft.color)
                    }
                    _ => {}
                }
            }
            Dialog::Font(draft) => {
                if app.id == super::AppId::Notepad {
                    app.notepad.font = draft.font;
                    app.notepad.reveal();
                } else if let AppState::Document(d) = &mut app.state {
                    d.format_selection(false, |s| {
                        s.mono = !draft.font.sans;
                        s.bold = draft.font.style & 1 != 0;
                        s.italic = draft.font.style & 2 != 0;
                        s.size = (draft.font.points * 2) as u16;
                        s.underline = draft.underline;
                        s.strike = draft.strike;
                    });
                }
            }
            Dialog::Page { draft, .. } => app.notepad.page = draft,
            Dialog::Print(_) => {}
        }
    } else {
        app.common_dialog = Some(dialog);
    }
    AppAction::None
}

fn scroll_limits(dialog: &Dialog, body: Rect) -> (u32, u32) {
    let r = bounds(dialog, body);
    let (w, h) = render::desired_size(dialog);
    (w.saturating_sub(r.width), h.saturating_sub(r.height))
}
fn content_rect(app: &ApplicationSession, dialog: &Dialog, body: Rect) -> Rect {
    let r = bounds(dialog, body);
    let (w, h) = render::desired_size(dialog);
    let limits = scroll_limits(dialog, body);
    Rect::new(
        r.x - app.common_scroll.0.min(limits.0) as i32,
        r.y - app.common_scroll.1.min(limits.1) as i32,
        w,
        h,
    )
}
fn reveal_focus(app: &mut ApplicationSession, body: Rect) {
    let Some(dialog) = &app.common_dialog else {
        return;
    };
    let r = bounds(dialog, body);
    let focus = match dialog {
        Dialog::Color { draft, .. } => {
            if draft.focus == 0 {
                color::swatch(draft.selected)
            } else if draft.focus <= 6 {
                color::field(draft.focus - 1)
            } else {
                Rect::new(286, 300, 238, 27)
            }
        }
        Dialog::Font(draft) => match draft.focus {
            0 => Rect::new(14, 90, 200, 48),
            1 => Rect::new(226, 90, 120, 96),
            2 => Rect::new(358, 63, 76, 25),
            3 => Rect::new(14, 207, 150, 24),
            _ => Rect::new(166, 207, 150, 24),
        },
        Dialog::Print(draft) => match draft.focus {
            0..=2 => render::print_field(Rect::new(0, 0, 480, 340), draft.focus),
            3 => Rect::new(22, 178, 125, 24),
            4 => Rect::new(156, 178, 125, 24),
            5 => Rect::new(22, 208, 125, 24),
            _ => Rect::new(302, 246, 125, 24),
        },
        Dialog::Page { focus, .. } => match focus {
            0..=5 => render::page_field(Rect::new(0, 0, 460, 382), *focus),
            6 => Rect::new(18, 62, 190, 26),
            7 => Rect::new(24, 131, 110, 23),
            _ => Rect::new(140, 131, 120, 23),
        },
    };
    let limits = scroll_limits(dialog, body);
    let visible = render::content_clip(r);
    let mut scroll = app.common_scroll;
    for (offset, start, end, size, limit) in [
        (
            &mut scroll.0,
            focus.x,
            focus.x + focus.width as i32,
            visible.width,
            limits.0,
        ),
        (
            &mut scroll.1,
            focus.y - 30,
            focus.y - 30 + focus.height as i32,
            visible.height,
            limits.1,
        ),
    ] {
        if start < *offset as i32 {
            *offset = start.max(0) as u32;
        } else if end > (*offset + size) as i32 {
            *offset = (end - size as i32).max(0) as u32;
        }
        *offset = (*offset).min(limit);
    }
    app.common_scroll = scroll;
}

fn popup_key(key: u8) -> Option<PopupKey> {
    Some(match key {
        input::UP => PopupKey::Up,
        input::DOWN => PopupKey::Down,
        input::HOME => PopupKey::Home,
        input::END => PopupKey::End,
        input::PAGE_UP => PopupKey::PageUp,
        input::PAGE_DOWN => PopupKey::PageDown,
        13 | b' ' => PopupKey::Enter,
        27 | 9 => PopupKey::Escape,
        _ => return None,
    })
}

pub fn key_at(app: &mut ApplicationSession, key: u8, body: Rect) -> AppAction {
    if matches!(key, b' ' | input::DOWN) {
        let content = app
            .common_dialog
            .as_ref()
            .map(|dialog| content_rect(app, dialog, body));
        if let Some(dialog) = &mut app.common_dialog {
            let r = content.unwrap();
            if let Dialog::Page {
                draft,
                focus: 6,
                popup,
                ..
            } = dialog
            {
                if !popup.is_open() {
                    popup.open(
                        Rect::new(r.x + 18, r.y + 62, 190, 26),
                        body,
                        Some(u32::from(draft.letter)),
                    );
                    return AppAction::None;
                }
            }
        }
    }
    let action = self::key(app, key);
    if key == 9 {
        reveal_focus(app, body);
    }
    action
}

pub fn captures_pointer(app: &ApplicationSession) -> bool {
    match &app.common_dialog {
        Some(Dialog::Color { draft, .. }) => draft.dragging(),
        Some(Dialog::Page { popup, .. }) => popup.is_open(),
        _ => false,
    }
}

pub fn pointer_event(
    app: &mut ApplicationSession,
    body: Rect,
    x: i32,
    y: i32,
    down: bool,
    held: bool,
) -> bool {
    if down {
        pointer(app, body, x, y);
        return true;
    }
    let content = app
        .common_dialog
        .as_ref()
        .map(|dialog| content_rect(app, dialog, body));
    if let Some(dialog) = &mut app.common_dialog {
        let r = content.unwrap();
        match dialog {
            Dialog::Color { draft, .. } => return draft.drag_to(x - r.x, y - r.y, held),
            Dialog::Page { popup, .. } if popup.is_open() => {
                popup.pointer(x, y, false);
                return true;
            }
            _ => {}
        }
    }
    false
}

pub fn wheel(app: &mut ApplicationSession, body: Rect, delta: i32) -> bool {
    if let Some(Dialog::Page { popup, .. }) = &mut app.common_dialog {
        if popup.is_open() {
            popup.wheel(delta);
            return true;
        }
    }
    if let Some(dialog) = &app.common_dialog {
        let limits = scroll_limits(dialog, body);
        let old = app.common_scroll;
        let (value, limit) = if limits.1 != 0 {
            (&mut app.common_scroll.1, limits.1)
        } else {
            (&mut app.common_scroll.0, limits.0)
        };
        *value = (i64::from(*value) - i64::from(delta) * 24).clamp(0, i64::from(limit)) as u32;
        return old != app.common_scroll;
    }
    false
}

/// Route a modal click. All geometry is shared with rendering.
pub fn pointer(app: &mut ApplicationSession, body: Rect, x: i32, y: i32) {
    let Some(mut dialog) = app.common_dialog.take() else {
        return;
    };
    let window = bounds(&dialog, body);
    let r = content_rect(app, &dialog, body);
    let local = (x - r.x, y - r.y);
    if let Dialog::Page { draft, popup, .. } = &mut dialog {
        if popup.is_open() {
            if let PopupResult::Selected(id) = popup.pointer(x, y, true) {
                draft.letter = id == 1;
            }
            app.common_dialog = Some(dialog);
            return;
        }
    }
    let cancel = Rect::new(window.x + window.width as i32 - 30, window.y + 4, 25, 21)
        .contains(x, y)
        || render::cancel_button(window).contains(x, y);
    let accept = render::ok_button(window).contains(x, y);
    if cancel || accept {
        app.common_dialog = Some(dialog);
        if cancel {
            key(app, 27);
        } else {
            key(app, 13);
        }
        return;
    }
    let limits = scroll_limits(&dialog, body);
    for index in 0..4 {
        let limit = if index < 2 { limits.0 } else { limits.1 };
        if limit != 0 && render::scroll_button(window, index).contains(x, y) {
            let offset = if index < 2 {
                &mut app.common_scroll.0
            } else {
                &mut app.common_scroll.1
            };
            *offset = if index % 2 == 0 {
                offset.saturating_sub(48)
            } else {
                offset.saturating_add(48).min(limit)
            };
            app.common_dialog = Some(dialog);
            return;
        }
    }
    if !render::content_clip(window).contains(x, y) {
        app.common_dialog = Some(dialog);
        return;
    }
    let effects = app.id != super::AppId::Notepad;
    match &mut dialog {
        Dialog::Color { draft, .. } => draft.pointer(local.0, local.1),
        Dialog::Font(draft) => {
            for i in 0..2 {
                if Rect::new(14, 90 + i * 24, 200, 24).contains(local.0, local.1) {
                    draft.font.sans = i == 1;
                    draft.focus = 0;
                }
            }
            for i in 0..4 {
                if Rect::new(226, 90 + i * 24, 120, 24).contains(local.0, local.1) {
                    draft.font.style = i as u8;
                    draft.focus = 1;
                }
            }
            if Rect::new(358, 63, 76, 25).contains(local.0, local.1) {
                draft.focus = 2;
                draft.select = true;
            }
            for (i, size) in [8, 9, 10, 11, 12, 14, 16, 18, 20, 24].iter().enumerate() {
                if Rect::new(358, 90 + i as i32 * 20, 76, 20).contains(local.0, local.1) {
                    draft.font.points = *size;
                    draft.points = size.to_string();
                    draft.focus = 2;
                    draft.select = true;
                }
            }
            if effects && Rect::new(14, 207, 150, 24).contains(local.0, local.1) {
                draft.underline = !draft.underline;
                draft.focus = 3;
            }
            if effects && Rect::new(166, 207, 150, 24).contains(local.0, local.1) {
                draft.strike = !draft.strike;
                draft.focus = 4;
            }
        }
        Dialog::Print(draft) => {
            for i in 0..3 {
                if render::print_field(r, i).contains(x, y) {
                    draft.focus = i;
                    draft.select = true;
                }
            }
            if Rect::new(22, 178, 125, 24).contains(local.0, local.1) {
                draft.range = false;
                draft.selection = false;
                draft.focus = 3;
            }
            if draft.selection_available && Rect::new(156, 178, 125, 24).contains(local.0, local.1)
            {
                draft.selection = true;
                draft.range = false;
                draft.focus = 4;
            }
            if Rect::new(22, 208, 125, 24).contains(local.0, local.1) {
                draft.range = true;
                draft.selection = false;
                draft.focus = 5;
            }
            if Rect::new(302, 246, 125, 24).contains(local.0, local.1) {
                draft.collate = !draft.collate;
                draft.focus = 6;
            }
        }
        Dialog::Page {
            draft,
            focus,
            select,
            popup,
            ..
        } => {
            if Rect::new(18, 62, 190, 26).contains(local.0, local.1) {
                *focus = 6;
                popup.open(
                    Rect::new(r.x + 18, r.y + 62, 190, 26),
                    body,
                    Some(u32::from(draft.letter)),
                );
            }
            if Rect::new(24, 131, 110, 23).contains(local.0, local.1) {
                draft.landscape = false;
                *focus = 7;
            }
            if Rect::new(140, 131, 120, 23).contains(local.0, local.1) {
                draft.landscape = true;
                *focus = 8;
            }
            for i in 0..6 {
                if render::page_field(r, i).contains(x, y) {
                    *focus = i;
                    *select = true;
                }
            }
        }
    }
    app.common_dialog = Some(dialog);
}

#[cfg(test)]
mod tests {
    use super::super::{wordpad, AppId};
    use super::*;
    fn paint_color(app: &ApplicationSession) -> u32 {
        let AppState::Paint(p) = &app.state else {
            panic!("paint");
        };
        p.color
    }
    #[test]
    fn color_pointer_accept_cancel_and_custom_colors_are_transactional() {
        let mut app = ApplicationSession::new(1, AppId::Paint, 0);
        let original = paint_color(&app);
        open_color(&mut app, ColorTarget::Paint);
        let body = Rect::new(0, 0, 640, 480);
        let r = bounds(app.common_dialog.as_ref().unwrap(), body);
        let swatch = color::swatch(8);
        pointer(&mut app, body, r.x + swatch.x + 3, r.y + swatch.y + 3);
        assert_eq!(paint_color(&app), original);
        key(&mut app, 27);
        assert_eq!(paint_color(&app), original);
        open_color(&mut app, ColorTarget::Paint);
        pointer(&mut app, body, r.x + swatch.x + 3, r.y + swatch.y + 3);
        pointer(&mut app, body, r.x + 290, r.y + 308);
        key(&mut app, 13);
        assert_eq!(paint_color(&app), 0xffff0000);
        assert_eq!(app.custom_colors[0], 0xffff0000);
        assert!(app.common_dialog.is_none());
        open_color(&mut app, ColorTarget::Paint);
        let Some(Dialog::Color { draft, .. }) = &mut app.common_dialog else {
            panic!("color");
        };
        draft.focus = 4;
        draft.fields[3] = "256".into();
        key(&mut app, 13);
        assert!(app.common_dialog.is_some());
        assert_eq!(paint_color(&app), 0xffff0000);
    }
    #[test]
    fn spectrum_retains_hue_at_black_and_drag_capture_clamps_to_edges() {
        let mut app = ApplicationSession::new(1, AppId::Paint, 0);
        open_color(&mut app, ColorTarget::Paint);
        let body = Rect::new(0, 0, 640, 480);
        let r = bounds(app.common_dialog.as_ref().unwrap(), body);
        pointer_event(&mut app, body, r.x + 286, r.y + 58, true, true);
        assert!(captures_pointer(&app));
        pointer_event(&mut app, body, r.x + 358, r.y + 58, false, true);
        pointer_event(&mut app, body, r.x + 358, r.y + 58, false, false);
        assert!(!captures_pointer(&app));
        pointer_event(&mut app, body, r.x + 516, r.y + 124, true, true);
        pointer_event(&mut app, body, -50, r.y + 124, false, true);
        pointer_event(&mut app, body, -50, r.y + 124, false, false);
        let Some(Dialog::Color { draft, .. }) = &app.common_dialog else {
            panic!("color");
        };
        assert!(
            draft.color & 0xff00 > 0xf000,
            "green hue survives the initial black color"
        );
        assert!(draft.color & 0xff0000 < 0x100000);
        key(&mut app, 13);
        assert!(app.common_dialog.is_none());
    }
    #[test]
    fn paper_dropdown_escape_cancels_only_popup_and_small_dialog_scroll_keeps_controls_reachable() {
        let mut app = ApplicationSession::new(1, AppId::Notepad, 0);
        open_page(&mut app);
        let body = Rect::new(0, 0, 640, 480);
        let r = bounds(app.common_dialog.as_ref().unwrap(), body);
        pointer(&mut app, body, r.x + 30, r.y + 70);
        assert!(captures_pointer(&app));
        key(&mut app, input::DOWN);
        key(&mut app, 27);
        let Some(Dialog::Page { draft, .. }) = &app.common_dialog else {
            panic!("page");
        };
        assert!(!draft.letter);
        key_at(&mut app, b' ', body);
        key(&mut app, input::END);
        key(&mut app, 13);
        let Some(Dialog::Page { draft, .. }) = &app.common_dialog else {
            panic!("page");
        };
        assert!(draft.letter);
        assert!(!app.notepad.page.letter);
        key(&mut app, 27);
        open_page(&mut app);
        let small = Rect::new(0, 0, 320, 240);
        for _ in 0..5 {
            key_at(&mut app, 9, small);
        }
        assert!(app.common_scroll.1 > 0);
        let r = content_rect(&app, app.common_dialog.as_ref().unwrap(), small);
        let field = render::page_field(r, 5);
        assert!(
            render::content_clip(bounds(app.common_dialog.as_ref().unwrap(), small))
                .intersection(&field)
                .is_some()
        );
        key_at(&mut app, b'x', small);
        let mut frame = Surface::new(320, 240);
        let damage = Rect::new(10, 10, 200, 180);
        frame.set_draw_clip(Some(damage));
        draw(&app, &mut frame, small, &|_, _, _, _| {});
        assert_eq!(frame.drawing_bounds(), damage);
        let window = bounds(app.common_dialog.as_ref().unwrap(), small);
        let ok = render::ok_button(window);
        pointer(&mut app, small, ok.x + 3, ok.y + 3);
        assert!(app.common_dialog.is_none());
        assert_eq!(app.notepad.page.footer, "x");
    }
    #[test]
    fn color_hsl_uses_windows_ranges_and_spectrum_returns_primary_colors() {
        assert_eq!(color::hsl_to_rgb(0, 240, 120), 0xffff0000);
        assert_eq!(color::hsl_to_rgb(80, 240, 120), 0xff00ff00);
        assert_eq!(color::hsl_to_rgb(160, 240, 120), 0xff0000ff);
        assert_eq!(color::rgb_to_hsl(0xffff0000), (0, 240, 120));
        assert_eq!(color::hsl_to_rgb(0, 0, 0), 0xff000000);
        assert_eq!(color::hsl_to_rgb(0, 0, 240), 0xffffffff);
    }
    #[test]
    fn font_validation_and_wordpad_format_commit_cancel_undo() {
        let mut app = ApplicationSession::new(1, AppId::WordPad, 0);
        app.paste("formatted");
        let AppState::Document(d) = &mut app.state else {
            panic!("document");
        };
        d.select_all();
        let original = d.current_style();
        wordpad::execute(&mut app, wordpad::Command::FontDialog);
        let Some(Dialog::Font(draft)) = &mut app.common_dialog else {
            panic!("font");
        };
        draft.points = "0".into();
        key(&mut app, 13);
        assert!(app.common_dialog.is_some());
        let Some(Dialog::Font(draft)) = &mut app.common_dialog else {
            panic!("font");
        };
        draft.points = "27".into();
        draft.font.style = 3;
        draft.underline = true;
        key(&mut app, 13);
        let AppState::Document(d) = &mut app.state else {
            panic!("document");
        };
        assert_eq!(d.current_style().size, 54);
        assert!(d.current_style().bold && d.current_style().italic && d.current_style().underline);
        d.undo();
        assert_eq!(d.current_style(), original);
        wordpad::execute(&mut app, wordpad::Command::MoreColors(false));
        app.key(input::RIGHT);
        app.key(27);
        let AppState::Document(d) = &app.state else {
            panic!("document");
        };
        assert_eq!(d.current_style(), original);
    }
    #[test]
    fn page_validation_cancel_and_landscape_commit_use_same_production_route() {
        let mut app = ApplicationSession::new(1, AppId::Notepad, 0);
        notepad::execute(&mut app, notepad::Command::Page);
        let Some(Dialog::Page { draft, .. }) = &mut app.common_dialog else {
            panic!("page");
        };
        draft.margins[0] = "NaN".into();
        app.key(13);
        assert!(app.common_dialog.is_some());
        app.key(27);
        assert_eq!(app.notepad.page, notepad::Page::default());
        notepad::execute(&mut app, notepad::Command::Page);
        let body = Rect::new(0, 0, 640, 480);
        let r = bounds(app.common_dialog.as_ref().unwrap(), body);
        pointer(&mut app, body, r.x + 145, r.y + 137);
        app.key(13);
        assert!(app.notepad.page.landscape);
        assert!(app.common_dialog.is_none());
    }
    #[test]
    fn print_backend_failure_stays_modal_and_cannot_claim_success() {
        let mut app = ApplicationSession::new(1, AppId::Notepad, 0);
        app.key(16);
        app.key(b'9');
        let AppState::Document(d) = &app.state else {
            panic!("document");
        };
        assert_eq!(d.editor.text, "");
        app.key(13);
        let Some(Dialog::Print(draft)) = &app.common_dialog else {
            panic!("print");
        };
        assert!(draft.error.contains("Printing failed"));
        let mut options = draft.clone();
        options.range = true;
        options.from = "9".into();
        options.to = "2".into();
        assert!(!options.valid_options());
        options.to = "10".into();
        assert!(options.valid_options());
        options.copies = "0".into();
        assert!(!options.valid_options());
        app.key(27);
        assert!(app.common_dialog.is_none());
    }
    #[test]
    fn shared_dialogs_render_real_controls_and_preview_without_mutating_state() {
        let mut app = ApplicationSession::new(1, AppId::WordPad, 0);
        let body = Rect::new(0, 0, 640, 480);
        for mode in 0..4 {
            match mode {
                0 => open_color(&mut app, ColorTarget::Text),
                1 => open_font(&mut app),
                2 => open_print(&mut app),
                _ => open_page(&mut app),
            }
            let before = app.common_dialog.clone();
            let mut surface = Surface::new(640, 480);
            let labels = core::cell::RefCell::new(alloc::vec::Vec::new());
            draw(&app, &mut surface, body, &|_, label, _, _| {
                labels.borrow_mut().push(label.to_string())
            });
            assert_eq!(app.common_dialog, before);
            assert!(surface.pixels().iter().any(|v| *v == 0xfff0f0f0));
            assert!(labels.borrow().iter().any(|v| v == "Cancel"));
        }
    }
}
