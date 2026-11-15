//! Notepad layout helpers.
use super::*;
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Cell {
    pub(super) index: usize,
    pub(super) ch: char,
    pub(super) x: u32,
    pub(super) width: u32,
}
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Line {
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) cells: Vec<Cell>,
    pub(super) width: u32,
    pub(super) newline: bool,
}
#[derive(Clone, Debug)]
pub(super) struct CachedLayout {
    pub(super) text: String,
    pub(super) font: Font,
    pub(super) width: u32,
    pub(super) wrap: bool,
    pub(super) lines: Rc<Vec<Line>>,
}
#[derive(Clone, Debug, Default)]
pub(super) struct LayoutCache(pub(super) RefCell<Option<CachedLayout>>);
impl PartialEq for LayoutCache {
    fn eq(&self, _: &Self) -> bool {
        // Derived rendering resources do not affect application state.
        true
    }
}
impl Notepad {
    pub(super) fn lines(&self, d: &Document, font: &FontLib, width: u32) -> Rc<Vec<Line>> {
        let mut cache = self.layout_cache.0.borrow_mut();
        if let Some(cached) = cache.as_ref() {
            if cached.font == self.font
                && cached.width == width
                && cached.wrap == d.word_wrap
                && cached.text == d.editor.text
            {
                return Rc::clone(&cached.lines);
            }
        }
        let lines = Rc::new(layout(d, font, self.font.pixels(), width));
        *cache = Some(CachedLayout {
            text: d.editor.text.clone(),
            font: self.font,
            width,
            wrap: d.word_wrap,
            lines: Rc::clone(&lines),
        });
        lines
    }
}
pub(super) fn advance(font: &FontLib, size: u32, ch: char, x: u32, space: u32) -> u32 {
    if ch == '\t' {
        let tab = space * 8;
        tab - x % tab
    } else {
        font.glyph_metrics(ch, size)
            .map_or(space, |m| (m.advance_width + 0.5) as u32)
    }
}
pub(super) fn layout(d: &Document, font: &FontLib, size: u32, width: u32) -> Vec<Line> {
    let chars: Vec<_> = d.editor.text.chars().collect();
    let space = font.measure(" ", size).map_or(8, |m| m.width).max(1);
    let mut lines = Vec::new();
    let mut at = 0;
    loop {
        let start = at;
        let mut x = 0;
        let mut cells = Vec::new();
        let mut last_break = None;
        while at < chars.len() && chars[at] != '\n' {
            let w = advance(font, size, chars[at], x, space);
            if d.word_wrap && x > 0 && x + w > width.max(1) {
                if let Some((index, count)) = last_break {
                    at = index;
                    cells.truncate(count);
                    x = cells.last().map_or(0, |c: &Cell| c.x + c.width);
                }
                break;
            }
            cells.push(Cell {
                index: at,
                ch: chars[at],
                x,
                width: w,
            });
            x += w;
            at += 1;
            if chars[at - 1].is_whitespace() {
                last_break = Some((at, cells.len()));
            }
        }
        let end = at;
        lines.push(Line {
            start,
            end,
            cells,
            width: x,
            newline: chars.get(at) == Some(&'\n'),
        });
        if at == chars.len() {
            break;
        }
        if chars[at] == '\n' {
            at += 1;
            if at == chars.len() {
                lines.push(Line {
                    start: at,
                    end: at,
                    cells: Vec::new(),
                    width: 0,
                    newline: false,
                });
                break;
            }
        }
    }
    lines
}
pub fn editor_bounds(b: Rect, d: &Document, n: &Notepad) -> Rect {
    Rect::new(
        b.x,
        b.y + 22,
        b.width.saturating_sub(17),
        b.height.saturating_sub(
            22 + if !d.word_wrap { 17 } else { 0 }
                + if n.status_bar && !d.word_wrap { 23 } else { 0 },
        ),
    )
}
pub(super) fn caret(lines: &[Line], index: usize, visual_end: Option<usize>) -> (usize, u32) {
    for (row, line) in lines.iter().enumerate() {
        if index <= line.end
            && (index < line.end
                || visual_end == Some(index)
                || lines.get(row + 1).is_none_or(|next| next.start != index))
        {
            return (
                row,
                line.cells
                    .iter()
                    .find(|c| c.index == index)
                    .map_or(line.width, |c| c.x),
            );
        }
    }
    (lines.len().saturating_sub(1), 0)
}
pub(super) fn viewport(
    n: &Notepad,
    lines: &[Line],
    d: &Document,
    r: Rect,
    height: u32,
) -> (usize, u32) {
    let visible = (r.height.saturating_sub(2) / height).max(1) as usize;
    let (row, x) = caret(lines, d.editor.cursor, n.visual_end);
    let mut top = n.top.min(lines.len().saturating_sub(visible));
    let mut left = n.left;
    if n.follow {
        if row < top {
            top = row;
        }
        if row >= top + visible {
            top = row + 1 - visible;
        }
        if x < left {
            left = x;
        }
        if x >= left + r.width.saturating_sub(5) {
            left = x.saturating_sub(r.width.saturating_sub(6));
        }
    }
    (top, if d.word_wrap { 0 } else { left })
}
pub(super) fn line_height(font: &FontLib, size: u32) -> u32 {
    font.line_metrics(size)
        .map_or(size + 3, |m| m.line_height)
        .max(size + 2)
}
