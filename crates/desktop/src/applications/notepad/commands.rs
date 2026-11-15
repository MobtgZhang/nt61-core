//! Notepad commands behavior.
use super::*;
pub fn execute(app: &mut ApplicationSession, c: Command) -> AppAction {
    let AppState::Document(d) = &mut app.state else {
        return AppAction::None;
    };
    let n = &mut app.notepad;
    if !enabled(d, n, c) {
        return AppAction::None;
    }
    app.menu = false;
    n.context = None;
    n.reveal();
    use Command::*;
    match c {
        New => return AppAction::NewDocument,
        Open => return AppAction::Open,
        Save => return app.save(),
        SaveAs => return AppAction::SaveAs,
        Exit => return AppAction::RequestClose,
        Undo => d.undo(),
        Cut => return AppAction::Copy(d.cut()),
        Copy => return AppAction::Copy(d.selected_text()),
        Paste => return AppAction::Paste,
        Delete => {
            d.cut();
        }
        SelectAll => d.select_all(),
        Find | Replace => {
            let selected = d.selected_text();
            if !selected.is_empty() && !selected.contains('\n') {
                n.search.query = selected;
            }
            n.search.replace = c == Replace;
            n.search.active = true;
            n.search.focused = true;
            n.search.focus = 0;
            n.search.select = true;
            n.search.message.clear();
        }
        Next => {
            if n.search.query.is_empty() {
                n.search.active = true;
                n.search.focused = true;
                n.search.focus = 0;
            } else {
                find(d, &mut n.search);
            }
        }
        GoTo => {
            n.dialog = Some(Dialog::GoTo {
                line: (d.position().0 + 1).to_string(),
                select: true,
                error: String::new(),
            })
        }
        Wrap => {
            d.word_wrap = !d.word_wrap;
            n.left = 0;
        }
        Status => n.status_bar = !n.status_bar,
        Font => n.dialog = Some(Dialog::Font(n.font)),
        Page => {
            n.dialog = Some(Dialog::Page {
                draft: n.page.clone(),
                focus: 0,
                select: true,
                error: String::new(),
            })
        }
        Print => n.dialog = Some(Dialog::Print),
        About => n.dialog = Some(Dialog::About),
        Help => n.dialog = Some(Dialog::Help),
        Time => return AppAction::InsertTime,
    }
    AppAction::None
}
pub(super) fn find(d: &mut Document, s: &mut Search) {
    if s.query.is_empty() {
        return;
    }
    if d.find_direction(&s.query, s.case, s.down || s.replace) {
        s.message.clear();
    } else {
        s.active = true;
        s.focused = true;
        s.message = format!("Cannot find \"{}\"", s.query);
    }
}
pub(super) fn replace(d: &mut Document, s: &mut Search, all: bool) {
    if s.query.is_empty() {
        return;
    }
    if all {
        if d.replace_all(&s.query, &s.replacement, s.case) == 0 {
            s.message = format!("Cannot find \"{}\"", s.query);
        } else {
            s.message.clear();
        }
    } else {
        let selected = d.selected_text();
        if if s.case {
            selected == s.query
        } else {
            selected.to_lowercase() == s.query.to_lowercase()
        } {
            d.replace_selected(&s.replacement);
        }
        find(d, s);
    }
}
pub(super) fn edit(field: &mut String, select: &mut bool, key: u8, numeric: bool) {
    if key == 1 {
        *select = true;
    } else if key == 8
        || key == input::DELETE
        || ((0x20..=0x7e).contains(&key) && (!numeric || key.is_ascii_digit() || key == b'.'))
    {
        if *select {
            field.clear();
            *select = false;
        }
        if key == 8 {
            field.pop();
        } else if key != input::DELETE && field.len() < 1024 {
            field.push(key as char);
        }
    }
}
pub fn key(app: &mut ApplicationSession, key: u8) -> AppAction {
    let n = &mut app.notepad;
    if let Some(mut dialog) = n.dialog.take() {
        if key == 27 {
            return AppAction::None;
        }
        let mut close = false;
        match &mut dialog {
            Dialog::GoTo {
                line,
                select,
                error,
            } => {
                if key == 13 {
                    if let AppState::Document(d) = &mut app.state {
                        if line.parse().ok().is_some_and(|line| d.goto_line(line)) {
                            close = true;
                            n.reveal();
                        } else {
                            *error = "The line number is beyond the total number of lines.".into();
                            *select = true;
                        }
                    }
                } else {
                    edit(line, select, key, true);
                }
            }
            Dialog::Font(draft) => match key {
                13 => {
                    n.font = *draft;
                    close = true;
                    n.reveal();
                }
                input::UP => draft.points = draft.points.saturating_sub(1).max(6),
                input::DOWN => draft.points = (draft.points + 1).min(72),
                9 => draft.sans = !draft.sans,
                b' ' => draft.style = (draft.style + 1) % 4,
                _ => {}
            },
            Dialog::Page {
                draft,
                focus,
                select,
                error,
            } => {
                if key == 13 {
                    if valid_page(draft) {
                        n.page = draft.clone();
                        close = true;
                    } else {
                        *error = "Enter valid margins inside the paper (millimeters).".into();
                    }
                } else if key == 9 {
                    *focus = (*focus + 1) % 6;
                    *select = true;
                } else {
                    let f = if *focus < 4 {
                        &mut draft.margins[*focus]
                    } else if *focus == 4 {
                        &mut draft.header
                    } else {
                        &mut draft.footer
                    };
                    edit(f, select, key, *focus < 4);
                }
            }
            _ => {
                close = key == 13 || key == b' ';
            }
        }
        if !close {
            n.dialog = Some(dialog);
        }
        return AppAction::None;
    }
    if n.search.active && n.search.focused {
        let s = &mut n.search;
        if key == 27 {
            s.active = false;
        } else {
            match key {
                9 => {
                    s.focus = (s.focus + 1) % if s.replace { 2 } else { 1 };
                    s.select = true;
                }
                13 => {
                    if s.focus == 0 {
                        find_document(&mut app.state, s, false, false);
                    } else {
                        find_document(&mut app.state, s, true, false);
                    }
                }
                18 if s.replace => find_document(&mut app.state, s, true, false),
                _ => {
                    let field = if s.focus == 1 {
                        &mut s.replacement
                    } else {
                        &mut s.query
                    };
                    edit(field, &mut s.select, key, false);
                    s.message.clear();
                }
            }
        }
        n.reveal();
        return AppAction::None;
    }
    if (input::ALT_FILE..=input::ALT_HELP).contains(&key) {
        app.menu = true;
        n.context = None;
        app.menu_kind = key - input::ALT_FILE;
        n.menu_index = 0;
        return AppAction::None;
    }
    if app.menu {
        match key {
            27 => app.menu = false,
            input::LEFT => {
                app.menu_kind = if app.menu_kind == 5 {
                    5
                } else {
                    (app.menu_kind + 4) % 5
                };
                n.menu_index = 0;
            }
            input::RIGHT => {
                app.menu_kind = if app.menu_kind == 5 {
                    5
                } else {
                    (app.menu_kind + 1) % 5
                };
                n.menu_index = 0;
            }
            input::UP | input::DOWN => {
                let items = menu(app.menu_kind);
                if n.menu_index >= items.len() {
                    n.menu_index = if key == input::DOWN {
                        items.len() - 1
                    } else {
                        0
                    };
                }
                for _ in 0..items.len() {
                    n.menu_index = if key == input::DOWN {
                        (n.menu_index + 1) % items.len()
                    } else {
                        (n.menu_index + items.len() - 1) % items.len()
                    };
                    if items[n.menu_index].command.is_some() {
                        break;
                    }
                }
            }
            13 => {
                if let Some(c) = menu(app.menu_kind)
                    .get(n.menu_index)
                    .and_then(|i| i.command)
                {
                    return execute(app, c);
                }
            }
            key if key.is_ascii_alphabetic() => {
                use Command::*;
                let command = match (app.menu_kind, key.to_ascii_lowercase()) {
                    (0, b'n') => Some(New),
                    (0, b'o') => Some(Open),
                    (0, b's') => Some(Save),
                    (0, b'a') => Some(SaveAs),
                    (0, b'u') => Some(Page),
                    (0, b'p') => Some(Print),
                    (0, b'x') => Some(Exit),
                    (1 | 5, b'u') => Some(Undo),
                    (1 | 5, b't') => Some(Cut),
                    (1 | 5, b'c') => Some(Copy),
                    (1 | 5, b'p') => Some(Paste),
                    (1 | 5, b'l') => Some(Delete),
                    (1, b'f') => Some(Find),
                    (1, b'n') => Some(Next),
                    (1, b'r') => Some(Replace),
                    (1, b'g') => Some(GoTo),
                    (1 | 5, b'a') => Some(SelectAll),
                    (1, b'd') => Some(Time),
                    (2, b'w') => Some(Wrap),
                    (2, b'f') => Some(Font),
                    (3, b's') => Some(Status),
                    (4, b'h') => Some(Help),
                    (4, b'a') => Some(About),
                    _ => None,
                };
                if let Some(c) = command {
                    return execute(app, c);
                }
            }
            _ => {}
        }
        return AppAction::None;
    }
    let command = match key {
        14 => Some(Command::New),
        15 => Some(Command::Open),
        19 => Some(Command::Save),
        16 => Some(Command::Print),
        26 => Some(Command::Undo),
        24 => Some(Command::Cut),
        3 => Some(Command::Copy),
        22 => Some(Command::Paste),
        6 => Some(Command::Find),
        7 => Some(Command::GoTo),
        input::FIND_NEXT => Some(Command::Next),
        input::REFRESH => Some(Command::Time),
        input::HELP => Some(Command::Help),
        _ => None,
    };
    // Ctrl+H is delivered as 0x08 by a console stream, like Backspace. The input
    // driver emits a separate private key for Ctrl+H so both operations work.
    let command = if key == 8 {
        None
    } else if key == input::REPLACE {
        Some(Command::Replace)
    } else {
        command
    };
    if let Some(c) = command {
        return execute(app, c);
    }
    n.follow = true;
    if let AppState::Document(d) = &mut app.state {
        if !visual_navigation(d, n, key) {
            n.visual_end = None;
            d.key(key);
        }
    }
    AppAction::None
}
pub(super) fn visual_navigation(d: &mut Document, n: &mut Notepad, key: u8) -> bool {
    let shift = (input::SHIFT_UP..=input::SHIFT_END).contains(&key);
    let key = match key {
        input::SHIFT_UP => input::UP,
        input::SHIFT_DOWN => input::DOWN,
        input::SHIFT_HOME => input::HOME,
        input::SHIFT_END => input::END,
        _ => key,
    };
    if !matches!(
        key,
        input::UP | input::DOWN | input::HOME | input::END | input::PAGE_UP | input::PAGE_DOWN
    ) {
        return false;
    }
    let Some(font) = n.renderer() else {
        return false;
    };
    let lines = n.lines(d, &font, n.editor_size.0.saturating_sub(5));
    let (row, x) = caret(&lines, d.editor.cursor, n.visual_end);
    let rows = (n.editor_size.1 / line_height(&font, n.font.pixels())).max(1) as usize;
    let target_row = match key {
        input::UP => row.saturating_sub(1),
        input::DOWN => (row + 1).min(lines.len() - 1),
        input::PAGE_UP => row.saturating_sub(rows),
        input::PAGE_DOWN => (row + rows).min(lines.len() - 1),
        _ => row,
    };
    let line = &lines[target_row];
    let anchor = d.anchor.unwrap_or(d.editor.cursor);
    d.editor.cursor = match key {
        input::HOME => line.start,
        input::END => line.end,
        _ => line
            .cells
            .iter()
            .find(|c| x < c.x + c.width / 2)
            .map_or(line.end, |c| c.index),
    };
    d.anchor = if shift { Some(anchor) } else { None };
    n.visual_end = (d.editor.cursor == line.end).then_some(d.editor.cursor);
    true
}
pub(super) fn find_document(state: &mut AppState, s: &mut Search, do_replace: bool, all: bool) {
    if let AppState::Document(d) = state {
        if do_replace {
            replace(d, s, all);
        } else {
            find(d, s);
        }
    }
}
pub(super) fn valid_page(p: &Page) -> bool {
    let values: Option<Vec<f32>> = p
        .margins
        .iter()
        .map(|s| s.parse::<f32>().ok().filter(|v| v.is_finite() && *v >= 0.0))
        .collect();
    values.is_some_and(|v| {
        let (w, h) = if p.letter {
            (215.9, 279.4)
        } else {
            (210.0, 297.0)
        };
        let (w, h) = if p.landscape { (h, w) } else { (w, h) };
        v[0] + v[1] < w && v[2] + v[3] < h
    })
}

// Layout uses exactly the same glyph advances for drawing, wrapping and hit tests.
