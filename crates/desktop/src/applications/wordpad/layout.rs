//! Rich text measurement, wrapping and caret geometry.
use super::*;
pub(super) fn pixels(s: Style, zoom: u16) -> u32 {
    (u32::from(s.size) * 2 * u32::from(zoom) / 300).clamp(5, 384)
}
pub(super) fn font(app: &ApplicationSession, s: Style) -> Option<Rc<FontLib>> {
    app.notepad
        .fonts
        .get(
            crate::applications::notepad::Font {
                sans: !s.mono,
                style: u8::from(s.bold) + 2 * u8::from(s.italic),
                points: 11,
            }
            .bytes(),
        )
        .ok()
}
pub(super) fn viewport(b: Rect, w: &WordPad) -> Rect {
    let y = if w.ruler { 153 } else { 129 };
    Rect::new(
        b.x + 1,
        b.y + y,
        b.width.saturating_sub(18),
        b.height
            .saturating_sub(y as u32 + if w.status { 25 } else { 2 }),
    )
}
pub(super) fn page(r: Rect, zoom: u16) -> Rect {
    let width = (816 * u32::from(zoom) / 100)
        .min(r.width.saturating_sub(24))
        .max(1);
    Rect::new(
        r.x + (r.width.saturating_sub(width) / 2) as i32,
        r.y,
        width,
        r.height,
    )
}
pub(super) fn margin(p: Rect, zoom: u16) -> u32 {
    (64 * u32::from(zoom) / 100).min(p.width / 5)
}
pub(super) fn layout(app: &ApplicationSession, d: &Document, width: u32) -> Vec<Line> {
    let mut lines = Vec::new();
    let chars: Vec<_> = d.editor.text.chars().collect();
    let mut start = 0;
    let mut y = 16;
    loop {
        let mut cells = Vec::new();
        let mut x = 0u32;
        let mut end = start;
        let mut height = 22;
        let mut last_space = None;
        while end < chars.len() && chars[end] != '\n' {
            let ch = chars[end];
            let s = d
                .rich
                .as_ref()
                .map_or_else(Style::default, |r| r.style(end));
            let px = pixels(s, app.wordpad.zoom);
            let f = font(app, s);
            let advance = if ch == '\t' {
                let tab = (px * 4).max(1);
                tab - x % tab
            } else {
                f.as_ref()
                    .and_then(|f| f.glyph_metrics(ch, px))
                    .map_or(px / 2 + 1, |m| (m.advance_width + 0.5).max(1.) as u32)
            };
            let indent = u32::from(s.indent) * u32::from(app.wordpad.zoom) / 1500
                + if s.bullet { px * 2 } else { 0 };
            if d.word_wrap && x + advance > width.saturating_sub(indent) && !cells.is_empty() {
                if let Some((i, len)) = last_space {
                    end = i;
                    cells.truncate(len);
                }
                break;
            }
            height = height.max(
                f.as_ref()
                    .and_then(|f| f.line_metrics(px))
                    .map_or(px + 6, |m| m.line_height + 3),
            );
            cells.push(Cell {
                index: end,
                ch,
                x,
                width: advance,
                style: s,
            });
            x += advance;
            end += 1;
            if ch == ' ' || ch == '\t' {
                last_space = Some((end, cells.len()));
            }
        }
        let s = d
            .rich
            .as_ref()
            .map_or_else(Style::default, |r| r.style(start));
        let used = cells.last().map_or(0, |c| c.x + c.width);
        let list_indent = if s.bullet {
            pixels(s, app.wordpad.zoom) * 2
        } else {
            0
        };
        let indent =
            (u32::from(s.indent) * u32::from(app.wordpad.zoom) / 1500).min(width / 2) + list_indent;
        let extra = width.saturating_sub(indent + used);
        let offset = indent
            + match s.align {
                1 => extra / 2,
                2 => extra,
                _ => 0,
            };
        for c in &mut cells {
            c.x += offset;
        }
        let newline = chars.get(end) == Some(&'\n');
        lines.push(Line {
            start,
            end,
            y,
            height,
            width: used + offset,
            cells,
            bullet: s.bullet && (start == 0 || chars.get(start - 1) == Some(&'\n')),
        });
        y += height;
        if end >= chars.len() {
            break;
        }
        start = end + usize::from(newline);
    }
    lines
}
pub(super) fn with_lines<T>(
    app: &ApplicationSession,
    d: &Document,
    width: u32,
    f: impl FnOnce(&[Line]) -> T,
) -> T {
    let mut c = app.wordpad.cache.borrow_mut();
    if c.lines.is_empty()
        || c.revision != d.revision
        || c.width != width
        || c.zoom != app.wordpad.zoom
        || c.wrap != d.word_wrap
    {
        *c = Cache {
            revision: d.revision,
            width,
            zoom: app.wordpad.zoom,
            wrap: d.word_wrap,
            lines: layout(app, d, width),
        };
    }
    f(&c.lines)
}
pub(super) fn caret(lines: &[Line], index: usize) -> (usize, u32) {
    let row = lines.iter().rposition(|l| l.start <= index).unwrap_or(0);
    let l = &lines[row];
    let x = l
        .cells
        .iter()
        .find(|c| c.index == index)
        .map_or(l.width, |c| c.x);
    (row, x)
}
pub(super) fn top(app: &ApplicationSession, d: &Document, lines: &[Line], height: u32) -> u32 {
    let last = lines.last().map_or(0, |l| l.y + l.height);
    let mut top = app.wordpad.top.min(last.saturating_sub(height));
    if !app.wordpad.manual_scroll {
        let (row, _) = caret(lines, d.editor.cursor);
        let l = &lines[row];
        if l.y < top + 8 {
            top = l.y.saturating_sub(8);
        } else if l.y + l.height > top + height {
            top = (l.y + l.height).saturating_sub(height);
        }
    }
    top
}
