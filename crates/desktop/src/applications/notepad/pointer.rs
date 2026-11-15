//! Notepad pointer behavior.
use super::*;
pub fn pointer(
    app: &mut ApplicationSession,
    b: Rect,
    x: i32,
    y: i32,
    down: bool,
    held: bool,
    right: bool,
) -> (bool, AppAction) {
    if app.notepad.dialog.is_some() {
        if down {
            dialog_pointer(app, b, x, y);
        }
        return (down, AppAction::None);
    }
    if !app.status.is_empty() && !app.status.starts_with("Saved ") {
        if down {
            app.status.clear();
        }
        return (down, AppAction::None);
    }
    if app.notepad.search.active {
        let s = &mut app.notepad.search;
        let r = search_rect(b, s);
        if r.contains(x, y) {
            if down {
                s.focused = true;
                if Rect::new(r.x + r.width as i32 - 30, r.y + 4, 25, 21).contains(x, y) {
                    s.active = false;
                }
                for (br, id, _) in search_buttons(r, s) {
                    if br.contains(x, y) {
                        match id {
                            0 => find_document(&mut app.state, s, false, false),
                            1 => find_document(&mut app.state, s, true, false),
                            2 => find_document(&mut app.state, s, true, true),
                            _ => s.active = false,
                        }
                    }
                }
                if Rect::new(r.x + 102, r.y + 42, r.width.saturating_sub(218), 25).contains(x, y) {
                    s.focus = 0;
                    s.select = true;
                }
                if s.replace
                    && Rect::new(r.x + 102, r.y + 75, r.width.saturating_sub(218), 25)
                        .contains(x, y)
                {
                    s.focus = 1;
                    s.select = true;
                }
                if Rect::new(r.x + 14, r.y + if s.replace { 119 } else { 96 }, 120, 23)
                    .contains(x, y)
                {
                    s.case = !s.case;
                }
                if !s.replace {
                    if Rect::new(r.x + 177, r.y + 108, 67, 23).contains(x, y) {
                        s.down = false;
                    }
                    if Rect::new(r.x + 248, r.y + 108, 77, 23).contains(x, y) {
                        s.down = true;
                    }
                }
                app.notepad.reveal();
            }
            return (down, AppAction::None);
        }
        // The common Find/Replace window is modeless: clicking the document
        // restores editor focus, while preserving search text and options.
        if down {
            app.notepad.search.focused = false;
        } else {
            return (false, AppAction::None);
        }
    }
    for i in 0..5 {
        let mr = menu_bar(b, i);
        if mr.contains(x, y) {
            let changed = app.hot != Some(mr);
            app.hot = Some(mr);
            if down {
                app.menu = !(app.menu && app.menu_kind == i as u8);
                app.notepad.context = None;
                app.menu_kind = i as u8;
                app.notepad.menu_index = usize::MAX;
            } else if app.menu {
                app.notepad.context = None;
                app.menu_kind = i as u8;
                app.notepad.menu_index = usize::MAX;
            }
            return (changed || down, AppAction::None);
        }
    }
    if app.menu {
        for (i, (mr, item)) in popup_rects(app, b).into_iter().enumerate() {
            if mr.contains(x, y) {
                let changed = app.hot != Some(mr);
                app.hot = Some(mr);
                app.notepad.menu_index = i;
                if down {
                    if let Some(c) = item.command {
                        return (true, execute(app, c));
                    }
                }
                return (changed || down, AppAction::None);
            }
        }
        if down {
            app.menu = false;
        }
        return (down, AppAction::None);
    }
    let hover = app.hot.take().is_some();
    let AppState::Document(d) = &mut app.state else {
        return (hover, AppAction::None);
    };
    let n = &mut app.notepad;
    if !held && !down {
        d.drag_anchor = None;
        n.scroll_drag = None;
        return (hover, AppAction::None);
    }
    let Some(font) = n.renderer() else {
        return (hover, AppAction::None);
    };
    let size = n.font.pixels();
    let h = line_height(&font, size);
    let r = editor_bounds(b, d, n);
    let lines = n.lines(d, &font, r.width.saturating_sub(5));
    let (top, left) = viewport(n, &lines, d, r, h);
    n.top = top;
    n.left = left;
    let vertical = Rect::new(r.x + r.width as i32, r.y, 17, r.height);
    let horizontal = Rect::new(r.x, r.y + r.height as i32, r.width, 17);
    let scroll = if down && vertical.contains(x, y) {
        Some(true)
    } else if down && !d.word_wrap && horizontal.contains(x, y) {
        Some(false)
    } else {
        n.scroll_drag
    };
    if let Some(v) = scroll {
        let bar = if v { vertical } else { horizontal };
        let coord = if v { y - bar.y } else { x - bar.x };
        let length = if v { bar.height } else { bar.width };
        let total = if v {
            lines.len() as u32
        } else {
            lines.iter().map(|l| l.width).max().unwrap_or(0) + 4
        };
        let page = if v { (r.height / h).max(1) } else { r.width };
        let old = if v { n.top as u32 } else { n.left };
        let pos = if coord < 17 {
            old.saturating_sub(if v { 1 } else { 8 })
        } else if coord >= length as i32 - 17 {
            old.saturating_add(if v { 1 } else { 8 })
                .min(total.saturating_sub(page))
        } else {
            n.scroll_drag = Some(v);
            ((coord - 17) as u32).saturating_mul(total.saturating_sub(page))
                / length.saturating_sub(34).max(1)
        };
        if v {
            n.top = pos as usize;
        } else {
            n.left = pos;
        }
        n.follow = false;
        return (true, AppAction::None);
    }
    if !r.contains(x, y) && d.drag_anchor.is_none() {
        return (hover, AppAction::None);
    }
    if right && down {
        app.menu = true;
        app.menu_kind = 5;
        n.context = Some((x, y));
        n.menu_index = usize::MAX;
        return (true, AppAction::None);
    }
    if !down && d.drag_anchor.is_none() {
        return (hover, AppAction::None);
    }
    let row = top + ((y - r.y).max(0) as u32 / h) as usize;
    let line = &lines[row.min(lines.len() - 1)];
    let cx = (x - r.x - 2).max(0) as u32 + left;
    let target = line
        .cells
        .iter()
        .find(|c| cx < c.x + c.width / 2)
        .map_or(line.end, |c| c.index);
    if down {
        let double = n.last_click.is_some_and(|(time, index)| {
            app.input_time.saturating_sub(time) < 400 && index == target
        });
        n.last_click = Some((app.input_time, target));
        if double {
            let chars: Vec<_> = d.editor.text.chars().collect();
            let mut a = target;
            let mut z = target;
            while a > 0 && (chars[a - 1].is_alphanumeric() || chars[a - 1] == '_') {
                a -= 1;
            }
            while z < chars.len() && (chars[z].is_alphanumeric() || chars[z] == '_') {
                z += 1;
            }
            d.anchor = Some(a);
            d.editor.cursor = z;
            d.drag_anchor = None;
            return (true, AppAction::None);
        }
        d.anchor = None;
        d.drag_anchor = Some(target);
    } else {
        d.anchor = d.drag_anchor;
    }
    d.editor.cursor = target;
    n.visual_end = (target == line.end).then_some(target);
    n.follow = false;
    (true, AppAction::None)
}
