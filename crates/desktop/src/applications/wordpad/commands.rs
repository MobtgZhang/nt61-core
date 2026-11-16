//! WordPad formatting commands and keyboard bindings.
use super::*;
pub fn execute(app: &mut ApplicationSession, c: Command) -> AppAction {
    app.wordpad.manual_scroll = false;
    let requested = match c {
        Command::FontMenu => Some(Popup::Font),
        Command::SizeMenu => Some(Popup::Size),
        Command::ColorMenu => Some(Popup::Color),
        Command::HighlightMenu => Some(Popup::Highlight),
        _ => None,
    };
    if let Some(popup) = requested {
        app.wordpad.popup = if app.wordpad.popup == Some(popup) {
            None
        } else {
            Some(popup)
        };
        return AppAction::None;
    }
    app.wordpad.popup = None;
    match c {
        Command::Tab(v) => app.wordpad.view = v,
        Command::Zoom(delta) => {
            app.wordpad.zoom = if delta == 0 {
                100
            } else {
                (app.wordpad.zoom as i16 + delta).clamp(25, 400) as u16
            }
        }
        Command::Ruler => app.wordpad.ruler = !app.wordpad.ruler,
        Command::Status => app.wordpad.status = !app.wordpad.status,
        Command::Time => return AppAction::InsertTime,
        Command::About => {
            app.status =
                "WordPad — NT61 6.1. RTF / DOCX rich text; Word 97-2003 DOC text import.".into()
        }
        Command::Print => super::super::common_dialogs::open_print(app),
        Command::FontDialog => super::super::common_dialogs::open_font(app),
        Command::PageSetup => super::super::common_dialogs::open_page(app),
        Command::MoreColors(highlight) => super::super::common_dialogs::open_color(app,
            if highlight { super::super::common_dialogs::ColorTarget::Highlight }
            else { super::super::common_dialogs::ColorTarget::Text }),
        _ => {
            if let AppState::Document(d) = &mut app.state {
                let s = d.current_style();
                match c {
                    Command::Bold => d.format_selection(false, |v| v.bold = !s.bold),
                    Command::Italic => d.format_selection(false, |v| v.italic = !s.italic),
                    Command::Underline => d.format_selection(false, |v| v.underline = !s.underline),
                    Command::Strike => d.format_selection(false, |v| v.strike = !s.strike),
                    Command::SetFamily(mono) => d.format_selection(false, |v| v.mono = mono),
                    Command::SetSize(size) => {
                        d.format_selection(false, |v| v.size = size.clamp(8, 288))
                    }
                    Command::SetColor(color) => d.format_selection(false, |v| v.color = color),
                    Command::SetHighlight(color) => {
                        d.format_selection(false, |v| v.highlight = color)
                    }
                    Command::Family => d.format_selection(false, |v| v.mono = !s.mono),
                    Command::Size(delta) => d.format_selection(false, |v| {
                        v.size = (v.size as i16 + delta).clamp(8, 288) as u16
                    }),
                    Command::Color => {
                        let palette = [0xff000000, 0xffcc0000, 0xff2058b0, 0xff008040, 0xff8020a0];
                        let at = palette.iter().position(|c| *c == s.color).unwrap_or(0);
                        d.format_selection(false, |v| v.color = palette[(at + 1) % palette.len()]);
                    }
                    Command::Highlight => d.format_selection(false, |v| {
                        v.highlight = if s.highlight == 0 { 0xffffff00 } else { 0 }
                    }),
                    Command::Align(a) => d.format_selection(true, |v| v.align = a),
                    Command::Indent(n) => d.format_selection(true, |v| {
                        v.indent = (v.indent as i32 + n as i32).clamp(0, 7200) as u16
                    }),
                    Command::Bullet => d.format_selection(true, |v| v.bullet = !s.bullet),
                    Command::SelectAll => d.select_all(),
                    Command::Wrap => d.word_wrap = !d.word_wrap,
                    _ => {}
                }
            }
        }
    }
    AppAction::None
}
pub fn key(app: &mut ApplicationSession, key: u8) -> Option<AppAction> {
    app.wordpad.manual_scroll = false;
    if key == 27 && (app.wordpad.popup.is_some() || app.menu) {
        app.wordpad.popup = None;
        app.menu = false;
        return Some(AppAction::None);
    }
    let c = match key {
        2 => Command::Bold,
        21 => Command::Underline,
        12 => Command::Align(0),
        5 => Command::Align(1),
        18 => Command::Align(2),
        crate::applications::explorer::input::ALT_VIEW => Command::Tab(true),
        _ => return None,
    };
    Some(execute(app, c))
}
