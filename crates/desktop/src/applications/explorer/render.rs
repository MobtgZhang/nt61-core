//! Windows 7 shell layout, painted with the project's own software controls.
use super::{file_size, file_type, layout::Layout, Edit, Explorer, Hit, Sort, View};
use crate::{
    compositor::aero,
    file::{FileSystem, NodeKind},
    graphics::{Rect, Surface},
};
use alloc::format;
const INK: u32 = 0xFF1E2938;
const BLUE: u32 = 0xFF234B73;
const MUTED: u32 = 0xFF687888;

struct Painter<'a, F> {
    frame: &'a mut Surface,
    bounds: Rect,
    text: F,
    icons: Option<&'a crate::shell_resources::ShellIcons>,
}
impl<F: Fn(&mut Surface, &str, Rect, u32)> Painter<'_, F> {
    fn fill(&mut self, b: Rect, color: u32) {
        if let Some(r) = b.intersection(&self.bounds) {
            self.frame.fill_rect(r.x, r.y, r.width, r.height, color);
        }
    }
    fn label(&mut self, value: &str, b: Rect, color: u32) {
        if let Some(r) = b.intersection(&self.bounds) {
            if r.height >= 16 {
                (self.text)(self.frame, value, r, color);
            }
        }
    }
    fn line(&mut self, b: Rect, color: u32) {
        self.fill(Rect::new(b.x, b.y, b.width, 1), color);
    }
    fn gradient(&mut self, b: Rect, top: u32, bottom: u32) {
        for y in 0..b.height {
            let t = y * 255 / b.height.max(1);
            let mut color = 0xFF000000;
            for shift in [0, 8, 16] {
                let a = (top >> shift) & 255;
                let z = (bottom >> shift) & 255;
                color |= ((a * (255 - t) + z * t) / 255) << shift;
            }
            self.fill(Rect::new(b.x, b.y + y as i32, b.width, 1), color);
        }
    }
    fn selection(&mut self, r: Rect, selected: bool, hot: bool) {
        if (selected || hot) && r.intersection(&self.bounds) == Some(r) {
            aero::selection(self.frame, r, selected, hot);
        }
    }
    fn icon(&mut self, directory: bool, r: Rect) {
        if r.intersection(&self.bounds) == Some(r) {
            if let Some(icons)=self.icons { icons.draw(self.frame,directory,r); }

        }
    }
    fn border(&mut self, r: Rect, color: u32) {
        self.line(r, color);
        self.line(
            Rect::new(r.x, r.y + r.height.saturating_sub(1) as i32, r.width, 1),
            color,
        );
        self.fill(Rect::new(r.x, r.y, 1, r.height), color);
        self.fill(
            Rect::new(r.x + r.width.saturating_sub(1) as i32, r.y, 1, r.height),
            color,
        );
    }
}
pub fn draw<F: Fn(&mut Surface, &str, Rect, u32)>(
    state: &Explorer,
    files: &FileSystem,
    frame: &mut Surface,
    b: Rect,
    text: F,
) {
    let l = Layout::new(b);
    let mut p = Painter {
        frame,
        bounds: b,
        text,
        icons: files.shell_icons.as_deref(),
    };
    p.fill(b, 0xFFFFFFFF);
    let top = Rect::new(b.x, b.y, b.width, 36.min(b.height));
    p.gradient(top, 0xFFF7FBFF, 0xFFE7EFF9);
    for (r, forward, enabled, hit) in [
        (l.back, false, state.can_back(), Hit::Back),
        (l.forward, true, state.can_forward(), Hit::Forward),
    ] {
        if r.intersection(&b) != Some(r) {
            continue;
        }
        p.frame
            .blend_rounded_rect(r, 13, if enabled { 0xFF8FAECF } else { 0xFFB7C4D1 });
        let inner = Rect::new(
            r.x + 1,
            r.y + 1,
            r.width.saturating_sub(2),
            r.height.saturating_sub(2),
        );
        p.frame.blend_rounded_rect(
            inner,
            12,
            if state.hot == Some(hit) && enabled {
                0xFFE0F4FF
            } else {
                0xFFEBF1F8
            },
        );
        let arrow = Rect::new(r.x + 7, r.y + 7, 12, 12);
        let color = if enabled { BLUE } else { 0xFFA1AAB3 };
        if forward {
            aero::arrow_right(p.frame, arrow, color);
        } else {
            aero::arrow_left(p.frame, arrow, color);
        }
    }
    for r in [l.address, l.refresh, l.search] {
        if r.width > 0 {
            p.fill(r, 0xFFFFFFFF);
            p.border(r, 0xFF93ABC5);
        }
    }
    p.icon(true, Rect::new(l.address.x + 3, l.address.y + 5, 16, 16));
    if let Some(address) = &state.address_edit {
        p.label(
            &format!("{address}|"),
            Rect::new(
                l.address.x + 23,
                l.address.y + 5,
                l.address.width.saturating_sub(26),
                18,
            ),
            INK,
        );
    } else {
        for (r, node) in state.breadcrumbs(files, l.address) {
            if state.hot == Some(Hit::Breadcrumb(node.id)) {
                p.selection(r, false, true);
            }
            p.label(
                &node.name,
                Rect::new(r.x + 5, r.y + 4, r.width.saturating_sub(20), 18),
                INK,
            );
            if r.width >= 20 {
                p.label(
                    ">",
                    Rect::new(r.x + r.width as i32 - 14, r.y + 4, 12, 18),
                    MUTED,
                );
            }
        }
    }
    p.label(
        "\u{21bb}",
        Rect::new(
            l.refresh.x + 4,
            l.refresh.y + 4,
            l.refresh.width.saturating_sub(6),
            18,
        ),
        0xFF3A8554,
    );
    let search = if state.query.is_empty() && state.edit != Some(Edit::Search) {
        StringOrStatic::Static("Search this folder")
    } else {
        StringOrStatic::Owned(format!(
            "{}{}",
            state.query,
            if state.edit == Some(Edit::Search) {
                "|"
            } else {
                ""
            }
        ))
    };
    p.label(
        search.as_str(),
        Rect::new(
            l.search.x + 6,
            l.search.y + 5,
            l.search.width.saturating_sub(12),
            18,
        ),
        if state.query.is_empty() { MUTED } else { INK },
    );
    p.gradient(l.commands, 0xFFF0F6FF, 0xFFD6E4F6);
    p.line(
        Rect::new(
            l.commands.x,
            l.commands.y + l.commands.height.saturating_sub(1) as i32,
            l.commands.width,
            1,
        ),
        0xFFA0B8D4,
    );
    for (i, label) in [
        "Organize  v",
        "Open",
        "New folder",
        if state.view == View::Details {
            "List"
        } else {
            "Details"
        },
    ]
    .iter()
    .enumerate()
    {
        let r = l.command(i);
        if r.width < 40 {
            continue;
        }
        if state.hot == Some(Hit::Command(i)) {
            p.selection(r, false, true);
        }
        p.label(
            label,
            Rect::new(r.x + 10, r.y + 5, r.width.saturating_sub(14), 18),
            if i == 1 && state.selected.is_none() {
                MUTED
            } else {
                BLUE
            },
        );
    }
    // Tree groups follow the reference's 24 px rhythm; unsupported locations are disabled.
    let nav_bounds = l.navigation;
    if nav_bounds.width > 0 {
        p.bounds = nav_bounds;
        for (index, (name, id, indent)) in state.navigation(files).iter().enumerate() {
            let row = Rect::new(
                nav_bounds.x + 2,
                nav_bounds.y + 6 + index as i32 * 24,
                nav_bounds.width.saturating_sub(5),
                24,
            );
            if row.y + 24 > nav_bounds.y + nav_bounds.height as i32 {
                break;
            }
            let selected = id.is_some_and(|id| id == state.directory) && *indent == 1;
            p.selection(
                row,
                selected,
                id.is_some_and(|id| state.hot == Some(Hit::Navigation(id))),
            );
            let x = row.x + 8 + *indent as i32 * 14;
            if id.is_some() {
                p.icon(true, Rect::new(x, row.y + 4, 16, 16));
            } else if *indent == 0 {
                p.label(
                    if index == 0 { "\u{2605}" } else { "\u{25b8}" },
                    Rect::new(x, row.y + 3, 16, 18),
                    if index == 0 { 0xFFC58E2B } else { BLUE },
                );
            }
            p.label(
                name,
                Rect::new(
                    x + 21,
                    row.y + 4,
                    row.width.saturating_sub(35 + *indent * 14),
                    18,
                ),
                if id.is_some() || *indent == 0 {
                    INK
                } else {
                    0xFF919AA5
                },
            );
        }
        p.bounds = b;
        p.fill(
            Rect::new(
                nav_bounds.x + nav_bounds.width.saturating_sub(1) as i32,
                nav_bounds.y,
                1,
                nav_bounds.height,
            ),
            0xFFD6E5F4,
        );
    }
    p.gradient(l.header, 0xFFFFFFFF, 0xFFF0F3F7);
    for (i, label) in ["Name", "Date modified", "Type", "Size"].iter().enumerate() {
        let col = l.column(i);
        if state.view == View::List && i > 0 {
            continue;
        }
        let sorted = matches!(
            (i, state.sort),
            (0, Sort::Name) | (2, Sort::Type) | (3, Sort::Size)
        );
        let label = if sorted {
            format!("{label} {}", if state.descending { "v" } else { "^" })
        } else {
            (*label).into()
        };
        p.label(
            &label,
            Rect::new(col.x + 8, col.y + 4, col.width.saturating_sub(12), 18),
            BLUE,
        );
        p.fill(
            Rect::new(
                col.x + col.width.saturating_sub(1) as i32,
                col.y,
                1,
                col.height,
            ),
            0xFFDDE5EF,
        );
    }
    p.line(
        Rect::new(
            l.header.x,
            l.header.y + l.header.height.saturating_sub(1) as i32,
            l.header.width,
            1,
        ),
        0xFFD8E1EC,
    );
    let rows = state.rows(files);
    let offset = state
        .offset
        .min(rows.len().saturating_sub(l.capacity().max(1)));
    p.bounds = l.list;
    for (index, node) in rows.iter().skip(offset).take(l.capacity()).enumerate() {
        let r = l.row(index);
        p.selection(
            r,
            state.selected == Some(node.id),
            state.hot == Some(Hit::Row(node.id)),
        );
        p.icon(
            node.kind == NodeKind::Directory,
            Rect::new(r.x + 3, r.y + 4, 16, 16),
        );
        let name_width = if state.view == View::Details {
            l.column(0).width.saturating_sub(32)
        } else {
            r.width.saturating_sub(32)
        };
        let label = if state.selected == Some(node.id) && state.edit == Some(Edit::Rename) {
            format!("{}|", state.rename)
        } else {
            node.name.clone()
        };
        p.label(&label, Rect::new(r.x + 25, r.y + 4, name_width, 18), INK);
        if state.view == View::Details {
            // The provider has no timestamp yet; never invent modification dates.
            let col = l.column(2);
            p.label(
                file_type(node),
                Rect::new(col.x + 8, r.y + 4, col.width.saturating_sub(12), 18),
                MUTED,
            );
            if node.kind != NodeKind::Directory {
                let col = l.column(3);
                let size = file_size(node);
                p.label(
                    &format!("{} KB", size.div_ceil(1024)),
                    Rect::new(col.x + 6, r.y + 4, col.width.saturating_sub(10), 18),
                    MUTED,
                );
            }
        }
    }
    if rows.is_empty() {
        p.label(
            if state.query.is_empty() {
                "This folder is empty."
            } else {
                "No items match your search."
            },
            Rect::new(
                l.list.x + 16,
                l.list.y + 16,
                l.list.width.saturating_sub(24),
                20,
            ),
            MUTED,
        );
    }
    p.bounds = b;
    if rows.len() > l.capacity() && l.scroll.height >= 32 {
        p.fill(l.scroll, 0xFFF1F4F8);
        p.border(l.scroll, 0xFFD7DFE8);
        p.label(
            "^",
            Rect::new(l.scroll.x + 3, l.scroll.y + 1, 12, 18),
            MUTED,
        );
        p.label(
            "v",
            Rect::new(
                l.scroll.x + 3,
                l.scroll.y + l.scroll.height as i32 - 19,
                12,
                18,
            ),
            MUTED,
        );
        let track = l.scroll.height.saturating_sub(36);
        let thumb = (track * l.capacity() as u32 / rows.len() as u32)
            .max(12)
            .min(track);
        let travel = track.saturating_sub(thumb);
        let maximum = rows.len().saturating_sub(l.capacity()).max(1);
        let y = l.scroll.y + 18 + (travel as usize * offset / maximum) as i32;
        let r = Rect::new(l.scroll.x + 2, y, l.scroll.width.saturating_sub(4), thumb);
        p.gradient(r, 0xFFE6F1FC, 0xFFC5D9EF);
        p.border(r, 0xFF9FB6D0);
    }
    p.gradient(l.details, 0xFFF5F9FD, 0xFFE2EBF7);
    p.line(l.details, 0xFFCBDFF4);
    if l.details.height >= 48 {
        if let Some(node) = state
            .selected
            .and_then(|id| files.node(id))
            .filter(|n| !n.deleted)
        {
            p.icon(
                node.kind == NodeKind::Directory,
                Rect::new(l.details.x + 14, l.details.y + 8, 32, 32),
            );
            p.label(
                &node.name,
                Rect::new(
                    l.details.x + 62,
                    l.details.y + 5,
                    l.details.width.saturating_sub(74),
                    18,
                ),
                INK,
            );
            let details = if node.kind == NodeKind::Directory {
                StringOrStatic::Static("File folder")
            } else {
                StringOrStatic::Owned(format!(
                    "{}    Size: {} bytes",
                    file_type(node),
                    file_size(node)
                ))
            };
            p.label(
                details.as_str(),
                Rect::new(
                    l.details.x + 62,
                    l.details.y + 27,
                    l.details.width.saturating_sub(74),
                    18,
                ),
                MUTED,
            );
        } else {
            p.icon(true, Rect::new(l.details.x + 14, l.details.y + 8, 32, 32));
            p.label(
                &format!("{} items", rows.len()),
                Rect::new(
                    l.details.x + 62,
                    l.details.y + 16,
                    l.details.width.saturating_sub(74),
                    18,
                ),
                INK,
            );
        }
    }
    p.fill(l.status, 0xFFF4F6F9);
    p.line(l.status, 0xFFD2DCE7);
    p.label(
        &if state.status.is_empty() && files.resource_error.is_some() {
            format!("Shell icon resources unavailable (0x{:08X})",files.resource_error.unwrap_or(0))
        } else if state.status.is_empty() {
            format!(
                "{} items{}",
                rows.len(),
                if !state.query.is_empty() {
                    " (filtered)"
                } else {
                    ""
                }
            )
        } else {
            state.status.clone()
        },
        Rect::new(
            l.status.x + 8,
            l.status.y + 3,
            l.status.width.saturating_sub(16),
            18,
        ),
        MUTED,
    );
    if let Some(menu) = state.menu {
        p.fill(menu, 0xFFF7F7F7);
        p.border(menu, 0xFF9BAABA);
        for (i, label) in [
            "Open",
            "Copy",
            "Paste",
            "New folder",
            "Rename",
            "Delete permanently",
            "Refresh",
            "New Text Document",
        ]
        .iter()
        .enumerate()
        {
            let row = Rect::new(
                menu.x + 2,
                menu.y + i as i32 * 26,
                menu.width.saturating_sub(4),
                26,
            );
            if row.y + 26 > menu.y + menu.height as i32 {
                break;
            }
            if state.hot == Some(Hit::Menu(i)) {
                p.selection(row, false, true);
            }
            let enabled = state.selected.is_some() || matches!(i, 2 | 3 | 6 | 7);
            p.label(
                label,
                Rect::new(row.x + 28, row.y + 5, row.width.saturating_sub(32), 18),
                if enabled { INK } else { MUTED },
            );
        }
    }
    if let Some(id)=state.delete_prompt{
        let (dialog,yes,no)=super::layout::delete_dialog(b);
        p.fill(dialog,0xFFF7FAFD);p.border(dialog,0xFF7B9BB9);
        p.gradient(Rect::new(dialog.x+1,dialog.y+1,dialog.width.saturating_sub(2),28.min(dialog.height)),0xFFEAF3FC,0xFFD4E4F3);
        p.label("Delete permanently",Rect::new(dialog.x+12,dialog.y+6,dialog.width.saturating_sub(24),18),INK);
        let folder=files.node(id).is_some_and(|n|n.kind==NodeKind::Directory);
        p.label(if folder{"Permanently delete this folder and its contents?"}else{"Permanently delete this file?"},Rect::new(dialog.x+16,dialog.y+43,dialog.width.saturating_sub(32),18),BLUE);
        if let Some(node)=files.node(id){p.label(&node.name,Rect::new(dialog.x+16,dialog.y+70,dialog.width.saturating_sub(32),18),INK);}
        for (r,label,accept)in [(yes,"Delete",true),(no,"Cancel",false)]{
            p.gradient(r,0xFFFFFFFF,0xFFE2EAF2);p.border(r,if state.delete_accept==accept{0xFF3286C4}else{0xFFA3B1BF});
            if state.hot==Some(Hit::ConfirmDelete(accept)){p.selection(r,false,true);}
            p.label(label,Rect::new(r.x+14,r.y+4,r.width.saturating_sub(18),18),INK);
        }
    }
}
enum StringOrStatic {
    Static(&'static str),
    Owned(alloc::string::String),
}
impl StringOrStatic {
    fn as_str(&self) -> &str {
        match self {
            Self::Static(s) => s,
            Self::Owned(s) => s,
        }
    }
}
