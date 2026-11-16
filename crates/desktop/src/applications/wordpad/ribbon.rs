//! Windows 7 accessory ribbon. Drawing and hit testing use the same control list.
use super::*;
use crate::applications::shared::toolbar::{self, Icon};

const SMALL_ICONS: &[u8] = include_bytes!("resources/toolbar-16.rgba");
const LARGE_ICONS: &[u8] = include_bytes!("resources/toolbar-32.rgba");
const COLORS: &[u32] = &[
    0xff000000, 0xff666666, 0xffa00000, 0xffe02020, 0xffe87810, 0xffffdc00, 0xff008040, 0xff20a8d0,
    0xff2058b0, 0xff8020a0, 0xffffffff, 0xffdddddd, 0xff884020, 0xffffb0c0, 0xffffd090, 0xffffff99,
    0xffb0e0a0, 0xffa0e0f0, 0xffa0b8e0, 0xffd8b0e0,
];

fn push(out: &mut Vec<Button>, b: Rect, x: i32, y: i32, w: u32, h: u32, label: &str, command: Ui) {
    let bounds = Rect::new(b.x + x, b.y + y, w, h);
    if bounds.intersection(&b) == Some(bounds) {
        out.push(Button {
            bounds,
            label: label.into(),
            command,
        });
    }
}

