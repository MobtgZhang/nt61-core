//! Notepad ui helpers.
use super::*;
pub(super) fn box_rect(frame: &mut Surface, r: Rect, bg: u32, border: u32) {
    frame.fill_rect(r.x, r.y, r.width, r.height, border);
    frame.fill_rect(
        r.x + 1,
        r.y + 1,
        r.width.saturating_sub(2),
        r.height.saturating_sub(2),
        bg,
    );
}
pub(super) fn text<F: Fn(&mut Surface, &str, Rect, u32)>(
    f: &F,
    frame: &mut Surface,
    s: &str,
    r: Rect,
    c: u32,
) {
    f(frame, s, r, c);
}
pub(super) fn ui_text<F: Fn(&mut Surface, &str, Rect, u32)>(
    f: &F,
    frame: &mut Surface,
    s: &str,
    x: i32,
    y: i32,
    w: u32,
) {
    text(f, frame, s, Rect::new(x, y, w, 20), INK);
}
pub(super) fn push_button<F: Fn(&mut Surface, &str, Rect, u32)>(
    frame: &mut Surface,
    f: &F,
    r: Rect,
    s: &str,
    default: bool,
    enabled: bool,
) {
    box_rect(
        frame,
        r,
        if enabled { 0xFFF4F4F4 } else { FACE },
        if default { 0xFF3C7FB1 } else { 0xFFACACAC },
    );
    frame.fill_rect(
        r.x + 2,
        r.y + 2,
        r.width.saturating_sub(4),
        r.height / 2,
        0xFFFAFAFA,
    );
    text(
        f,
        frame,
        s,
        Rect::new(r.x + 9, r.y + 5, r.width.saturating_sub(12), 20),
        if enabled { INK } else { GRAY },
    );
}
pub(super) fn checkbox<F: Fn(&mut Surface, &str, Rect, u32)>(
    frame: &mut Surface,
    f: &F,
    r: Rect,
    s: &str,
    checked: bool,
    radio: bool,
) {
    if radio {
        for cy in -6i32..=6 {
            for cx in -6i32..=6 {
                let d = cx * cx + cy * cy;
                if d <= 40 {
                    frame.fill_rect(
                        r.x + 6 + cx,
                        r.y + 9 + cy,
                        1,
                        1,
                        if d >= 26 {
                            0xFF707070
                        } else if checked && d <= 9 {
                            0xFF303030
                        } else {
                            0xFFFFFFFF
                        },
                    );
                }
            }
        }
    } else {
        box_rect(
            frame,
            Rect::new(r.x, r.y + 3, 13, 13),
            0xFFFFFFFF,
            0xFF707070,
        );
        if checked {
            ui_text(f, frame, "✓", r.x, r.y, 15);
        }
    }
    ui_text(f, frame, s, r.x + 20, r.y, r.width.saturating_sub(20));
}
pub(super) fn field<F: Fn(&mut Surface, &str, Rect, u32)>(
    frame: &mut Surface,
    f: &F,
    r: Rect,
    s: &str,
    focus: bool,
    selected: bool,
) {
    box_rect(
        frame,
        r,
        0xFFFFFFFF,
        if focus { 0xFF3399FF } else { 0xFF7F9DB9 },
    );
    if selected && focus {
        let w = (s.chars().count() as u32 * 8).min(r.width.saturating_sub(8));
        frame.fill_rect(r.x + 3, r.y + 3, w, 20, 0xFF3399FF);
    }
    text(
        f,
        frame,
        s,
        Rect::new(r.x + 4, r.y + 4, r.width.saturating_sub(8), 20),
        if selected && focus { 0xFFFFFFFF } else { INK },
    );
    if focus && !selected {
        let x = r.x + 4 + (s.chars().count() as u32 * 8).min(r.width.saturating_sub(9)) as i32;
        frame.fill_rect(x, r.y + 4, 1, 17, INK);
    }
}
pub fn dialog_rect(b: Rect, width: u32, height: u32) -> Rect {
    let w = width.min(b.width.saturating_sub(10));
    let h = height.min(b.height.saturating_sub(10));
    Rect::new(
        b.x + (b.width.saturating_sub(w) / 2) as i32,
        b.y + 32.min(b.height.saturating_sub(h) as i32),
        w,
        h,
    )
}
pub(super) fn dialog_frame<F: Fn(&mut Surface, &str, Rect, u32)>(
    frame: &mut Surface,
    f: &F,
    r: Rect,
    title: &str,
) {
    frame.shadow_rect(r, 5);
    box_rect(frame, r, FACE, 0xFF6F8297);
    frame.fill_rect(r.x + 1, r.y + 1, r.width.saturating_sub(2), 28, 0xFFDCE8F4);
    ui_text(
        f,
        frame,
        title,
        r.x + 9,
        r.y + 6,
        r.width.saturating_sub(44),
    );
    push_button(
        frame,
        f,
        Rect::new(r.x + r.width as i32 - 30, r.y + 4, 25, 21),
        "×",
        false,
        true,
    );
}
pub(super) fn search_rect(b: Rect, s: &Search) -> Rect {
    dialog_rect(b, 456, if s.replace { 228 } else { 190 })
}
pub(super) fn search_buttons(r: Rect, s: &Search) -> Vec<(Rect, usize, &'static str)> {
    let x = r.x + r.width as i32 - 104;
    let mut buttons = alloc::vec![(Rect::new(x, r.y + 42, 94, 26), 0, "Find Next")];
    if s.replace {
        buttons.push((Rect::new(x, r.y + 74, 94, 26), 1, "Replace"));
        buttons.push((Rect::new(x, r.y + 106, 94, 26), 2, "Replace All"));
    }
    buttons.push((
        Rect::new(x, r.y + if s.replace { 138 } else { 76 }, 94, 26),
        3,
        "Cancel",
    ));
    buttons
}
pub(super) fn draw_search<F: Fn(&mut Surface, &str, Rect, u32)>(
    frame: &mut Surface,
    f: &F,
    b: Rect,
    s: &Search,
) {
    let r = search_rect(b, s);
    dialog_frame(frame, f, r, if s.replace { "Replace" } else { "Find" });
    ui_text(f, frame, "Find what:", r.x + 12, r.y + 47, 85);
    field(
        frame,
        f,
        Rect::new(r.x + 102, r.y + 42, r.width.saturating_sub(218), 25),
        &s.query,
        s.focus == 0,
        s.select,
    );
    if s.replace {
        ui_text(f, frame, "Replace with:", r.x + 12, r.y + 80, 88);
        field(
            frame,
            f,
            Rect::new(r.x + 102, r.y + 75, r.width.saturating_sub(218), 25),
            &s.replacement,
            s.focus == 1,
            s.select,
        );
    }
    let y = r.y + if s.replace { 119 } else { 96 };
    checkbox(
        frame,
        f,
        Rect::new(r.x + 14, y, 120, 23),
        "Match case",
        s.case,
        false,
    );
    if !s.replace {
        box_rect(
            frame,
            Rect::new(r.x + 164, r.y + 87, 166, 52),
            FACE,
            0xFFD0D0D0,
        );
        ui_text(f, frame, "Direction", r.x + 174, r.y + 80, 90);
        checkbox(
            frame,
            f,
            Rect::new(r.x + 177, r.y + 108, 67, 23),
            "Up",
            !s.down,
            true,
        );
        checkbox(
            frame,
            f,
            Rect::new(r.x + 248, r.y + 108, 77, 23),
            "Down",
            s.down,
            true,
        );
    }
    for (br, id, label) in search_buttons(r, s) {
        push_button(frame, f, br, label, id == 0, id == 3 || !s.query.is_empty());
    }
    text(
        f,
        frame,
        &s.message,
        Rect::new(r.x + 12, r.y + r.height as i32 - 35, r.width - 24, 28),
        INK,
    );
}
