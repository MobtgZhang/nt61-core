//! Notepad dialogs helpers.
use super::*;
pub(super) fn draw_dialog<F: Fn(&mut Surface, &str, Rect, u32)>(
    frame: &mut Surface,
    f: &F,
    b: Rect,
    n: &Notepad,
    d: &Dialog,
) {
    match d {
        Dialog::GoTo {
            line,
            select,
            error,
        } => {
            let r = dialog_rect(b, 355, 170);
            dialog_frame(frame, f, r, "Go To Line");
            ui_text(f, frame, "Line number:", r.x + 15, r.y + 43, 280);
            field(
                frame,
                f,
                Rect::new(r.x + 15, r.y + 67, r.width - 30, 25),
                line,
                true,
                *select,
            );
            text(
                f,
                frame,
                error,
                Rect::new(r.x + 15, r.y + 98, r.width - 30, 22),
                INK,
            );
            for (i, label) in ["Go To", "Cancel"].iter().enumerate() {
                push_button(
                    frame,
                    f,
                    Rect::new(
                        r.x + r.width as i32 - 180 + i as i32 * 84,
                        r.y + 130,
                        76,
                        26,
                    ),
                    label,
                    i == 0,
                    true,
                );
            }
        }
        Dialog::Font(draft) => {
            let r = dialog_rect(b, 450, 334);
            dialog_frame(frame, f, r, "Font");
            for (x, w, label) in [
                (14, 210, "Font:"),
                (236, 102, "Font style:"),
                (350, 82, "Size:"),
            ] {
                ui_text(f, frame, label, r.x + x, r.y + 42, w);
            }
            field(
                frame,
                f,
                Rect::new(r.x + 14, r.y + 65, 210, 25),
                draft.name(),
                true,
                false,
            );
            field(
                frame,
                f,
                Rect::new(r.x + 236, r.y + 65, 102, 25),
                draft.style_name(),
                false,
                false,
            );
            field(
                frame,
                f,
                Rect::new(r.x + 350, r.y + 65, 82, 25),
                &draft.points.to_string(),
                false,
                false,
            );
            for (i, label) in ["DejaVu Sans Mono", "DejaVu Sans"].iter().enumerate() {
                let br = Rect::new(r.x + 14, r.y + 94 + i as i32 * 24, 210, 24);
                frame.fill_rect(
                    br.x,
                    br.y,
                    br.width,
                    br.height,
                    if draft.sans == (i == 1) {
                        0xFF3399FF
                    } else {
                        0xFFFFFFFF
                    },
                );
                text(
                    f,
                    frame,
                    label,
                    Rect::new(br.x + 4, br.y + 3, br.width - 8, 20),
                    if draft.sans == (i == 1) {
                        0xFFFFFFFF
                    } else {
                        INK
                    },
                );
            }
            for (i, label) in ["Regular", "Bold", "Oblique", "Bold Oblique"]
                .iter()
                .enumerate()
            {
                let br = Rect::new(r.x + 236, r.y + 94 + i as i32 * 24, 102, 24);
                frame.fill_rect(
                    br.x,
                    br.y,
                    br.width,
                    br.height,
                    if draft.style == i as u8 {
                        0xFF3399FF
                    } else {
                        0xFFFFFFFF
                    },
                );
                text(
                    f,
                    frame,
                    label,
                    Rect::new(br.x + 4, br.y + 3, br.width - 8, 20),
                    if draft.style == i as u8 {
                        0xFFFFFFFF
                    } else {
                        INK
                    },
                );
            }
            for (i, size) in [8, 10, 11, 12, 14, 16, 18, 24].iter().enumerate() {
                let br = Rect::new(r.x + 350, r.y + 94 + i as i32 * 20, 82, 20);
                frame.fill_rect(
                    br.x,
                    br.y,
                    br.width,
                    br.height,
                    if draft.points == *size {
                        0xFF3399FF
                    } else {
                        0xFFFFFFFF
                    },
                );
                text(
                    f,
                    frame,
                    &size.to_string(),
                    Rect::new(br.x + 4, br.y + 1, br.width - 8, 18),
                    if draft.points == *size {
                        0xFFFFFFFF
                    } else {
                        INK
                    },
                );
            }
            box_rect(
                frame,
                Rect::new(r.x + 14, r.y + 205, 324, 72),
                0xFFFFFFFF,
                0xFFB0B0B0,
            );
            ui_text(f, frame, "Sample", r.x + 25, r.y + 198, 90);
            if let Ok(font) = n.fonts.get(draft.bytes()) {
                let mut sample = Surface::new(308, 48);
                sample.fill_rect(0, 0, 308, 48, 0xFFFFFFFF);
                let _ = font.draw_text(&mut sample, "AaBbYyZz 0123", 6, 3, draft.pixels(), INK);
                frame.copy_region(&sample, Rect::new(0, 0, 308, 48), r.x + 20, r.y + 220);
            }
            ui_text(f, frame, "Script: Western", r.x + 14, r.y + 286, 180);
            for (i, label) in ["OK", "Cancel"].iter().enumerate() {
                push_button(
                    frame,
                    f,
                    Rect::new(
                        r.x + r.width as i32 - 180 + i as i32 * 84,
                        r.y + 296,
                        76,
                        26,
                    ),
                    label,
                    i == 0,
                    true,
                );
            }
        }
        Dialog::Page {
            draft,
            focus,
            select,
            error,
        } => {
            let r = dialog_rect(b, 460, 382);
            dialog_frame(frame, f, r, "Page Setup");
            ui_text(f, frame, "Paper", r.x + 16, r.y + 39, 100);
            push_button(
                frame,
                f,
                Rect::new(r.x + 18, r.y + 62, 190, 26),
                if draft.letter {
                    "Size: Letter"
                } else {
                    "Size: A4"
                },
                false,
                true,
            );
            ui_text(f, frame, "Source: Automatic", r.x + 240, r.y + 66, 180);
            ui_text(f, frame, "Orientation", r.x + 18, r.y + 105, 150);
            checkbox(
                frame,
                f,
                Rect::new(r.x + 24, r.y + 131, 110, 23),
                "Portrait",
                !draft.landscape,
                true,
            );
            checkbox(
                frame,
                f,
                Rect::new(r.x + 140, r.y + 131, 120, 23),
                "Landscape",
                draft.landscape,
                true,
            );
            ui_text(f, frame, "Margins (millimeters)", r.x + 18, r.y + 165, 250);
            for (i, label) in ["Left:", "Right:", "Top:", "Bottom:"].iter().enumerate() {
                let x = r.x + 18 + (i % 2) as i32 * 218;
                let y = r.y + 190 + (i / 2) as i32 * 32;
                ui_text(f, frame, label, x, y + 4, 65);
                field(
                    frame,
                    f,
                    Rect::new(x + 70, y, 118, 25),
                    &draft.margins[i],
                    *focus == i,
                    *select,
                );
            }
            for (i, (label, value)) in [("Header:", &draft.header), ("Footer:", &draft.footer)]
                .iter()
                .enumerate()
            {
                let y = r.y + 263 + i as i32 * 30;
                ui_text(f, frame, label, r.x + 18, y + 3, 70);
                field(
                    frame,
                    f,
                    Rect::new(r.x + 88, y, r.width - 106, 25),
                    value,
                    *focus == i + 4,
                    *select,
                );
            }
            text(
                f,
                frame,
                error,
                Rect::new(r.x + 18, r.y + 322, r.width - 36, 22),
                INK,
            );
            for (i, label) in ["OK", "Cancel"].iter().enumerate() {
                push_button(
                    frame,
                    f,
                    Rect::new(
                        r.x + r.width as i32 - 180 + i as i32 * 84,
                        r.y + r.height as i32 - 35,
                        76,
                        26,
                    ),
                    label,
                    i == 0,
                    true,
                );
            }
        }
        Dialog::About | Dialog::Help | Dialog::Print | Dialog::Message(_) => {
            let r = dialog_rect(b, 470, if *d == Dialog::Help { 326 } else { 237 });
            dialog_frame(
                frame,
                f,
                r,
                match d {
                    Dialog::About => "About Notepad",
                    Dialog::Help => "Notepad Help",
                    Dialog::Print => "Print",
                    _ => "Notepad",
                },
            );
            let lines: Vec<String> = match d {
                Dialog::About => {
                    crate::applications::icons::draw(
                        frame,
                        crate::applications::AppId::Notepad,
                        Rect::new(r.x + 18, r.y + 45, 48, 48),
                    );
                    alloc::vec![
                        "Notepad".into(),
                        "NT61 Desktop — Version 6.1.7601".into(),
                        "Windows 7 style plain text editor".into(),
                        "Open source implementation for NT61.".into(),
                        format!("Font: {}", n.font.name())
                    ]
                }
                Dialog::Help => [
                    "Create and edit plain text documents.",
                    "Ctrl+N / O / S: New / Open / Save",
                    "Ctrl+Z: Undo; Ctrl+A: Select All",
                    "Ctrl+X / C / V: Cut / Copy / Paste",
                    "Ctrl+F: Find; F3: Find Next",
                    "Ctrl+H: Replace; Ctrl+G: Go To Line",
                    "F5: insert the system time and date",
                    "Format: Word Wrap and Font",
                    "Save As: choose file name and encoding",
                    "Search stops at the start or end of the file.",
                ]
                .iter()
                .map(|s| (*s).into())
                .collect(),
                Dialog::Print => alloc::vec![
                    "No printers are installed.".into(),
                    "Printing is unavailable on this system.".into(),
                    "No print job has been submitted.".into()
                ],
                Dialog::Message(message) => alloc::vec![message.clone()],
                _ => Vec::new(),
            };
            let x = if *d == Dialog::About { 80 } else { 18 };
            for (i, line) in lines.iter().enumerate() {
                ui_text(
                    f,
                    frame,
                    line,
                    r.x + x,
                    r.y + 45 + i as i32 * 23,
                    r.width.saturating_sub(x as u32 + 12),
                );
            }
            push_button(
                frame,
                f,
                Rect::new(
                    r.x + r.width as i32 - 92,
                    r.y + r.height as i32 - 38,
                    76,
                    26,
                ),
                "OK",
                true,
                true,
            );
        }
    }
}
pub(super) fn dialog_pointer(app: &mut ApplicationSession, b: Rect, x: i32, y: i32) -> bool {
    let Some(mut dialog) = app.notepad.dialog.take() else {
        return false;
    };
    let mut enter = false;
    let mut cancel = false;
    let r = match dialog {
        Dialog::GoTo { .. } => dialog_rect(b, 355, 170),
        Dialog::Font(_) => dialog_rect(b, 450, 334),
        Dialog::Page { .. } => dialog_rect(b, 460, 382),
        Dialog::Help => dialog_rect(b, 470, 326),
        _ => dialog_rect(b, 470, 237),
    };
    if Rect::new(r.x + r.width as i32 - 30, r.y + 4, 25, 21).contains(x, y) {
        cancel = true;
    }
    match &mut dialog {
        Dialog::GoTo { select, .. } => {
            if Rect::new(r.x + 15, r.y + 67, r.width - 30, 25).contains(x, y) {
                *select = true;
            }
            enter |= Rect::new(r.x + r.width as i32 - 180, r.y + 130, 76, 26).contains(x, y);
            cancel |= Rect::new(r.x + r.width as i32 - 96, r.y + 130, 76, 26).contains(x, y);
        }
        Dialog::Font(draft) => {
            if Rect::new(r.x + 14, r.y + 94, 210, 48).contains(x, y) {
                draft.sans = y >= r.y + 118;
            }
            if Rect::new(r.x + 236, r.y + 94, 102, 96).contains(x, y) {
                draft.style = ((y - r.y - 94) / 24) as u8;
            }
            if Rect::new(r.x + 350, r.y + 94, 82, 160).contains(x, y) {
                draft.points = [8, 10, 11, 12, 14, 16, 18, 24][((y - r.y - 94) / 20) as usize];
            }
            enter |= Rect::new(r.x + r.width as i32 - 180, r.y + 296, 76, 26).contains(x, y);
            cancel |= Rect::new(r.x + r.width as i32 - 96, r.y + 296, 76, 26).contains(x, y);
        }
        Dialog::Page {
            draft,
            focus,
            select,
            ..
        } => {
            if Rect::new(r.x + 18, r.y + 62, 190, 26).contains(x, y) {
                draft.letter = !draft.letter;
            }
            if Rect::new(r.x + 24, r.y + 131, 110, 23).contains(x, y) {
                draft.landscape = false;
            }
            if Rect::new(r.x + 140, r.y + 131, 120, 23).contains(x, y) {
                draft.landscape = true;
            }
            for i in 0..6 {
                let br = if i < 4 {
                    Rect::new(
                        r.x + 88 + (i % 2) as i32 * 218,
                        r.y + 190 + (i / 2) as i32 * 32,
                        118,
                        25,
                    )
                } else {
                    Rect::new(r.x + 88, r.y + 263 + (i - 4) as i32 * 30, r.width - 106, 25)
                };
                if br.contains(x, y) {
                    *focus = i;
                    *select = true;
                }
            }
            enter |= Rect::new(
                r.x + r.width as i32 - 180,
                r.y + r.height as i32 - 35,
                76,
                26,
            )
            .contains(x, y);
            cancel |= Rect::new(
                r.x + r.width as i32 - 96,
                r.y + r.height as i32 - 35,
                76,
                26,
            )
            .contains(x, y);
        }
        _ => {
            enter |= Rect::new(
                r.x + r.width as i32 - 92,
                r.y + r.height as i32 - 38,
                76,
                26,
            )
            .contains(x, y)
        }
    }
    app.notepad.dialog = Some(dialog);
    if cancel {
        app.notepad.dialog = None;
    } else if enter {
        key(app, 13);
    }
    true
}