pub fn buttons(app: &ApplicationSession, b: Rect) -> Vec<Button> {
    let mut out = Vec::new();
    for (x, label, c) in [
        (4, "File", Ui::File),
        (67, "Home", Ui::WordPad(Command::Tab(false))),
        (134, "View", Ui::WordPad(Command::Tab(true))),
    ] {
        push(&mut out, b, x, 2, 60, 26, label, c);
    }
    for (x, label, c) in [
        (211, "Save", Ui::Save),
        (240, "Undo", Ui::Undo),
        (269, "Redo", Ui::Redo),
    ] {
        push(&mut out, b, x, 2, 27, 26, label, c);
    }
    if app.wordpad.view {
        for (x, label, c) in [
            (8, "Zoom in", Command::Zoom(10)),
            (76, "Zoom out", Command::Zoom(-10)),
            (144, "100%", Command::Zoom(0)),
        ] {
            push(&mut out, b, x, 38, 64, 59, label, Ui::WordPad(c));
        }
        for (y, label, c) in [
            (38, "Ruler", Command::Ruler),
            (67, "Status bar", Command::Status),
        ] {
            push(&mut out, b, 232, y, 133, 26, label, Ui::WordPad(c));
        }
        push(
            &mut out,
            b,
            392,
            38,
            123,
            59,
            "Word wrap",
            Ui::WordPad(Command::Wrap),
        );
    } else {
        push(&mut out, b, 8, 36, 54, 62, "Paste", Ui::Paste);
        push(&mut out, b, 65, 36, 65, 26, "Cut", Ui::Cut);
        push(&mut out, b, 65, 66, 65, 26, "Copy", Ui::Copy);
        let style = if let AppState::Document(d) = &app.state {
            d.current_style()
        } else {
            Style::default()
        };
        push(
            &mut out,
            b,
            141,
            36,
            139,
            25,
            if style.mono {
                "DejaVu Mono"
            } else {
                "DejaVu Sans"
            },
            Ui::WordPad(Command::FontMenu),
        );
        push(
            &mut out,
            b,
            283,
            36,
            49,
            25,
            &format!("{}", style.size / 2),
            Ui::WordPad(Command::SizeMenu),
        );
        push(
            &mut out,
            b,
            335,
            36,
            27,
            25,
            "Grow font",
            Ui::WordPad(Command::Size(2)),
        );
        push(
            &mut out,
            b,
            365,
            36,
            27,
            25,
            "Shrink font",
            Ui::WordPad(Command::Size(-2)),
        );
        for (i, (label, c)) in [
            ("Bold", Command::Bold),
            ("Italic", Command::Italic),
            ("Underline", Command::Underline),
            ("Strikethrough", Command::Strike),
            ("Text highlight", Command::HighlightMenu),
            ("Font color", Command::ColorMenu),
        ]
        .into_iter()
        .enumerate()
        {
            push(
                &mut out,
                b,
                141 + i as i32 * 35,
                67,
                32,
                26,
                label,
                Ui::WordPad(c),
            );
        }
        for (i, (label, c)) in [
            ("Bullets", Command::Bullet),
            ("Decrease indent", Command::Indent(-360)),
            ("Increase indent", Command::Indent(360)),
            ("Align left", Command::Align(0)),
            ("Center", Command::Align(1)),
            ("Align right", Command::Align(2)),
        ]
        .into_iter()
        .enumerate()
        {
            push(
                &mut out,
                b,
                406 + (i % 3) as i32 * 35,
                36 + (i / 3) as i32 * 31,
                32,
                26,
                label,
                Ui::WordPad(c),
            );
        }
        push(
            &mut out,
            b,
            531,
            36,
            88,
            62,
            "Date/time",
            Ui::WordPad(Command::Time),
        );
        for (y, label, c) in [
            (34, "Find", Ui::Find),
            (60, "Replace", Ui::Replace),
            (86, "Select all", Ui::WordPad(Command::SelectAll)),
        ] {
            push(&mut out, b, 638, y, 105, 25, label, c);
        }
    }
    if app.wordpad.status {
        let x = b.width.saturating_sub(207) as i32;
        let y = b.height.saturating_sub(23) as i32;
        push(
            &mut out,
            b,
            x,
            y,
            55,
            22,
            &format!("{}%", app.wordpad.zoom),
            Ui::WordPad(Command::Zoom(0)),
        );
        push(
            &mut out,
            b,
            x + 58,
            y,
            24,
            22,
            "Zoom out",
            Ui::WordPad(Command::Zoom(-10)),
        );
        push(
            &mut out,
            b,
            x + 176,
            y,
            24,
            22,
            "Zoom in",
            Ui::WordPad(Command::Zoom(10)),
        );
    }
    match app.wordpad.popup {
        Some(Popup::Font) => {
            for (i, (label, mono)) in [("DejaVu Sans", false), ("DejaVu Sans Mono", true)]
                .into_iter()
                .enumerate()
            {
                push(
                    &mut out,
                    b,
                    141,
                    63 + i as i32 * 30,
                    208,
                    30,
                    label,
                    Ui::WordPad(Command::SetFamily(mono)),
                );
            }
        }
        Some(Popup::Size) => {
            for (i, size) in [8, 9, 10, 11, 12, 14, 16, 18, 20, 22, 24, 26, 28, 36, 48, 72]
                .into_iter()
                .enumerate()
            {
                push(
                    &mut out,
                    b,
                    283 + (i / 8) as i32 * 49,
                    63 + (i % 8) as i32 * 26,
                    49,
                    26,
                    &format!("{size}"),
                    Ui::WordPad(Command::SetSize(size * 2)),
                );
            }
        }
        Some(Popup::Color | Popup::Highlight) => {
            let highlight = app.wordpad.popup == Some(Popup::Highlight);
            for (i, color) in COLORS.iter().copied().enumerate() {
                push(
                    &mut out,
                    b,
                    276 + (i % 10) as i32 * 22,
                    97 + (i / 10) as i32 * 24,
                    22,
                    24,
                    "Color",
                    Ui::WordPad(if highlight {
                        Command::SetHighlight(color)
                    } else {
                        Command::SetColor(color)
                    }),
                );
            }
            push(&mut out, b, 276, 174, 220, 26, "More colors...",
                Ui::WordPad(Command::MoreColors(highlight)));
            if highlight {
                push(
                    &mut out,
                    b,
                    276,
                    145,
                    220,
                    26,
                    "No highlight",
                    Ui::WordPad(Command::SetHighlight(0)),
                );
            }
        }
        None => {}
    }
    if app.menu {
        for (i, (label, c)) in [
            ("New                 Ctrl+N", Ui::NewDocument),
            ("Open...             Ctrl+O", Ui::Open),
            ("Save                Ctrl+S", Ui::Save),
            ("Save as...", Ui::SaveAs),
            ("Print...", Ui::WordPad(Command::Print)),
            ("Page setup...", Ui::WordPad(Command::PageSetup)),
            ("Font...", Ui::WordPad(Command::FontDialog)),
            ("About WordPad", Ui::WordPad(Command::About)),
            ("Exit", Ui::Exit),
        ]
        .into_iter()
        .enumerate()
        {
            push(&mut out, b, 4, 30 + i as i32 * 32, 270, 32, label, c);
        }
    }
    out
}

