//! Vista/Windows 7 common item dialog using the same shell artwork as Explorer.
use super::{encoding_label, layout::Layout, FileDialog, Focus, Mode};
use crate::{
    applications::explorer::theme,
    compositor::aero,
    graphics::{Rect, Surface},
};
use alloc::format;

fn border(frame: &mut Surface, b: Rect, fill: u32, edge: u32) {
    frame.fill_rect(b.x, b.y, b.width, b.height, edge);
    frame.fill_rect(
        b.x + 1,
        b.y + 1,
        b.width.saturating_sub(2),
        b.height.saturating_sub(2),
        fill,
    );
}
fn label<F: Fn(&mut Surface, &str, Rect, u32)>(
    f: &F,
    frame: &mut Surface,
    value: &str,
    b: Rect,
    color: u32,
) {
    if b.width > 8 && b.height >= 18 {
        f(
            frame,
            value,
            Rect::new(b.x + 6, b.y + 4, b.width - 8, 18),
            color,
        );
    }
}
pub fn draw<F: Fn(&mut Surface, &str, Rect, u32)>(
    d: &FileDialog,
    frame: &mut Surface,
    body: Rect,
    text: &F,
) {
    let l = Layout::new(body);
    let b = l.bounds;
    let mut glass = aero::WindowGlassCache::default();
    glass.prepare(frame, 0, b, 0, true);
    aero::shadow(frame, b, 8);
    border(frame, b, 0xFFFFFFFF, 0xFF8BA7C2);
    glass.paint(frame, 0, true);
    aero::window_border(frame, b, 5);
    theme::icon(frame, "Folder", Rect::new(b.x + 9, b.y + 6, 16, 16));
    label(
        text,
        frame,
        if d.mode == Mode::Open {
            "Open"
        } else {
            "Save As"
        },
        Rect::new(b.x + 28, b.y + 1, b.width.saturating_sub(32), 26),
        0xFF172635,
    );
    theme::navigation(frame, l.back, false, d.can_back(), false, false);
    theme::navigation(frame, l.forward, true, d.can_forward(), false, false);
    border(
        frame,
        l.address,
        0xFFFFFFFF,
        if d.focus == Focus::Address {
            0xFF3C7FB1
        } else {
            0xFF98ADBF
        },
    );
    theme::icon(
        frame,
        "Folder",
        Rect::new(l.address.x + 4, l.address.y + 4, 16, 16),
    );
    label(
        text,
        frame,
        &d.address,
        Rect::new(
            l.address.x + 22,
            l.address.y,
            l.address.width.saturating_sub(26),
            25,
        ),
        0xFF172635,
    );
    frame.fill_rect(b.x + 1, b.y + 65, b.width.saturating_sub(2), 25, 0xFFF0F4FA);
    label(
        text,
        frame,
        "Organize",
        Rect::new(b.x + 8, b.y + 66, 84, 24),
        0xFF234B73,
    );
    label(text, frame, "New folder", l.new_folder, 0xFF234B73);
    if d.focus == Focus::NewFolder {
        let edit = Rect::new(
            l.new_folder.x + 114,
            l.new_folder.y,
            b.width.saturating_sub(230),
            24,
        );
        border(frame, edit, 0xFFFFFFFF, 0xFF3C7FB1);
        label(text, frame, &d.folder_name, edit, 0xFF172635);
    }
    if l.navigation.width > 0 {
        label(
            text,
            frame,
            "Favorites",
            Rect::new(
                l.navigation.x + 22,
                l.navigation.y + 3,
                l.navigation.width.saturating_sub(26),
                24,
            ),
            0xFF172635,
        );
        theme::icon(
            frame,
            "Favorites",
            Rect::new(l.navigation.x + 8, l.navigation.y + 7, 16, 16),
        );
        for (i, (name, id)) in d.locations.iter().enumerate() {
            let row = Rect::new(
                l.navigation.x + 4,
                l.navigation.y + 30 + i as i32 * 26,
                l.navigation.width.saturating_sub(8),
                24,
            );
            if row.y + 24 > l.navigation.y + l.navigation.height as i32 {
                break;
            }
            if d.directory == *id {
                aero::selection(frame, row, true, false);
            }
            theme::icon(frame, name, Rect::new(row.x + 14, row.y + 4, 16, 16));
            label(
                text,
                frame,
                name,
                Rect::new(row.x + 32, row.y, row.width.saturating_sub(34), 24),
                0xFF172635,
            );
        }
        frame.fill_rect(
            l.navigation.x + l.navigation.width as i32,
            l.navigation.y,
            1,
            l.navigation.height,
            0xFFD6E5F4,
        );
    }
    frame.fill_rect(
        l.header.x,
        l.header.y,
        l.header.width,
        l.header.height,
        0xFFF7F9FC,
    );
    label(
        text,
        frame,
        if d.sort_descending {
            "Name ▾"
        } else {
            "Name ▴"
        },
        l.header,
        0xFF45617D,
    );
    if l.header.width >= 340 {
        label(
            text,
            frame,
            "Type",
            Rect::new(
                l.header.x + l.header.width as i32 * 2 / 3,
                l.header.y,
                l.header.width / 3,
                24,
            ),
            0xFF45617D,
        );
    }
    for (i, entry) in d
        .entries
        .iter()
        .skip(d.offset())
        .take((l.list.height / 24) as usize)
        .enumerate()
    {
        let row = Rect::new(
            l.list.x + 3,
            l.list.y + i as i32 * 24,
            l.list.width.saturating_sub(6),
            24,
        );
        if d.selected() == Some(i + d.offset()) {
            aero::selection(frame, row, true, false);
        }
        theme::icon(
            frame,
            if entry.directory {
                "Folder"
            } else {
                "Document"
            },
            Rect::new(row.x + 3, row.y + 4, 16, 16),
        );
        let name_width = if row.width >= 340 {
            row.width * 2 / 3 - 24
        } else {
            row.width.saturating_sub(26)
        };
        label(
            text,
            frame,
            &entry.name,
            Rect::new(row.x + 21, row.y, name_width, 24),
            0xFF172635,
        );
        if row.width >= 340 {
            label(
                text,
                frame,
                if entry.directory {
                    "File folder"
                } else {
                    "File"
                },
                Rect::new(
                    l.header.x + l.header.width as i32 * 2 / 3,
                    row.y,
                    l.header.width / 3,
                    24,
                ),
                0xFF606B77,
            );
        }
    }
    if let Some(bar) = super::super::shared::scrollbar::Scrollbar::new(
        l.scroll,
        d.entries.len(),
        (l.list.height / 24).max(1) as usize,
        d.offset(),
    ) {
        bar.draw(frame, false, d.captures_pointer());
    }
    let footer_y = l.name.y - 10;
    frame.fill_rect(
        b.x + 1,
        footer_y,
        b.width.saturating_sub(2),
        (b.y + b.height as i32 - footer_y - 1).max(0) as u32,
        0xFFF0F0F0,
    );
    for (title, field, value, focused) in [
        (
            "File name:",
            l.name,
            d.name.as_str(),
            d.focus == Focus::Name,
        ),
        (
            "File type:",
            l.filter,
            d.filter_label(),
            d.focus == Focus::Filter,
        ),
    ] {
        label(
            text,
            frame,
            title,
            Rect::new(b.x + 8, field.y, 90, 25),
            0xFF172635,
        );
        border(
            frame,
            field,
            0xFFFFFFFF,
            if focused { 0xFF3C7FB1 } else { 0xFFABADB3 },
        );
        label(text, frame, value, field, 0xFF172635);
    }
    for row in 0..4 {
        frame.fill_rect(
            l.filter.x + l.filter.width.saturating_sub(18) as i32 + row,
            l.filter.y + 10 + row,
            (7 - row * 2) as u32,
            1,
            0xFF253545,
        );
    }
    if d.text_encoding && d.overwrite.is_none() {
        let encoding = encoding_label(d.encoding);
        border(
            frame,
            l.encoding,
            0xFFFFFFFF,
            if d.focus == Focus::Encoding {
                0xFF3C7FB1
            } else {
                0xFFABADB3
            },
        );
        label(
            text,
            frame,
            &format!("Encoding: {encoding}"),
            l.encoding,
            0xFF172635,
        );
    }
    if !d.status.is_empty() {
        frame.fill_rect(
            l.status.x,
            l.status.y,
            l.status.width,
            l.status.height,
            0xFFFFF9E7,
        );
        label(text, frame, &d.status, l.status, 0xFF8A411A);
    }
    for (r, value, default) in [
        (
            l.accept,
            if d.overwrite.is_some() {
                "Replace"
            } else if d.mode == Mode::Open {
                "Open"
            } else {
                "Save"
            },
            true,
        ),
        (
            l.cancel,
            if d.overwrite.is_some() {
                "Keep file"
            } else {
                "Cancel"
            },
            false,
        ),
    ] {
        border(
            frame,
            r,
            0xFFE8E8E8,
            if default { 0xFF3C7FB1 } else { 0xFF8E8F8F },
        );
        frame.fill_rect(
            r.x + 2,
            r.y + 2,
            r.width.saturating_sub(4),
            r.height / 2,
            0xFFFAFAFA,
        );
        label(
            text,
            frame,
            value,
            Rect::new(r.x + 12, r.y + 1, r.width.saturating_sub(18), r.height),
            0xFF172635,
        );
    }
    d.popup.draw(frame, text);
}
