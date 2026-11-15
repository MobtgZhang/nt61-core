//! Notepad menus behavior.
use super::*;
pub fn menu(kind: u8) -> &'static [Item] {
    use Command::*;
    const FILE: &[Item] = &[
        item("New", "Ctrl+N", New),
        item("Open...", "Ctrl+O", Open),
        item("Save", "Ctrl+S", Save),
        item("Save As...", "", SaveAs),
        SEP,
        item("Page Setup...", "", Page),
        item("Print...", "Ctrl+P", Print),
        SEP,
        item("Exit", "", Exit),
    ];
    const EDIT: &[Item] = &[
        item("Undo", "Ctrl+Z", Undo),
        SEP,
        item("Cut", "Ctrl+X", Cut),
        item("Copy", "Ctrl+C", Copy),
        item("Paste", "Ctrl+V", Paste),
        item("Delete", "Del", Delete),
        SEP,
        item("Find...", "Ctrl+F", Find),
        item("Find Next", "F3", Next),
        item("Replace...", "Ctrl+H", Replace),
        item("Go To...", "Ctrl+G", GoTo),
        SEP,
        item("Select All", "Ctrl+A", SelectAll),
        item("Time/Date", "F5", Time),
    ];
    const CONTEXT: &[Item] = &[
        item("Undo", "", Undo),
        SEP,
        item("Cut", "", Cut),
        item("Copy", "", Copy),
        item("Paste", "", Paste),
        item("Delete", "", Delete),
        SEP,
        item("Select All", "", SelectAll),
    ];
    const FORMAT: &[Item] = &[item("Word Wrap", "", Wrap), item("Font...", "", Font)];
    const VIEW: &[Item] = &[item("Status Bar", "", Status)];
    const HELP: &[Item] = &[
        item("View Help", "", Help),
        SEP,
        item("About Notepad", "", About),
    ];
    match kind {
        0 => FILE,
        1 => EDIT,
        2 => FORMAT,
        3 => VIEW,
        5 => CONTEXT,
        _ => HELP,
    }
}
pub fn enabled(d: &Document, n: &Notepad, c: Command) -> bool {
    use Command::*;
    match c {
        Undo => d.can_undo(),
        Cut | Copy | Delete => d.selection().is_some(),
        Paste => n.clipboard,
        Find | Next | Replace => !d.editor.text.is_empty(),
        GoTo | Status => !d.word_wrap,
        _ => true,
    }
}
const MENU_X: [i32; 5] = [0, 38, 76, 138, 179];
const MENU_W: [u32; 5] = [38, 38, 62, 41, 42];
pub fn menu_bar(b: Rect, kind: usize) -> Rect {
    Rect::new(b.x + MENU_X[kind], b.y, MENU_W[kind], 21)
}
pub fn menu_rects(b: Rect, kind: u8) -> Vec<(Rect, Item)> {
    let mut y = b.y + 24;
    let width = if kind == 1 { 246 } else { 232 }.min(b.width);
    let x = (b.x + MENU_X[kind.min(4) as usize]).min(b.x + b.width as i32 - width as i32);
    menu(kind)
        .iter()
        .map(|item| {
            let h = if item.command.is_some() { 22 } else { 7 };
            let r = Rect::new(x + 3, y, width.saturating_sub(6), h);
            y += h as i32;
            (r, *item)
        })
        .collect()
}
pub fn popup_rects(app: &ApplicationSession, b: Rect) -> Vec<(Rect, Item)> {
    let mut rects = menu_rects(b, app.menu_kind);
    if let Some((x, y)) = app.notepad.context.filter(|_| app.menu_kind == 5) {
        let height = rects
            .last()
            .map_or(0, |(r, _)| r.y + r.height as i32 - b.y - 24);
        let target_x = x.min(b.x + b.width as i32 - 232).max(b.x);
        let target_y = y.min(b.y + b.height as i32 - height - 4).max(b.y);
        for (r, _) in &mut rects {
            r.x += target_x - b.x - MENU_X[4];
            r.y += target_y - b.y - 24;
        }
    }
    rects
}