fn icon(command: &Ui) -> Option<Icon> {
    Some(match command {
        Ui::Save => Icon::Save,
        Ui::Undo => Icon::Undo,
        Ui::Redo => Icon::Redo,
        Ui::Paste => Icon::Paste,
        Ui::Cut => Icon::Cut,
        Ui::Copy => Icon::Copy,
        Ui::Find => Icon::Find,
        Ui::Replace => Icon::Replace,
        Ui::WordPad(c) => match c {
            Command::Bold => Icon::Bold,
            Command::Italic => Icon::Italic,
            Command::Underline => Icon::Underline,
            Command::Strike => Icon::Strike,
            Command::ColorMenu => Icon::Color,
            Command::HighlightMenu => Icon::Highlight,
            Command::Size(n) => {
                if *n > 0 {
                    Icon::Grow
                } else {
                    Icon::Shrink
                }
            }
            Command::Align(0) => Icon::AlignLeft,
            Command::Align(1) => Icon::AlignCenter,
            Command::Align(_) => Icon::AlignRight,
            Command::Indent(n) => {
                if *n > 0 {
                    Icon::IndentRight
                } else {
                    Icon::IndentLeft
                }
            }
            Command::Bullet => Icon::Bullets,
            Command::Time => Icon::Calendar,
            Command::SelectAll => Icon::SelectAll,
            Command::Zoom(n) => {
                if *n == 0 {
                    Icon::ActualSize
                } else if *n > 0 {
                    Icon::ZoomIn
                } else {
                    Icon::ZoomOut
                }
            }
            Command::Ruler => Icon::Ruler,
            Command::Status => Icon::Status,
            Command::Wrap => Icon::Wrap,
            _ => return None,
        },
        _ => return None,
    })
}

fn active(app: &ApplicationSession, c: &Ui) -> bool {
    let AppState::Document(d) = &app.state else {
        return false;
    };
    let s = d.current_style();
    match c {
        Ui::WordPad(c) => match c {
            Command::Tab(v) => *v == app.wordpad.view,
            Command::Bold => s.bold,
            Command::Italic => s.italic,
            Command::Underline => s.underline,
            Command::Strike => s.strike,
            Command::Align(a) => *a == s.align,
            Command::Ruler => app.wordpad.ruler,
            Command::Status => app.wordpad.status,
            Command::Wrap => d.word_wrap,
            Command::Bullet => s.bullet,
            _ => false,
        },
        _ => false,
    }
}

fn border(frame: &mut Surface, r: Rect, color: u32) {
    frame.fill_rect(r.x, r.y, r.width, 1, color);
    frame.fill_rect(r.x, r.y + r.height as i32 - 1, r.width, 1, color);
    frame.fill_rect(r.x, r.y, 1, r.height, color);
    frame.fill_rect(r.x + r.width as i32 - 1, r.y, 1, r.height, color);
}

