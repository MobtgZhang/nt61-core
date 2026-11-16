//! Windows 7 style Notepad. Document editing and VFS writes remain shared with the native host.
//! Notepatch is a behavior reference; this implementation and its artwork are original.
use super::{document::Document, explorer::input, AppAction, AppState, ApplicationSession};
use crate::graphics::{
    font::{FontLib, FontLibrary},
    Rect, Surface,
};
use alloc::{
    format,
    rc::Rc,
    string::{String, ToString},
    vec::Vec,
};
use core::cell::RefCell;
mod commands;
mod dialogs;
mod menus;
mod pointer;
mod render;
use commands::*;
pub use commands::{execute, key};
pub use menus::{enabled, menu, menu_bar, menu_rects, popup_rects};
pub use pointer::pointer;
pub use render::{confirm_buttons, draw};
mod layout;
mod ui;
use dialogs::*;
pub use layout::editor_bounds;
use layout::*;
pub use ui::dialog_rect;
use ui::*;
const INK: u32 = 0xFF000000;
const FACE: u32 = 0xFFF0F0F0;
const GRAY: u32 = 0xFF6D6D6D;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Font {
    pub sans: bool,
    pub style: u8,
    pub points: u32,
}
impl Default for Font {
    fn default() -> Self {
        Self {
            sans: false,
            style: 0,
            points: 10,
        }
    }
}
impl Font {
    fn pixels(self) -> u32 {
        (self.points * 4).div_ceil(3)
    }
    fn name(self) -> &'static str {
        if self.sans {
            "DejaVu Sans"
        } else {
            "DejaVu Sans Mono"
        }
    }
    fn style_name(self) -> &'static str {
        ["Regular", "Bold", "Oblique", "Bold Oblique"][self.style as usize % 4]
    }
    pub(crate) fn bytes(self) -> &'static [u8] {
        match (self.sans, self.style) {
            (false, 0) => crate::graphics::font::MONOSPACE_FONT,
            (true, 0) => crate::graphics::font::UI_FONT,
            (false, 1) => include_bytes!("../shared/fonts/DejaVuSansMono-Bold.ttf"),
            (false, 2) => {
                include_bytes!("../shared/fonts/DejaVuSansMono-Oblique.ttf")
            }
            (false, _) => {
                include_bytes!("../shared/fonts/DejaVuSansMono-BoldOblique.ttf")
            }
            (true, 1) => include_bytes!("../shared/fonts/DejaVuSans-Bold.ttf"),
            (true, 2) => include_bytes!("../shared/fonts/DejaVuSans-Oblique.ttf"),
            (true, _) => {
                include_bytes!("../shared/fonts/DejaVuSans-BoldOblique.ttf")
            }
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Search {
    pub query: String,
    pub replacement: String,
    pub replace: bool,
    pub down: bool,
    pub case: bool,
    pub focus: usize,
    pub select: bool,
    pub active: bool,
    pub focused: bool,
    pub message: String,
}
impl Default for Search {
    fn default() -> Self {
        Self {
            query: String::new(),
            replacement: String::new(),
            replace: false,
            down: true,
            case: false,
            focus: 0,
            select: true,
            active: false,
            focused: true,
            message: String::new(),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Page {
    pub landscape: bool,
    pub letter: bool,
    pub margins: [String; 4],
    pub header: String,
    pub footer: String,
}
impl Default for Page {
    fn default() -> Self {
        Self {
            landscape: false,
            letter: false,
            margins: ["20".into(), "20".into(), "25".into(), "25".into()],
            header: "&f".into(),
            footer: "Page &p".into(),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Dialog {
    GoTo {
        line: String,
        select: bool,
        error: String,
    },
    Font(Font),
    Page {
        draft: Page,
        focus: usize,
        select: bool,
        error: String,
    },
    About,
    Help,
    Print,
    Message(String),
}
#[derive(Clone, Debug, PartialEq)]
pub struct Notepad {
    pub status_bar: bool,
    pub font: Font,
    pub search: Search,
    pub dialog: Option<Dialog>,
    pub page: Page,
    pub top: usize,
    pub left: u32,
    pub follow: bool,
    pub menu_index: usize,
    pub clipboard: bool,
    pub scroll_drag: Option<bool>,
    pub last_click: Option<(u64, usize)>,
    pub context: Option<(i32, i32)>,
    pub editor_size: (u32, u32),
    // A wrap boundary has two visual positions with the same text index.
    visual_end: Option<usize>,
    pub(crate) fonts: FontLibrary,
    layout_cache: LayoutCache,
}
impl Default for Notepad {
    fn default() -> Self {
        Self {
            status_bar: false,
            font: Font::default(),
            search: Search::default(),
            dialog: None,
            page: Page::default(),
            top: 0,
            left: 0,
            follow: true,
            menu_index: 0,
            clipboard: false,
            scroll_drag: None,
            last_click: None,
            context: None,
            editor_size: (606, 385),
            visual_end: None,
            fonts: FontLibrary::default(),
            layout_cache: LayoutCache::default(),
        }
    }
}
impl Notepad {
    fn renderer(&self) -> Option<Rc<FontLib>> {
        self.fonts.get(self.font.bytes()).ok()
    }
    pub fn reveal(&mut self) {
        self.visual_end = None;
        self.follow = true;
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    New,
    Open,
    Save,
    SaveAs,
    Page,
    Print,
    Exit,
    Undo,
    Cut,
    Copy,
    Paste,
    Delete,
    Find,
    Next,
    Replace,
    GoTo,
    SelectAll,
    Time,
    Wrap,
    Font,
    Status,
    Help,
    About,
}
#[derive(Clone, Copy)]
pub struct Item {
    pub label: &'static str,
    pub shortcut: &'static str,
    pub command: Option<Command>,
}
const fn item(label: &'static str, shortcut: &'static str, command: Command) -> Item {
    Item {
        label,
        shortcut,
        command: Some(command),
    }
}
const SEP: Item = Item {
    label: "",
    shortcut: "",
    command: None,
};
#[cfg(test)]
mod tests {
    use super::super::{document::Encoding, AppId};
    use super::*;
    #[test]
    fn font_preview_repaints_and_size_changes_reuse_parsed_face() {
        let mut app = ApplicationSession::new(1, AppId::Notepad, 0);
        let before = app.notepad.renderer().unwrap();
        execute(&mut app, Command::Font);
        let mut frame = Surface::new(640, 460);
        let bounds = Rect::new(0, 0, 640, 460);
        for _ in 0..4 {
            draw(&app, &mut frame, bounds, &|_, _, _, _| {});
        }
        assert!(Rc::ptr_eq(&before, &app.notepad.renderer().unwrap()));
        if let Some(crate::applications::common_dialogs::Dialog::Font(draft)) = &mut app.common_dialog {
            draft.points = "20".into();
        }
        draw(&app, &mut frame, bounds, &|_, _, _, _| {});
        key(&mut app, 13);
        assert_eq!(app.notepad.font.points, 20);
        assert!(Rc::ptr_eq(&before, &app.notepad.renderer().unwrap()));
    }
    #[test]
    fn layout_reuses_unchanged_documents_and_invalidates_each_input() {
        let mut n = Notepad::default();
        let font = n.renderer().unwrap();
        let mut d = Document::new("large.txt");
        d.insert(&"line café\n".repeat(2048));
        let first = n.lines(&d, &font, 200);
        d.move_to(1, 2);
        assert!(Rc::ptr_eq(&first, &n.lines(&d, &font, 200)));
        d.insert("x");
        let edited = n.lines(&d, &font, 200);
        assert!(!Rc::ptr_eq(&first, &edited));
        d.undo();
        let undone = n.lines(&d, &font, 200);
        assert_eq!(*undone, *first);
        d.load(b"new file", false).unwrap();
        assert!(!Rc::ptr_eq(&undone, &n.lines(&d, &font, 200)));
        let plain = n.lines(&d, &font, 200);
        d.word_wrap = true;
        assert!(!Rc::ptr_eq(&plain, &n.lines(&d, &font, 200)));
        let wrapped = n.lines(&d, &font, 200);
        assert!(!Rc::ptr_eq(&wrapped, &n.lines(&d, &font, 100)));
        n.font.points = 20;
        assert!(!Rc::ptr_eq(&wrapped, &n.lines(&d, &font, 200)));
    }
    #[test]
    fn end_at_wrap_boundary_stays_on_row_and_commands_clear_affinity() {
        let mut a = ApplicationSession::new(1, AppId::Notepad, 0);
        a.paste("abcdefghijklmno");
        execute(&mut a, Command::Wrap);
        a.notepad.editor_size = (40, 200);
        key(&mut a, input::CTRL_HOME);
        key(&mut a, input::END);
        let AppState::Document(d) = &a.state else {
            panic!()
        };
        let font = a.notepad.renderer().unwrap();
        let lines = a.notepad.lines(d, &font, 35);
        assert_eq!(d.editor.cursor, lines[0].end);
        assert_eq!(caret(&lines, d.editor.cursor, a.notepad.visual_end).0, 0);
        key(&mut a, input::HOME);
        let AppState::Document(d) = &a.state else {
            panic!()
        };
        assert_eq!(d.editor.cursor, 0);
        key(&mut a, input::END);
        execute(&mut a, Command::SelectAll);
        assert_eq!(a.notepad.visual_end, None);
    }
    #[test]
    fn win7_menu_inventory_and_disabled_states() {
        let app = ApplicationSession::new(1, AppId::Notepad, 0);
        let AppState::Document(d) = &app.state else {
            panic!()
        };
        assert_eq!(menu(0).iter().filter(|i| i.command.is_some()).count(), 7);
        assert!(!enabled(d, &app.notepad, Command::Undo));
        assert!(!enabled(d, &app.notepad, Command::Copy));
        assert!(!enabled(d, &app.notepad, Command::Find));
        assert!(menu(1).iter().all(|i| i.label != "Redo"));
    }
    #[test]
    fn search_direction_no_wrap_unicode_replace_all_is_one_undo() {
        let mut d = Document::new("Untitled.txt");
        d.insert("Ä 中 cat CAT cat");
        d.move_to(0, 0);
        assert!(d.find_direction("ä", false, true));
        assert_eq!(d.selected_text(), "Ä");
        assert!(d.find_direction("cat", false, true));
        assert_eq!(d.position().1, 7);
        assert!(d.find_direction("中", true, false));
        assert_eq!(d.selected_text(), "中");
        d.move_to(0, usize::MAX);
        assert!(!d.find_direction("cat", false, true));
        assert_eq!(d.replace_all("cat", "cat!", false), 3);
        assert_eq!(d.editor.text, "Ä 中 cat! cat! cat!");
        d.undo();
        assert_eq!(d.editor.text, "Ä 中 cat CAT cat");
        assert_eq!(d.replace_all("", "x", true), 0);
    }
    #[test]
    fn goto_font_and_page_cancel_are_transactional() {
        let mut a = ApplicationSession::new(1, AppId::Notepad, 0);
        a.paste("a\nb\nc");
        execute(&mut a, Command::GoTo);
        key(&mut a, b'2');
        key(&mut a, 13);
        let AppState::Document(d) = &a.state else {
            panic!()
        };
        assert_eq!(d.position(), (1, 0));
        execute(&mut a, Command::Font);
        key(&mut a, input::DOWN);
        key(&mut a, 27);
        assert_eq!(a.notepad.font.points, 10);
        execute(&mut a, Command::Page);
        key(&mut a, b'9');
        key(&mut a, 27);
        assert_eq!(a.notepad.page, Page::default());
    }
    #[test]
    fn find_text_survives_closing_dialog_and_f3() {
        let mut a = ApplicationSession::new(1, AppId::Notepad, 0);
        a.paste("one two one");
        if let AppState::Document(d) = &mut a.state {
            d.move_to(0, 0);
        }
        execute(&mut a, Command::Find);
        for b in b"one" {
            key(&mut a, *b);
        }
        key(&mut a, 13);
        key(&mut a, 27);
        key(&mut a, input::FIND_NEXT);
        let AppState::Document(d) = &a.state else {
            panic!()
        };
        assert_eq!(d.selection(), Some((8, 11)));
        key(&mut a, input::FIND_NEXT);
        assert!(!a.notepad.search.message.is_empty());
    }
    #[test]
    fn word_wrap_uses_word_boundaries_and_preserves_bytes() {
        let mut d = Document::new("text.txt");
        d.encoding = Encoding::Utf16Le;
        d.insert("one two three\nlast");
        let bytes = d.encoded();
        d.word_wrap = true;
        let font = FontLib::monospace().unwrap();
        let w = font.measure("one two ", 14).unwrap().width;
        let lines = layout(&d, &font, 14, w);
        assert_eq!(lines[0].end, 8);
        assert_eq!(d.encoded(), bytes);
    }
    #[test]
    fn modeless_find_retains_window_and_search_while_editor_has_focus() {
        let mut a = ApplicationSession::new(1, AppId::Notepad, 0);
        a.paste("hello");
        execute(&mut a, Command::Find);
        pointer(
            &mut a,
            Rect::new(0, 0, 640, 460),
            10,
            300,
            true,
            true,
            false,
        );
        assert!(a.notepad.search.active);
        assert!(!a.notepad.search.focused);
        key(&mut a, b'!');
        let AppState::Document(d) = &a.state else {
            panic!()
        };
        assert!(d.editor.text.contains('!'));
    }
    #[test]
    fn context_menu_keeps_selection_and_mouse_opened_menus_support_keyboard() {
        let mut a = ApplicationSession::new(1, AppId::Notepad, 0);
        a.paste("text");
        key(&mut a, 1);
        pointer(
            &mut a,
            Rect::new(0, 0, 640, 460),
            300,
            300,
            true,
            false,
            true,
        );
        assert_eq!(a.menu_kind, 5);
        let AppState::Document(d) = &a.state else {
            panic!()
        };
        assert_eq!(d.selected_text(), "text");
        key(&mut a, input::DOWN);
        assert!(a.notepad.menu_index < menu(5).len());
        assert!(matches!(key(&mut a,b'c'),AppAction::Copy(text) if text=="text"));
    }
    #[test]
    fn wrap_navigation_moves_by_visual_rows_and_shift_keeps_anchor() {
        let mut a = ApplicationSession::new(1, AppId::Notepad, 0);
        a.paste("one two three four five six");
        execute(&mut a, Command::Wrap);
        a.notepad.editor_size = (75, 200);
        key(&mut a, input::CTRL_HOME);
        key(&mut a, input::DOWN);
        let AppState::Document(d) = &a.state else {
            panic!()
        };
        assert!(d.editor.cursor > 0);
        assert_eq!(d.position().0, 0);
        let anchor = d.editor.cursor;
        key(&mut a, input::SHIFT_DOWN);
        let AppState::Document(d) = &a.state else {
            panic!()
        };
        assert_eq!(d.anchor, Some(anchor));
        assert!(d.editor.cursor > anchor);
    }
    #[test]
    fn margins_reject_nan_negative_and_paper_overflow() {
        let mut p = Page::default();
        assert!(valid_page(&p));
        for value in ["NaN", "-1", "1000", ""] {
            p.margins[0] = value.into();
            assert!(!valid_page(&p));
        }
    }
}
