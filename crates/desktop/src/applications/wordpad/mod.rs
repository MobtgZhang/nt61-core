//! Local WordPad: shared document/VFS/font services, a ribbon and rich text viewport.
mod commands;
pub mod formats;
mod layout;
mod pointer;
mod ribbon;
use super::{
    document::Document,
    rich_text::Style,
    view::{Button, Command as Ui},
    AppAction, AppState, ApplicationSession,
};
use crate::graphics::{font::FontLib, Rect, Surface};
use alloc::{format, rc::Rc, vec::Vec};
pub use commands::{execute, key};
use core::cell::RefCell;
use layout::*;
pub use pointer::pointer;
pub use ribbon::buttons;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Tab(bool),
    Bold,
    Italic,
    Underline,
    Strike,
    Family,
    FontMenu,
    FontDialog,
    MoreColors(bool),
    PageSetup,
    SizeMenu,
    ColorMenu,
    HighlightMenu,
    SetFamily(bool),
    SetSize(u16),
    SetColor(u32),
    SetHighlight(u32),
    Size(i16),
    Color,
    Highlight,
    Align(u8),
    Indent(i16),
    Bullet,
    Zoom(i16),
    Ruler,
    Status,
    Wrap,
    SelectAll,
    Time,
    About,
    Print,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Cell {
    index: usize,
    ch: char,
    x: u32,
    width: u32,
    style: Style,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Line {
    start: usize,
    end: usize,
    y: u32,
    height: u32,
    width: u32,
    cells: Vec<Cell>,
    bullet: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Cache {
    revision: u64,
    width: u32,
    zoom: u16,
    wrap: bool,
    lines: Vec<Line>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Popup {
    Font,
    Size,
    Color,
    Highlight,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WordPad {
    pub view: bool,
    pub zoom: u16,
    pub ruler: bool,
    pub status: bool,
    pub top: u32,
    pub manual_scroll: bool,
    pub popup: Option<Popup>,
    scroll_drag: Option<i32>,
    zoom_drag: bool,
    cache: RefCell<Cache>,
}
impl Default for WordPad {
    fn default() -> Self {
        Self {
            view: false,
            zoom: 100,
            ruler: true,
            status: true,
            top: 0,
            manual_scroll: false,
            popup: None,
            scroll_drag: None,
            zoom_drag: false,
            cache: RefCell::new(Cache::default()),
        }
    }
}
pub fn draw<F: Fn(&mut Surface, &str, Rect, u32)>(
    app: &ApplicationSession,
    frame: &mut Surface,
    b: Rect,
    text: &F,
) {
    let AppState::Document(d) = &app.state else {
        return;
    };
    let w = &app.wordpad;
    frame.fill_rect(b.x, b.y, b.width, b.height, 0xffc8d4e2);
    frame.fill_rect(b.x, b.y, b.width, 29, 0xffedf3f9);
    let r = viewport(b, w);
    let p = page(r, w.zoom);
    let m = margin(p, w.zoom);
    frame.fill_rect(p.x + 3, p.y, p.width, r.height, 0xff899caf);
    frame.fill_rect(p.x, p.y, p.width, r.height, 0xffffffff);
    if w.ruler {
        frame.fill_rect(p.x, b.y + 131, p.width, 18, 0xfff9fafb);
        for i in 0..=20 {
            let x = p.x + m as i32 + (i * 32 * u32::from(w.zoom) / 100) as i32;
            if x < p.x + p.width as i32 - m as i32 {
                frame.fill_rect(x, b.y + 143, 1, if i % 2 == 0 { 6 } else { 3 }, 0xff6c7782);
                if i % 2 == 0 {
                    text(
                        frame,
                        &format!("{}", i / 2),
                        Rect::new(x + 2, b.y + 131, 20, 13),
                        0xff697587,
                    );
                }
            }
        }
    }
    let mut paper = Surface::new(p.width.max(1), r.height.max(1));
    paper.fill_rect(0, 0, p.width, r.height, 0xffffffff);
    with_lines(app, d, p.width.saturating_sub(m * 2).max(1), |lines| {
        let top = top(app, d, lines, r.height);
        let selected = d.selection();
        for l in lines
            .iter()
            .filter(|l| l.y + l.height >= top && l.y < top + r.height)
        {
            let y = l.y as i32 - top as i32;
            if l.bullet {
                if let Some(first) = l.cells.first() {
                    if let Some(face) = font(app, first.style) {
                        let px = pixels(first.style, w.zoom);
                        let _ = face.draw_text(
                            &mut paper,
                            "•",
                            m as i32 + first.x as i32 - px as i32,
                            y,
                            px,
                            first.style.color,
                        );
                    }
                }
            }
            for c in &l.cells {
                let x = m as i32 + c.x as i32;
                if x >= p.width as i32 {
                    continue;
                }
                let sel = selected.is_some_and(|(a, b)| c.index >= a && c.index < b);
                let color = if sel { 0xffffffff } else { c.style.color };
                if sel || c.style.highlight != 0 {
                    paper.fill_rect(
                        x,
                        y,
                        c.width,
                        l.height,
                        if sel { 0xff3399ff } else { c.style.highlight },
                    );
                }
                let px = pixels(c.style, w.zoom);
                if c.ch != '\t' {
                    if let Some(f) = font(app, c.style) {
                        let mut bytes = [0; 4];
                        let ch = if f.has_glyph(c.ch) { c.ch } else { '□' };
                        let _ =
                            f.draw_text(&mut paper, ch.encode_utf8(&mut bytes), x, y, px, color);
                    }
                }
                if c.style.underline {
                    paper.fill_rect(x, y + px as i32 + 2, c.width, 1, color);
                }
                if c.style.strike {
                    paper.fill_rect(x, y + px as i32 / 2 + 2, c.width, 1, color);
                }
            }
        }
        if app.dialog.is_none() && !app.confirm && app.search.is_none() {
            let (row, x) = caret(lines, d.editor.cursor);
            let l = &lines[row];
            paper.fill_rect(
                m as i32 + x as i32,
                l.y as i32 - top as i32,
                1,
                l.height,
                0xff000000,
            );
        }
    });
    frame.copy_region(&paper, Rect::new(0, 0, p.width, r.height), p.x, p.y);
    frame.fill_rect(b.x + b.width as i32 - 16, r.y, 15, r.height, 0xffedf0f4);
    pointer::draw_scroll_thumb(app, d, frame, b);
    text(
        frame,
        "▲",
        Rect::new(b.x + b.width as i32 - 15, r.y, 15, 20),
        0xff51657b,
    );
    text(
        frame,
        "▼",
        Rect::new(
            b.x + b.width as i32 - 15,
            r.y + r.height as i32 - 20,
            15,
            20,
        ),
        0xff51657b,
    );
    if w.status {
        frame.fill_rect(b.x, b.y + b.height as i32 - 24, b.width, 24, 0xffedf3f9);
        text(
            frame,
            if app.status.is_empty() {
                "Ready"
            } else {
                &app.status
            },
            Rect::new(
                b.x + 8,
                b.y + b.height as i32 - 21,
                b.width.saturating_sub(230),
                20,
            ),
            0xff43566b,
        );
    }
    ribbon::draw(app, frame, b, text);
    if let Some(s) = &app.search {
        super::editor_view::search_draw(s, frame, b, text);
    }
    if let Some(dialog) = &app.dialog {
        super::file_dialog::draw(dialog, frame, b, text);
    }
    super::common_dialogs::draw(app, frame, b, text);
    if app.confirm {
        frame.fill_rect(
            b.x + 8,
            b.y + 40,
            b.width.saturating_sub(16),
            110,
            0xfff0f0f0,
        );
        text(
            frame,
            "Save changes to this document?",
            Rect::new(b.x + 18, b.y + 65, b.width.saturating_sub(36), 30),
            0xff172c44,
        );
        for (r, label) in
            super::view::confirm_buttons(b)
                .into_iter()
                .zip(["Save", "Don't save", "Cancel"])
        {
            crate::compositor::aero::menu_item(frame, r, false);
            text(
                frame,
                label,
                Rect::new(r.x + 6, r.y + 5, r.width - 12, 22),
                0xff172c44,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::AppId;
    use super::*;
    #[test]
    fn selection_formatting_undo_redo_and_typing_are_independent() {
        let mut app = ApplicationSession::new(1, AppId::WordPad, 0);
        app.paste("normal bold 中😀");
        let AppState::Document(d) = &mut app.state else {
            panic!()
        };
        d.anchor = Some(7);
        d.editor.cursor = 11;
        d.mark_saved();
        execute(&mut app, Command::Bold);
        let AppState::Document(d) = &mut app.state else {
            panic!()
        };
        let r = d.rich.as_ref().unwrap();
        assert!(!r.style(0).bold);
        assert!(r.style(7).bold);
        assert!(!r.style(12).bold);
        assert!(d.dirty);
        d.undo();
        assert!(!d.dirty);
        assert!(!d.rich.as_ref().unwrap().style(7).bold);
        d.redo();
        assert!(d.rich.as_ref().unwrap().style(7).bold);
        d.insert("X");
        assert_eq!(d.editor.text, "normal X 中😀");
        assert_eq!(
            d.rich.as_ref().unwrap().styles.len(),
            d.editor.text.chars().count()
        );
        assert!(d.rich.as_ref().unwrap().style(7).bold);
        d.undo();
        assert_eq!(d.editor.text, "normal bold 中😀");
        d.key(crate::applications::explorer::input::DELETE);
        d.key(8);
        assert_eq!(
            d.rich.as_ref().unwrap().styles.len(),
            d.editor.text.chars().count()
        );
        d.replace_all("normal", "new", true);
        assert_eq!(
            d.rich.as_ref().unwrap().styles.len(),
            d.editor.text.chars().count()
        );
    }
    #[test]
    fn format_without_selection_controls_new_text_and_page_layout_hit_test() {
        let mut app = ApplicationSession::new(1, AppId::WordPad, 0);
        execute(&mut app, Command::Bold);
        execute(&mut app, Command::Bold);
        app.paste("plain");
        execute(&mut app, Command::Italic);
        app.paste(" italic");
        let AppState::Document(d) = &app.state else {
            panic!()
        };
        assert!(!d.rich.as_ref().unwrap().style(0).bold);
        assert!(d.rich.as_ref().unwrap().style(6).italic);
        let lines = layout(&app, d, 300);
        assert_eq!(lines.len(), 1);
        let last = lines[0].cells.last().unwrap();
        let b = Rect::new(0, 0, 788, 564);
        let r = viewport(b, &app.wordpad);
        let p = page(r, 100);
        let x = p.x + margin(p, 100) as i32 + last.x as i32;
        assert!(pointer(&mut app, b, x, r.y + 18, true, true));
        let AppState::Document(d) = &app.state else {
            panic!()
        };
        assert_eq!(d.editor.cursor, last.index);
    }
}

impl WordPad {
    pub(super) fn dragging_controls(&self) -> bool {
        self.scroll_drag.is_some() || self.zoom_drag
    }

    pub(super) fn release_pointer(&mut self) {
        self.scroll_drag = None;
        self.zoom_drag = false;
    }

    pub fn reset_document(&mut self) {
        self.top = 0;
        self.manual_scroll = false;
        self.popup = None;
        self.scroll_drag = None;
        self.zoom_drag = false;
        *self.cache.get_mut() = Cache::default();
    }
}

#[cfg(test)]
mod ribbon_regressions {
    use super::*;
    use crate::{applications::AppId, shell::ShellModel, theme::Theme};

    #[test]
    fn scroll_and_zoom_drags_preserve_selection_and_release_over_ribbon() {
        let mut shell = ShellModel::new(Theme::aero());
        let id = shell.launch_application(AppId::WordPad);
        let body = shell.application_body(id).unwrap();
        let app = shell
            .applications
            .iter_mut()
            .find(|a| a.window_id == id)
            .unwrap();
        app.paste(&"line of text\n".repeat(80));
        let AppState::Document(d) = &mut app.state else {
            panic!()
        };
        d.anchor = Some(0);
        d.editor.cursor = 4;
        // Grab the thumb at the top, then drag it to the bottom.
        let r = viewport(body, &app.wordpad);
        let x = body.x + body.width as i32 - 10;
        shell.application_pointer(id, x, r.y + 25, true, true, false);
        shell.application_pointer(id, x, r.y + r.height as i32 - 21, false, true, false);
        let app = shell.application(id).unwrap();
        assert!(app.wordpad.top > r.height);
        assert!(app.wordpad.manual_scroll);
        assert!(app.wordpad.dragging_controls());
        shell.application_pointer(id, x, r.y + r.height as i32 - 21, false, false, false);
        let x = body.x + body.width as i32 - 80;
        let y = body.y + body.height as i32 - 12;
        shell.application_pointer(id, x, y, true, true, false);
        // Releasing over a ribbon control must still end the captured gesture.
        shell.application_pointer(id, body.x + 150, body.y + 80, false, false, false);
        let app = shell.application(id).unwrap();
        assert!(!app.wordpad.dragging_controls());
        assert!(app.wordpad.zoom > 100);
        let AppState::Document(d) = &app.state else {
            panic!()
        };
        assert_eq!(d.selection(), Some((0, 4)));
    }

    #[test]
    fn size_popup_over_document_preserves_selection_and_applies_clicked_value() {
        let mut shell = ShellModel::new(Theme::aero());
        let id = shell.launch_application(AppId::WordPad);
        let body = shell.application_body(id).unwrap();
        let app = shell
            .applications
            .iter_mut()
            .find(|a| a.window_id == id)
            .unwrap();
        app.paste("selected text");
        let AppState::Document(d) = &mut app.state else {
            panic!()
        };
        d.select_all();
        execute(app, Command::SizeMenu);
        // 36 pt is below the ribbon, over the editor; it must not reposition the caret.
        let button = buttons(shell.application(id).unwrap(), body)
            .into_iter()
            .find(|b| b.command == Ui::WordPad(Command::SetSize(72)))
            .unwrap();
        assert!(button.bounds.y > body.y + 153);
        assert!(shell.application_pointer(
            id,
            button.bounds.x + 4,
            button.bounds.y + 4,
            true,
            true,
            false
        ));
        let app = shell.application(id).unwrap();
        assert!(app.wordpad.popup.is_none());
        let AppState::Document(d) = &app.state else {
            panic!()
        };
        assert_eq!(d.selection(), Some((0, 13)));
        assert!(d.rich.as_ref().unwrap().styles.iter().all(|s| s.size == 72));
    }

    #[test]
    fn bullets_are_paragraph_properties_and_survive_both_rich_formats() {
        let mut app = ApplicationSession::new(1, AppId::WordPad, 0);
        app.paste("first\nsecond\nthird");
        let AppState::Document(d) = &mut app.state else {
            panic!()
        };
        d.anchor = Some(0);
        d.editor.cursor = 6;
        execute(&mut app, Command::Bullet);
        let AppState::Document(d) = &app.state else {
            panic!()
        };
        let rich = d.rich.as_ref().unwrap();
        assert!(rich.style(0).bullet);
        assert!(rich.style(5).bullet);
        assert!(!rich.style(6).bullet);
        assert_eq!(d.editor.text, "first\nsecond\nthird");
        let lines = layout(&app, d, 300);
        assert!(lines[0].bullet);
        assert!(!lines[1].bullet);
        for format in [formats::Format::Rtf, formats::Format::Docx] {
            let bytes = rich.encode(&d.editor.text, format).unwrap();
            let loaded = formats::decode(&bytes).unwrap();
            assert_eq!(loaded.text, d.editor.text);
            assert_eq!(loaded.rich.styles, rich.styles);
        }
    }

    #[test]
    fn new_document_discards_cached_text_even_with_same_revision() {
        let mut app = ApplicationSession::new(1, AppId::WordPad, 0);
        app.paste("old text");
        let AppState::Document(d) = &app.state else {
            panic!()
        };
        with_lines(&app, d, 300, |lines| assert_eq!(lines[0].cells[0].ch, 'o'));
        let revision = d.revision;
        app.state = ApplicationSession::new(1, AppId::WordPad, 0).state;
        let AppState::Document(d) = &mut app.state else {
            panic!()
        };
        d.revision = revision;
        app.wordpad.reset_document();
        let AppState::Document(d) = &app.state else {
            panic!()
        };
        with_lines(&app, d, 300, |lines| assert!(lines[0].cells.is_empty()));
    }
}
