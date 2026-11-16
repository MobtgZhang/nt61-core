//! Notepad render behavior.
use super::*;
pub fn draw<F: Fn(&mut Surface, &str, Rect, u32)>(
    app: &ApplicationSession,
    frame: &mut Surface,
    b: Rect,
    f: &F,
) {
    let AppState::Document(d) = &app.state else {
        return;
    };
    let n = &app.notepad;
    frame.fill_rect(b.x, b.y, b.width, b.height, FACE);
    let r = editor_bounds(b, d, n);
    if let Some(font) = n.renderer() {
        let size = n.font.pixels();
        let h = line_height(&font, size);
        let lines = n.lines(d, &font, r.width.saturating_sub(5));
        let (top, left) = viewport(n, &lines, d, r, h);
        let mut paper = Surface::new(r.width.max(1), r.height.max(1));
        paper.fill_rect(0, 0, r.width, r.height, 0xFFFFFFFF);
        let selected = d.selection();
        for (row, line) in lines
            .iter()
            .enumerate()
            .skip(top)
            .take((r.height / h + 1) as usize)
        {
            let y = 1 + ((row - top) as u32 * h) as i32;
            for cell in &line.cells {
                if cell.x.saturating_add(cell.width) <= left {
                    continue;
                }
                if cell.x >= left.saturating_add(r.width) {
                    break;
                }
                let x = 2 + cell.x as i32 - left as i32;
                let sel = selected.is_some_and(|(a, b)| cell.index >= a && cell.index < b);
                if sel {
                    paper.fill_rect(x, y, cell.width, h, 0xFF3399FF);
                }
                if cell.ch != '\t' {
                    let mut bytes = [0; 4];
                    let ch = if font.has_glyph(cell.ch) {
                        cell.ch
                    } else {
                        '□'
                    };
                    let _ = font.draw_text(
                        &mut paper,
                        ch.encode_utf8(&mut bytes),
                        x,
                        y,
                        size,
                        if sel { 0xFFFFFFFF } else { INK },
                    );
                }
            }
            if selected.is_some_and(|(a, b)| line.end >= a && line.end < b) && line.newline {
                paper.fill_rect(2 + line.width as i32 - left as i32, y, 5, h, 0xFF3399FF);
            }
        }
        let (row, x) = caret(&lines, d.editor.cursor, n.visual_end);
        if !(n.search.active && n.search.focused) && n.dialog.is_none() && row >= top {
            paper.fill_rect(
                2 + x as i32 - left as i32,
                1 + ((row - top) as u32 * h) as i32,
                1,
                h.saturating_sub(1),
                INK,
            );
        }
        frame.copy_region(&paper, Rect::new(0, 0, r.width, r.height), r.x, r.y);
        let rows = (r.height / h).max(1) as usize;
        let vr = Rect::new(r.x + r.width as i32, r.y, 17, r.height);
        draw_scroll(
            frame,
            f,
            vr,
            true,
            top as u32,
            rows as u32,
            lines.len() as u32,
        );
        if !d.word_wrap {
            let max = lines
                .iter()
                .map(|l| l.width)
                .max()
                .unwrap_or(0)
                .saturating_add(4);
            draw_scroll(
                frame,
                f,
                Rect::new(r.x, r.y + r.height as i32, r.width, 17),
                false,
                left,
                r.width,
                max,
            );
        }
    }
    if n.status_bar && !d.word_wrap {
        let y = b.y + b.height as i32 - 23;
        frame.fill_rect(b.x, y, b.width, 1, 0xFFD4D4D4);
        let x = b.x + (b.width.saturating_sub(170)) as i32;
        frame.fill_rect(x, y + 3, 1, 18, 0xFFD0D0D0);
        let (line, col) = d.position();
        ui_text(
            f,
            frame,
            &format!("Ln {}, Col {}", line + 1, col + 1),
            x + 8,
            y + 4,
            155,
        );
    }
    frame.fill_rect(b.x, b.y + 21, b.width, 1, 0xFFB5B5B5);
    for (i, label) in ["File", "Edit", "Format", "View", "Help"]
        .iter()
        .enumerate()
    {
        let mr = menu_bar(b, i);
        if (app.menu && app.menu_kind == i as u8) || app.hot == Some(mr) {
            box_rect(frame, mr, 0xFFD5E7F8, 0xFF7DA2CE);
        }
        ui_text(
            f,
            frame,
            label,
            mr.x + 5,
            mr.y + 3,
            mr.width.saturating_sub(6),
        );
    }
    if app.menu {
        let rects = popup_rects(app, b);
        if let (Some((first, _)), Some((last, _))) = (rects.first(), rects.last()) {
            let popup = Rect::new(
                first.x - 3,
                first.y - 3,
                first.width + 6,
                (last.y - first.y) as u32 + last.height + 6,
            );
            frame.shadow_rect(popup, 4);
            box_rect(frame, popup, 0xFFF5F5F5, 0xFF979797);
            frame.fill_rect(
                popup.x + 28,
                popup.y + 3,
                1,
                popup.height.saturating_sub(6),
                0xFFDDDDDD,
            );
        }
        for (i, (mr, item)) in rects.into_iter().enumerate() {
            if let Some(c) = item.command {
                let enable = enabled(d, n, c);
                if enable && (app.hot == Some(mr) || n.menu_index == i) {
                    box_rect(frame, mr, 0xFFE4F1FB, 0xFFA9D1F5);
                }
                let checked =
                    (c == Command::Wrap && d.word_wrap) || (c == Command::Status && n.status_bar);
                if checked {
                    ui_text(f, frame, "✓", mr.x + 3, mr.y + 2, 20);
                }
                text(
                    f,
                    frame,
                    item.label,
                    Rect::new(mr.x + 32, mr.y + 3, mr.width.saturating_sub(95), 20),
                    if enable { INK } else { GRAY },
                );
                text(
                    f,
                    frame,
                    item.shortcut,
                    Rect::new(mr.x + mr.width as i32 - 79, mr.y + 3, 77, 20),
                    if enable { INK } else { GRAY },
                );
            } else {
                frame.fill_rect(
                    mr.x + 29,
                    mr.y + 3,
                    mr.width.saturating_sub(32),
                    1,
                    0xFFD7D7D7,
                );
            }
        }
    }
    if n.search.active {
        draw_search(frame, f, b, &n.search);
    }
    if let Some(dialog) = &n.dialog {
        draw_dialog(frame, f, b, n, dialog);
    }
    if !app.status.is_empty() && !app.status.starts_with("Saved ") {
        let r = dialog_rect(b, 420, 130);
        dialog_frame(frame, f, r, "Notepad");
        ui_text(f, frame, &app.status, r.x + 16, r.y + 45, r.width - 32);
        push_button(
            frame,
            f,
            Rect::new(r.x + r.width as i32 - 90, r.y + 88, 75, 26),
            "OK",
            true,
            true,
        );
    }
    if let Some(dialog) = &app.dialog {
        crate::applications::file_dialog::draw(dialog, frame, b, f);
    }
    super::super::common_dialogs::draw(app, frame, b, f);
    if app.confirm {
        let r = dialog_rect(b, 480, 170);
        dialog_frame(frame, f, r, "Notepad");
        text(
            f,
            frame,
            &format!("Do you want to save changes to {}?", app.file_name()),
            Rect::new(r.x + 22, r.y + 48, r.width.saturating_sub(44), 48),
            0xFF003399,
        );
        for (br, label) in confirm_buttons(b)
            .into_iter()
            .zip(["Save", "Don't Save", "Cancel"])
        {
            push_button(frame, f, br, label, label == "Save", true);
        }
    }
}
pub fn confirm_buttons(b: Rect) -> [Rect; 3] {
    let r = dialog_rect(b, 480, 170);
    core::array::from_fn(|i| {
        Rect::new(
            r.x + r.width as i32 - 310 + i as i32 * 100,
            r.y + r.height as i32 - 44,
            92,
            27,
        )
    })
}
pub(super) fn draw_scroll<F: Fn(&mut Surface, &str, Rect, u32)>(
    frame: &mut Surface,
    f: &F,
    r: Rect,
    vertical: bool,
    pos: u32,
    page: u32,
    total: u32,
) {
    box_rect(frame, r, 0xFFF1F1F1, 0xFFE3E3E3);
    let length = if vertical { r.height } else { r.width };
    for (end, label) in [
        (false, if vertical { "▴" } else { "‹" }),
        (true, if vertical { "▾" } else { "›" }),
    ] {
        let arrow = if vertical {
            Rect::new(
                r.x,
                r.y + if end {
                    length.saturating_sub(17) as i32
                } else {
                    0
                },
                17,
                17,
            )
        } else {
            Rect::new(
                r.x + if end {
                    length.saturating_sub(17) as i32
                } else {
                    0
                },
                r.y,
                17,
                17,
            )
        };
        text(
            f,
            frame,
            label,
            Rect::new(arrow.x + 3, arrow.y, 14, 17),
            if total > page { 0xFF555555 } else { 0xFFAAAAAA },
        );
    }
    if total > page {
        let track = length.saturating_sub(34);
        let thumb = (track.saturating_mul(page) / total).max(18).min(track);
        let offset = (track - thumb).saturating_mul(pos.min(total - page)) / (total - page);
        let br = if vertical {
            Rect::new(r.x + 1, r.y + 17 + offset as i32, 15, thumb)
        } else {
            Rect::new(r.x + 17 + offset as i32, r.y + 1, thumb, 15)
        };
        box_rect(frame, br, 0xFFDCE7F2, 0xFF9CACBB);
    }
}
