use super::*;
use alloc::format;
use crate::compositor::aero;
const INK: u32 = 0xff000000;
const GRAY: u32 = 0xff6d6d6d;

pub(super) fn desired_size(dialog: &Dialog) -> (u32, u32) {
    match dialog {
        Dialog::Color { .. } => (540, 398),
        Dialog::Font(_) => (450, 390),
        Dialog::Print(_) => (480, 340),
        Dialog::Page { .. } => (460, 382),
    }
}
pub fn bounds(dialog: &Dialog, body: Rect) -> Rect {
    let (w, h) = desired_size(dialog);
    super::notepad::dialog_rect(body, w, h)
}
pub(super) fn content_clip(r: Rect) -> Rect {
    Rect::new(
        r.x + 1,
        r.y + 30,
        r.width.saturating_sub(2),
        r.height.saturating_sub(74),
    )
}
pub(super) fn scroll_button(r: Rect, index: usize) -> Rect {
    Rect::new(
        r.x + 7 + index as i32 * 20,
        r.y + r.height as i32 - 35,
        18,
        24,
    )
}
pub(super) fn ok_button(r: Rect) -> Rect {
    Rect::new(
        r.x + r.width as i32 - 180,
        r.y + r.height as i32 - 36,
        76,
        26,
    )
}
pub(super) fn cancel_button(r: Rect) -> Rect {
    Rect::new(
        r.x + r.width as i32 - 96,
        r.y + r.height as i32 - 36,
        76,
        26,
    )
}
pub(super) fn print_field(r: Rect, i: usize) -> Rect {
    let (x, y) = [(161, 208), (266, 208), (161, 246)][i];
    Rect::new(r.x + x, r.y + y, 58, 24)
}
pub(super) fn page_field(r: Rect, i: usize) -> Rect {
    if i < 4 {
        Rect::new(
            r.x + 88 + (i % 2) as i32 * 218,
            r.y + 190 + (i / 2) as i32 * 32,
            118,
            25,
        )
    } else {
        Rect::new(r.x + 88, r.y + 263 + (i - 4) as i32 * 30, r.width - 106, 25)
    }
}
fn box_rect(frame: &mut Surface, r: Rect, bg: u32, edge: u32) {
    frame.fill_rect(r.x, r.y, r.width, r.height, edge);
    frame.fill_rect(
        r.x + 1,
        r.y + 1,
        r.width.saturating_sub(2),
        r.height.saturating_sub(2),
        bg,
    );
}
fn label<F: Fn(&mut Surface, &str, Rect, u32)>(
    frame: &mut Surface,
    f: &F,
    r: Rect,
    x: i32,
    y: i32,
    w: u32,
    s: &str,
) {
    f(frame, s, Rect::new(r.x + x, r.y + y, w, 20), INK);
}
fn button<F: Fn(&mut Surface, &str, Rect, u32)>(
    frame: &mut Surface,
    f: &F,
    r: Rect,
    s: &str,
    enabled: bool,
    default: bool,
) {
    box_rect(
        frame,
        r,
        0xfff4f4f4,
        if default { 0xff3c7fb1 } else { 0xffacacac },
    );
    frame.fill_rect(r.x + 2, r.y + 2, r.width.saturating_sub(4), 10, 0xfffafafa);
    f(
        frame,
        s,
        Rect::new(r.x + 7, r.y + 5, r.width.saturating_sub(12), 20),
        if enabled { INK } else { GRAY },
    );
}
fn scroll_arrow(frame: &mut Surface, r: Rect, direction: usize, enabled: bool) {
    box_rect(frame, r, 0xfff4f4f4, 0xffacacac);
    let color = if enabled { INK } else { GRAY };
    let (cx, cy) = (r.x + r.width as i32 / 2, r.y + r.height as i32 / 2);
    for i in 0..4 {
        match direction {
            0 => frame.fill_rect(cx - 2 + i, cy - i, 1, (i * 2 + 1) as u32, color),
            1 => frame.fill_rect(cx + 2 - i, cy - i, 1, (i * 2 + 1) as u32, color),
            2 => frame.fill_rect(cx - i, cy - 2 + i, (i * 2 + 1) as u32, 1, color),
            _ => frame.fill_rect(cx - i, cy + 2 - i, (i * 2 + 1) as u32, 1, color),
        }
    }
}
fn focus_ring(frame: &mut Surface, r: Rect) {
    for x in (r.x..r.x + r.width as i32).step_by(2) {
        frame.fill_rect(x, r.y, 1, 1, INK);
        frame.fill_rect(x, r.y + r.height as i32 - 1, 1, 1, INK);
    }
    for y in (r.y..r.y + r.height as i32).step_by(2) {
        frame.fill_rect(r.x, y, 1, 1, INK);
        frame.fill_rect(r.x + r.width as i32 - 1, y, 1, 1, INK);
    }
}
fn field<F: Fn(&mut Surface, &str, Rect, u32)>(
    frame: &mut Surface,
    f: &F,
    r: Rect,
    s: &str,
    focused: bool,
    selected: bool,
) {
    box_rect(
        frame,
        r,
        if focused && selected {
            0xff3399ff
        } else {
            0xffffffff
        },
        if focused { 0xff3c7fb1 } else { 0xff7a7a7a },
    );
    f(
        frame,
        s,
        Rect::new(r.x + 4, r.y + 4, r.width.saturating_sub(8), 19),
        if focused && selected { 0xffffffff } else { INK },
    );
}
fn check<F: Fn(&mut Surface, &str, Rect, u32)>(
    frame: &mut Surface,
    f: &F,
    r: Rect,
    s: &str,
    checked: bool,
    enabled: bool,
) {
    box_rect(
        frame,
        Rect::new(r.x, r.y + 3, 13, 13),
        0xffffffff,
        0xff707070,
    );
    if checked {
        f(frame, "✓", Rect::new(r.x, r.y, 15, 20), INK);
    }
    f(
        frame,
        s,
        Rect::new(r.x + 19, r.y, r.width.saturating_sub(19), 22),
        if enabled { INK } else { GRAY },
    );
}
fn radio<F: Fn(&mut Surface, &str, Rect, u32)>(
    frame: &mut Surface,
    f: &F,
    r: Rect,
    s: &str,
    checked: bool,
    enabled: bool,
) {
    for y in -6i32..=6 {
        for x in -6i32..=6 {
            let distance = x * x + y * y;
            if distance <= 40 {
                frame.fill_rect(
                    r.x + 6 + x,
                    r.y + 10 + y,
                    1,
                    1,
                    if distance >= 26 { GRAY } else { 0xffffffff },
                );
            }
            if checked && distance <= 7 {
                frame.fill_rect(
                    r.x + 6 + x,
                    r.y + 10 + y,
                    1,
                    1,
                    if enabled { INK } else { GRAY },
                );
            }
        }
    }
    f(
        frame,
        s,
        Rect::new(r.x + 19, r.y, r.width.saturating_sub(19), 22),
        if enabled { INK } else { GRAY },
    );
}
fn color<F: Fn(&mut Surface, &str, Rect, u32)>(
    d: &ColorDialog,
    frame: &mut Surface,
    r: Rect,
    f: &F,
) {
    label(frame, f, r, 14, 36, 220, "Basic colors:");
    label(frame, f, r, 14, 228, 220, "Custom colors:");
    for i in 0..64 {
        let local = super::color::swatch(i);
        let swatch = Rect::new(r.x + local.x, r.y + local.y, local.width, local.height);
        box_rect(
            frame,
            swatch,
            if i < 48 {
                BASIC_COLORS[i]
            } else {
                d.custom[i - 48]
            },
            if d.selected == i {
                0xff000000
            } else {
                0xff808080
            },
        );
        if d.selected == i {
            frame.fill_rect(swatch.x, swatch.y, swatch.width, 2, 0xffffffff);
            frame.fill_rect(
                swatch.x,
                swatch.y + swatch.height as i32 - 2,
                swatch.width,
                2,
                0xff000000,
            );
        }
    }
    let (hue, sat, lum) = d.hsl();
    for y in 0..132 {
        for x in 0..218 {
            frame.fill_rect(
                r.x + 286 + x,
                r.y + 58 + y,
                1,
                1,
                super::color::hsl_to_rgb(x as u32 * 239 / 217, 240 - y as u32 * 240 / 131, 120),
            );
        }
        frame.fill_rect(
            r.x + 512,
            r.y + 58 + y,
            12,
            1,
            super::color::hsl_to_rgb(hue, sat, 240 - y as u32 * 240 / 131),
        );
    }
    let marker_x = r.x + 286 + (hue * 217 / 239) as i32;
    let marker_y = r.y + 58 + ((240 - sat) * 131 / 240) as i32;
    frame.fill_rect(marker_x - 4, marker_y, 9, 1, 0xff000000);
    frame.fill_rect(marker_x, marker_y - 4, 1, 9, 0xff000000);
    frame.fill_rect(
        r.x + 510,
        r.y + 58 + ((240 - lum) * 131 / 240) as i32,
        16,
        2,
        0xff000000,
    );
    box_rect(
        frame,
        Rect::new(r.x + 250, r.y + 202, 34, 70),
        d.color,
        0xff808080,
    );
    for (i, name) in ["Hue:", "Sat:", "Lum:", "Red:", "Green:", "Blue:"]
        .iter()
        .enumerate()
    {
        let local = super::color::field(i);
        label(
            frame,
            f,
            r,
            local.x - if i < 3 { 43 } else { 64 },
            local.y + 4,
            if i < 3 { 43 } else { 64 },
            name,
        );
        field(
            frame,
            f,
            Rect::new(r.x + local.x, r.y + local.y, local.width, local.height),
            &d.fields[i],
            d.focus == i + 1,
            d.select,
        );
    }
    button(
        frame,
        f,
        Rect::new(r.x + 286, r.y + 300, 238, 27),
        "Add to Custom Colors",
        true,
        false,
    );
    label(frame, f, r, 14, 335, 510, &d.error);
}
fn font<F: Fn(&mut Surface, &str, Rect, u32)>(
    app: &ApplicationSession,
    d: &FontDialog,
    frame: &mut Surface,
    r: Rect,
    f: &F,
) {
    for (x, w, name) in [
        (14, 200, "Font:"),
        (226, 120, "Font style:"),
        (358, 76, "Size:"),
    ] {
        label(frame, f, r, x, 39, w, name);
    }
    field(
        frame,
        f,
        Rect::new(r.x + 14, r.y + 63, 200, 25),
        if d.font.sans {
            "DejaVu Sans"
        } else {
            "DejaVu Sans Mono"
        },
        d.focus == 0,
        false,
    );
    field(
        frame,
        f,
        Rect::new(r.x + 226, r.y + 63, 120, 25),
        ["Regular", "Bold", "Oblique", "Bold Oblique"][usize::from(d.font.style) % 4],
        d.focus == 1,
        false,
    );
    field(
        frame,
        f,
        Rect::new(r.x + 358, r.y + 63, 76, 25),
        &d.points,
        d.focus == 2,
        d.select,
    );
    for (i, name) in ["DejaVu Sans Mono", "DejaVu Sans"].iter().enumerate() {
        field(
            frame,
            f,
            Rect::new(r.x + 14, r.y + 90 + i as i32 * 24, 200, 24),
            name,
            d.font.sans == (i == 1),
            true,
        );
    }
    for (i, name) in ["Regular", "Bold", "Oblique", "Bold Oblique"]
        .iter()
        .enumerate()
    {
        field(
            frame,
            f,
            Rect::new(r.x + 226, r.y + 90 + i as i32 * 24, 120, 24),
            name,
            usize::from(d.font.style) == i,
            true,
        );
    }
    for (i, size) in [8, 9, 10, 11, 12, 14, 16, 18, 20, 24].iter().enumerate() {
        field(
            frame,
            f,
            Rect::new(r.x + 358, r.y + 90 + i as i32 * 20, 76, 20),
            &format!("{size}"),
            d.font.points == *size,
            true,
        );
    }
    if app.id != super::super::AppId::Notepad {
        check(
            frame,
            f,
            Rect::new(r.x + 14, r.y + 207, 150, 24),
            "Underline",
            d.underline,
            true,
        );
        check(
            frame,
            f,
            Rect::new(r.x + 166, r.y + 207, 150, 24),
            "Strikeout",
            d.strike,
            true,
        );
        if d.focus == 3 || d.focus == 4 {
            focus_ring(
                frame,
                Rect::new(
                    r.x + if d.focus == 3 { 12 } else { 164 },
                    r.y + 205,
                    146,
                    24,
                ),
            );
        }
    }
    label(frame, f, r, 14, 236, 100, "Sample");
    box_rect(
        frame,
        Rect::new(r.x + 14, r.y + 258, 332, 59),
        0xffffffff,
        0xff808080,
    );
    if let Ok(face) = app.notepad.fonts.get(d.font.bytes()) {
        let mut sample = Surface::new(324, 53);
        sample.fill_rect(0, 0, 324, 53, 0xffffffff);
        let size = (d.font.points * 4).div_ceil(3);
        let _ = face.draw_text(&mut sample, "AaBbYyZz 0123", 4, 2, size, INK);
        if d.underline {
            sample.fill_rect(4, size.min(49) as i32, 218, 1, INK);
        }
        if d.strike {
            sample.fill_rect(4, (size / 2).min(49) as i32, 218, 1, INK);
        }
        frame.copy_region(&sample, Rect::new(0, 0, 324, 53), r.x + 18, r.y + 261);
    }
    label(frame, f, r, 14, 328, 420, &d.error);
}
fn print<F: Fn(&mut Surface, &str, Rect, u32)>(
    d: &PrintDialog,
    frame: &mut Surface,
    r: Rect,
    f: &F,
) {
    label(frame, f, r, 18, 40, 440, "Select Printer");
    box_rect(
        frame,
        Rect::new(r.x + 18, r.y + 65, r.width.saturating_sub(36), 79),
        0xffffffff,
        0xff808080,
    );
    crate::applications::explorer::theme::printer(frame, Rect::new(r.x + 28, r.y + 78, 32, 32));
    label(frame, f, r, 70, 80, 380, "NT61 PostScript Document Writer");
    label(frame, f, r, 28, 109, 420, "Status: Ready — output in Documents");
    label(frame, f, r, 18, 151, 200, "Page Range");
    radio(
        frame,
        f,
        Rect::new(r.x + 22, r.y + 178, 125, 24),
        "All",
        !d.range && !d.selection,
        true,
    );
    radio(
        frame,
        f,
        Rect::new(r.x + 156, r.y + 178, 125, 24),
        "Selection",
        d.selection,
        d.selection_available,
    );
    radio(
        frame,
        f,
        Rect::new(r.x + 22, r.y + 208, 125, 24),
        "Pages",
        d.range,
        true,
    );
    label(frame, f, r, 124, 212, 37, "From:");
    label(frame, f, r, 235, 212, 31, "To:");
    label(frame, f, r, 22, 250, 139, "Number of copies:");
    for (i, value) in [&d.from, &d.to, &d.copies].iter().enumerate() {
        field(frame, f, print_field(r, i), value, d.focus == i, d.select);
    }
    check(
        frame,
        f,
        Rect::new(r.x + 302, r.y + 246, 125, 24),
        "Collate",
        d.collate,
        true,
    );
    if let Some((x, y, w)) = match d.focus {
        3 => Some((20, 176, 125)),
        4 => Some((154, 176, 125)),
        5 => Some((20, 206, 100)),
        6 => Some((300, 244, 125)),
        _ => None,
    } {
        focus_ring(frame, Rect::new(r.x + x, r.y + y, w, 24));
    }
    label(frame, f, r, 18, 280, 444, &d.error);
}
fn page<F: Fn(&mut Surface, &str, Rect, u32)>(
    d: &notepad::Page,
    focus: usize,
    select: bool,
    error: &str,
    frame: &mut Surface,
    r: Rect,
    f: &F,
) {
    label(frame, f, r, 16, 39, 200, "Paper size:");
    button(
        frame,
        f,
        Rect::new(r.x + 18, r.y + 62, 190, 26),
        if d.letter {
            "Letter (8.5 x 11 in)"
        } else {
            "A4 (210 x 297 mm)"
        },
        true,
        focus == 6,
    );
    label(frame, f, r, 190, 65, 16, "▾");
    label(frame, f, r, 16, 103, 200, "Orientation");
    radio(
        frame,
        f,
        Rect::new(r.x + 24, r.y + 131, 110, 23),
        "Portrait",
        !d.landscape,
        true,
    );
    radio(
        frame,
        f,
        Rect::new(r.x + 140, r.y + 131, 120, 23),
        "Landscape",
        d.landscape,
        true,
    );
    if focus == 7 || focus == 8 {
        focus_ring(
            frame,
            Rect::new(r.x + if focus == 7 { 22 } else { 138 }, r.y + 129, 112, 24),
        );
    }
    let (pw, ph) = if d.landscape { (108, 76) } else { (76, 108) };
    box_rect(
        frame,
        Rect::new(r.x + 326, r.y + 47, pw, ph),
        0xffffffff,
        0xff808080,
    );
    for i in 0..4 {
        frame.fill_rect(r.x + 335, r.y + 70 + i * 9, pw - 18, 1, 0xffc0c0c0);
    }
    label(frame, f, r, 16, 164, 280, "Margins (millimeters)");
    for (i, name) in ["Left:", "Right:", "Top:", "Bottom:", "Header:", "Footer:"]
        .iter()
        .enumerate()
    {
        let field_rect = page_field(r, i);
        f(
            frame,
            name,
            Rect::new(field_rect.x - 70, field_rect.y + 4, 68, 20),
            INK,
        );
        let value = if i < 4 {
            &d.margins[i]
        } else if i == 4 {
            &d.header
        } else {
            &d.footer
        };
        field(frame, f, field_rect, value, focus == i, select);
    }
    label(frame, f, r, 18, 326, 426, error);
}