pub(super) fn draw<F: Fn(&mut Surface, &str, Rect, u32)>(
    app: &ApplicationSession,
    frame: &mut Surface,
    b: Rect,
    text: &F,
) {
    frame.fill_rect(b.x, b.y + 30, b.width, 96, 0xfff4f7fb);
    border(frame, Rect::new(b.x, b.y + 29, b.width, 98), 0xffbdccdc);
    let groups: &[(i32, u32, &str)] = if app.wordpad.view {
        &[
            (0, 220, "Zoom"),
            (220, 162, "Show or hide"),
            (382, 143, "Settings"),
        ]
    } else {
        &[
            (0, 135, "Clipboard"),
            (135, 264, "Font"),
            (399, 121, "Paragraph"),
            (520, 107, "Insert"),
            (627, 130, "Editing"),
        ]
    };
    for &(x, width, label) in groups {
        if x + width as i32 > b.width as i32 {
            continue;
        }
        frame.fill_rect(b.x + x + width as i32 - 1, b.y + 36, 1, 82, 0xffd2dce7);
        text(
            frame,
            label,
            Rect::new(b.x + x + 7, b.y + 104, width - 12, 18),
            0xff65788a,
        );
    }
    // Status bar zoom track uses the same 25..400% range as the commands.
    if app.wordpad.status && b.width >= 230 {
        let x = b.x + b.width as i32 - 119;
        let y = b.y + b.height as i32 - 11;
        frame.fill_rect(x, y, 82, 2, 0xff889bab);
        let at = (u32::from(app.wordpad.zoom) - 25) * 82 / 375;
        frame.fill_rect(x + at as i32, y - 5, 5, 12, 0xff6fa8cc);
    }
    let mut tooltip = None;
    for button in buttons(app, b) {
        let r = button.bounds;
        let popup = match &button.command {
            Ui::WordPad(
                Command::SetFamily(_)
                | Command::SetSize(_)
                | Command::SetColor(_)
                | Command::SetHighlight(_)
                | Command::MoreColors(_),
            ) => true,
            _ => false,
        };
        let menu = app.menu && r.y >= b.y + 30 && r.x == b.x + 4;
        let hot = app.hot == Some(r);
        let selected = active(app, &button.command);
        let combo = matches!(
            button.command,
            Ui::WordPad(Command::FontMenu | Command::SizeMenu)
        );
        if popup || menu || combo {
            frame.fill_rect(r.x, r.y, r.width, r.height, 0xffffffff);
            border(frame, r, 0xffaebdca);
        }
        if button.command == Ui::File {
            frame.fill_rect(r.x, r.y, r.width, r.height, 0xff286aa7);
            text(
                frame,
                "File",
                Rect::new(r.x + 14, r.y + 4, r.width - 18, 20),
                0xffffffff,
            );
            continue;
        }
        if hot || selected {
            crate::compositor::aero::menu_item(frame, r, true);
        }
        if let Ui::WordPad(Command::SetColor(color) | Command::SetHighlight(color)) = button.command
        {
            if color != 0 {
                let swatch = Rect::new(r.x + 4, r.y + 4, r.width - 8, r.height - 8);
                frame.fill_rect(swatch.x, swatch.y, swatch.width, swatch.height, color);
                border(frame, swatch, 0xff8293a1);
                continue;
            }
        }
        let status = r.y >= b.y + b.height as i32 - 24;
        if status && matches!(button.command, Ui::WordPad(Command::Zoom(0))) {
            text(
                frame,
                &button.label,
                Rect::new(r.x + 5, r.y + 2, r.width - 8, 19),
                0xff29445b,
            );
        } else if let Some(icon) = icon(&button.command).filter(|_| !menu) {
            let large = r.height > 40;
            let size = if large { 32 } else { 16 };
            let label = large || r.width > 55;
            let x = if large || !label {
                r.x + ((r.width - size) / 2) as i32
            } else {
                r.x + 4
            };
            let y = if large {
                r.y + 3
            } else {
                r.y + ((r.height - size) / 2) as i32
            };
            toolbar::draw(
                frame,
                if large { LARGE_ICONS } else { SMALL_ICONS },
                icon,
                size,
                Rect::new(x, y, size, size),
            );
            if label {
                let label_bounds = if large {
                    Rect::new(r.x + 4, r.y + 39, r.width - 6, 20)
                } else {
                    Rect::new(r.x + 25, r.y + 3, r.width - 27, 20)
                };
                text(frame, &button.label, label_bounds, 0xff223f57);
            } else if hot {
                tooltip = Some((r, button.label.clone()));
            }
            if let Ui::WordPad(Command::ColorMenu | Command::HighlightMenu) = button.command {
                if let AppState::Document(d) = &app.state {
                    let color = if button.command == Ui::WordPad(Command::ColorMenu) {
                        d.current_style().color
                    } else {
                        d.current_style().highlight
                    };
                    frame.fill_rect(
                        r.x + 6,
                        r.y + r.height as i32 - 5,
                        r.width - 12,
                        3,
                        if color == 0 { 0xffffffff } else { color },
                    );
                }
            }
        } else {
            text(
                frame,
                &button.label,
                Rect::new(
                    r.x + 6,
                    r.y + 4,
                    r.width.saturating_sub(if combo { 20 } else { 8 }),
                    r.height - 5,
                ),
                0xff223f57,
            );
            if combo {
                let x = r.x + r.width as i32 - 11;
                frame.fill_rect(x, r.y + 10, 7, 1, 0xff526a7c);
                frame.fill_rect(x + 1, r.y + 11, 5, 1, 0xff526a7c);
                frame.fill_rect(x + 2, r.y + 12, 3, 1, 0xff526a7c);
            }
        }
    }
    if app.wordpad.popup.is_none() && !app.menu {
        if let Some((r, label)) = tooltip {
            let width = (label.len() as u32 * 8 + 14).min(b.width);
            let x = r.x.min(b.x + b.width as i32 - width as i32);
            let y = r.y + r.height as i32 + 6;
            let bounds = Rect::new(x, y, width, 25);
            frame.fill_rect(x, y, width, 25, 0xfffffff0);
            border(frame, bounds, 0xff9daab5);
            text(
                frame,
                &label,
                Rect::new(x + 6, y + 3, width - 10, 20),
                0xff263e50,
            );
        }
    }
}