pub fn draw<F: Fn(&mut Surface, &str, Rect, u32)>(
    app: &ApplicationSession,
    frame: &mut Surface,
    body: Rect,
    f: &F,
) {
    let Some(dialog) = &app.common_dialog else {
        return;
    };
    let r = bounds(dialog, body);
    let mut glass = aero::WindowGlassCache::default();
    glass.prepare(frame, 0, r, 0, true);
    aero::shadow(frame, r, 8);
    box_rect(frame, r, 0xfff0f0f0, 0xff6f8297);
    glass.paint(frame, 0, true);
    aero::window_border(frame, r, 5);
    label(
        frame,
        f,
        r,
        9,
        6,
        r.width.saturating_sub(44),
        match dialog {
            Dialog::Color { .. } => "Color",
            Dialog::Font(_) => "Font",
            Dialog::Print(_) => "Print",
            Dialog::Page { .. } => "Page Setup",
        },
    );
    aero::window_control_button(
        frame,
        Rect::new(r.x + r.width as i32 - 30, r.y + 4, 25, 21),
        aero::WindowControlButtonKind::Close,
        false,
    );
    let old_clip = frame.drawing_bounds();
    frame.set_draw_clip(Some(
        old_clip
            .intersection(&content_clip(r))
            .unwrap_or(Rect::new(0, 0, 0, 0)),
    ));
    let content = super::content_rect(app, dialog, body);
    match dialog {
        Dialog::Color { draft, .. } => color(draft, frame, content, f),
        Dialog::Font(draft) => font(app, draft, frame, content, f),
        Dialog::Print(draft) => print(draft, frame, content, f),
        Dialog::Page {
            draft,
            focus,
            select,
            error,
            ..
        } => page(draft, *focus, *select, error, frame, content, f),
    }
    frame.set_draw_clip(Some(old_clip));
    let limits = super::scroll_limits(dialog, body);
    if limits.0 != 0 {
        scroll_arrow(frame, scroll_button(r, 0), 0, app.common_scroll.0 != 0);
        scroll_arrow(
            frame,
            scroll_button(r, 1),
            1,
            app.common_scroll.0 < limits.0,
        );
    }
    if limits.1 != 0 {
        scroll_arrow(frame, scroll_button(r, 2), 2, app.common_scroll.1 != 0);
        scroll_arrow(
            frame,
            scroll_button(r, 3),
            3,
            app.common_scroll.1 < limits.1,
        );
    }
    let printing = matches!(dialog, Dialog::Print(_));
    button(
        frame,
        f,
        ok_button(r),
        if printing { "Print" } else { "OK" },
        true,
        true,
    );
    button(frame, f, cancel_button(r), "Cancel", true, false);
    if let Dialog::Page { popup, .. } = dialog {
        popup.draw(frame, f);
    }
}
