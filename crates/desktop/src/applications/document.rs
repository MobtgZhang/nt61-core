//! Editable documents, clipboard operations and a conservative RTF writer.
use super::rich_text::{RichText, Style};
use crate::widgets::textbox::TextBox;
use alloc::{format, string::String, vec::Vec};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    Ansi,
    Utf8,
    Utf8Bom,
    Utf16Le,
    Utf16Be,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Edit {
    text: String,
    cursor: usize,
    anchor: Option<usize>,
    rich: Option<RichText>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Document {
    pub editor: TextBox,
    pub bold: bool,
    pub italic: bool,
    pub dirty: bool,
    pub filename: String,
    pub file_id: Option<u32>,
    pub encoding: Encoding,
    pub crlf: bool,
    pub anchor: Option<usize>,
    pub scroll: usize,
    pub word_wrap: bool,
    pub drag_anchor: Option<usize>,
    pub revision: u64,
    pub rich: Option<RichText>,
    undo: Vec<Edit>,
    redo: Vec<Edit>,
    saved: String,
    saved_styles: Vec<Style>,
}
impl Document {
    pub fn new(filename: &str) -> Self {
        let mut editor = TextBox::new(0);
        editor.focused = true;
        Self {
            editor,
            bold: false,
            italic: false,
            dirty: false,
            filename: filename.into(),
            file_id: None,
            encoding: Encoding::Utf8,
            crlf: true,
            anchor: None,
            scroll: 0,
            word_wrap: false,
            drag_anchor: None,
            revision: 1,
            rich: None,
            undo: Vec::new(),
            redo: Vec::new(),
            saved: String::new(),
            saved_styles: Vec::new(),
        }
    }
    fn snapshot(&self) -> Edit {
        Edit {
            text: self.editor.text.clone(),
            cursor: self.editor.cursor,
            anchor: self.anchor,
            rich: self.rich.clone(),
        }
    }
    fn checkpoint(&mut self) {
        while self.undo.len() >= 64
            || (!self.undo.is_empty()
                && self
                    .undo
                    .iter()
                    .map(|e| {
                        e.text.len()
                            + e.rich
                                .as_ref()
                                .map_or(0, |r| r.styles.len() * core::mem::size_of::<Style>())
                    })
                    .sum::<usize>()
                    > 4 * 1024 * 1024)
        {
            self.undo.remove(0);
        }
        self.undo.push(self.snapshot());
        self.redo.clear();
    }
    fn changed(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        self.dirty = self.editor.text != self.saved
            || self
                .rich
                .as_ref()
                .is_some_and(|r| r.styles != self.saved_styles);
    }
    fn byte(&self, ch: usize) -> usize {
        self.editor
            .text
            .char_indices()
            .nth(ch)
            .map_or(self.editor.text.len(), |(i, _)| i)
    }
    pub fn selection(&self) -> Option<(usize, usize)> {
        let a = self.anchor?;
        let c = self.editor.cursor;
        (a != c).then_some((a.min(c), a.max(c)))
    }
    pub fn selected_text(&self) -> String {
        self.selection().map_or_else(String::new, |(a, b)| {
            self.editor.text[self.byte(a)..self.byte(b)].into()
        })
    }
    fn delete_selection(&mut self) -> bool {
        let Some((a, b)) = self.selection() else {
            self.anchor = None;
            return false;
        };
        if let Some(r) = &mut self.rich {
            r.splice(a, b, 0);
        }
        self.editor
            .text
            .replace_range(self.byte(a)..self.byte(b), "");
        self.editor.cursor = a;
        self.anchor = None;
        true
    }
    pub fn select_all(&mut self) {
        self.anchor = Some(0);
        self.editor.cursor = self.editor.text.chars().count();
    }
    pub fn cut(&mut self) -> String {
        let text = self.selected_text();
        if !text.is_empty() {
            self.checkpoint();
            self.delete_selection();
            self.changed();
        }
        text
    }
    pub fn position(&self) -> (usize, usize) {
        let prefix = &self.editor.text[..self.byte(self.editor.cursor)];
        (
            prefix.bytes().filter(|b| *b == b'\n').count(),
            prefix.rsplit('\n').next().unwrap_or("").chars().count(),
        )
    }
    pub fn move_to(&mut self, line: usize, column: usize) {
        let mut index = 0;
        for (i, s) in self.editor.text.split('\n').enumerate() {
            if i == line {
                self.editor.cursor = index + column.min(s.chars().count());
                self.anchor = None;
                return;
            }
            index += s.chars().count() + 1;
        }
        self.editor.cursor = self.editor.text.chars().count();
        self.anchor = None;
    }
    pub fn key(&mut self, key: u8) -> bool {
        use super::explorer::input;
        if (input::SHIFT_UP..=input::SHIFT_END).contains(&key) {
            let anchor = self.anchor.take().unwrap_or(self.editor.cursor);
            let nav = match key {
                input::SHIFT_UP => input::UP,
                input::SHIFT_DOWN => input::DOWN,
                input::SHIFT_LEFT => input::LEFT,
                input::SHIFT_RIGHT => input::RIGHT,
                input::SHIFT_HOME => input::HOME,
                _ => input::END,
            };
            self.key(nav);
            self.anchor = Some(anchor);
            return true;
        }
        if key == input::CTRL_HOME {
            self.editor.cursor = 0;
            self.anchor = None;
            return true;
        }
        if key == input::CTRL_END {
            self.editor.cursor = self.editor.text.chars().count();
            self.anchor = None;
            return true;
        }
        match key {
            input::LEFT => {
                if let Some((start, _)) = self.selection() {
                    self.editor.cursor = start;
                } else {
                    self.editor.move_cursor_left();
                }
                self.anchor = None;
                return true;
            }
            input::RIGHT => {
                if let Some((_, end)) = self.selection() {
                    self.editor.cursor = end;
                } else {
                    self.editor.move_cursor_right();
                }
                self.anchor = None;
                return true;
            }
            input::HOME
            | input::END
            | input::UP
            | input::DOWN
            | input::PAGE_UP
            | input::PAGE_DOWN => {
                let (line, col) = self.position();
                match key {
                    input::HOME => self.move_to(line, 0),
                    input::END => self.move_to(line, usize::MAX),
                    input::UP => self.move_to(line.saturating_sub(1), col),
                    input::DOWN => self.move_to(line + 1, col),
                    input::PAGE_UP => self.move_to(line.saturating_sub(12), col),
                    _ => self.move_to(line + 12, col),
                }
                return true;
            }
            1 => {
                self.select_all();
                return true;
            }
            8 | 9 | 10 | 13 | input::DELETE | 0x20..=0x7e => {}
            _ => return false,
        }
        if key == 8 && self.editor.cursor == 0 && self.selection().is_none() {
            return false;
        }
        if key == input::DELETE
            && self.editor.cursor == self.editor.text.chars().count()
            && self.selection().is_none()
        {
            return false;
        }
        if self.rich.is_some()
            && !matches!(key, 8 | input::DELETE)
            && self.editor.text.len() + 1 > super::rich_text::MAX_DOCUMENT
        {
            return false;
        }
        self.checkpoint();
        let selected = self.delete_selection();
        match key {
            8 => {
                if !selected {
                    if let Some(r) = &mut self.rich {
                        r.splice(self.editor.cursor - 1, self.editor.cursor, 0);
                    }
                    self.editor.backspace();
                }
            }
            input::DELETE => {
                if !selected {
                    if let Some(r) = &mut self.rich {
                        r.splice(self.editor.cursor, self.editor.cursor + 1, 0);
                    }
                    let a = self.byte(self.editor.cursor);
                    let b = self.byte(self.editor.cursor + 1);
                    self.editor.text.replace_range(a..b, "");
                }
            }
            _ => {
                if let Some(r) = &mut self.rich {
                    r.splice(self.editor.cursor, self.editor.cursor, 1);
                }
                self.editor.insert(match key {
                    10 | 13 => '\n',
                    9 => '\t',
                    _ => key as char,
                });
            }
        }
        self.changed();
        true
    }
    pub fn insert(&mut self, text: &str) {
        if self.rich.is_some()
            && self.editor.text.len().saturating_add(text.len()) > super::rich_text::MAX_DOCUMENT
        {
            return;
        }
        self.checkpoint();
        self.delete_selection();
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        if let Some(r) = &mut self.rich {
            r.splice(self.editor.cursor, self.editor.cursor, text.chars().count());
        }
        let at = self.byte(self.editor.cursor);
        self.editor.text.insert_str(at, &text);
        self.editor.cursor += text.chars().count();
        self.changed();
    }
    fn restore(&mut self, edit: Edit) {
        self.editor.text = edit.text;
        self.editor.cursor = edit.cursor;
        self.anchor = edit.anchor;
        self.rich = edit.rich;
        self.changed();
    }
    pub fn undo(&mut self) {
        if let Some(edit) = self.undo.pop() {
            self.redo.push(self.snapshot());
            self.restore(edit);
        }
    }
    pub fn redo(&mut self) {
        if let Some(edit) = self.redo.pop() {
            self.undo.push(self.snapshot());
            self.restore(edit);
        }
    }
    pub fn mark_saved(&mut self) {
        self.saved = self.editor.text.clone();
        self.saved_styles = self
            .rich
            .as_ref()
            .map_or_else(Vec::new, |r| r.styles.clone());
        self.dirty = false;
    }
    pub fn ansi_representable(&self) -> bool {
        self.editor
            .text
            .chars()
            .all(|c| (0..=255u16).any(|b| cp1252(b as u8) as u32 == c as u32))
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn encoded(&self) -> Vec<u8> {
        self.encoded_as(self.encoding)
    }
    pub fn encoded_as(&self, encoding: Encoding) -> Vec<u8> {
        let text = if self.crlf {
            self.editor.text.replace('\n', "\r\n")
        } else {
            self.editor.text.clone()
        };
        match encoding {
            Encoding::Ansi => text
                .chars()
                .map(|c| {
                    (0..=255u16)
                        .find(|b| cp1252(*b as u8) as u32 == c as u32)
                        .map_or(b'?', |b| b as u8)
                })
                .collect(),
            Encoding::Utf8 => text.into_bytes(),
            Encoding::Utf8Bom => {
                let mut out = alloc::vec![0xef, 0xbb, 0xbf];
                out.extend_from_slice(text.as_bytes());
                out
            }
            Encoding::Utf16Le | Encoding::Utf16Be => {
                let le = encoding == Encoding::Utf16Le;
                let mut out = if le {
                    alloc::vec![0xff, 0xfe]
                } else {
                    alloc::vec![0xfe, 0xff]
                };
                for u in text.encode_utf16() {
                    out.extend_from_slice(&if le { u.to_le_bytes() } else { u.to_be_bytes() });
                }
                out
            }
        }
    }
    /// Search uses character indices, with no implicit wrap (Windows 7 behavior).
    pub fn find(&mut self, needle: &str, case_sensitive: bool) -> bool {
        self.find_direction(needle, case_sensitive, true)
    }
    pub fn find_direction(&mut self, needle: &str, case_sensitive: bool, down: bool) -> bool {
        let n = needle.chars().count();
        if n == 0 {
            return false;
        }
        let (a, b) = self
            .selection()
            .unwrap_or((self.editor.cursor, self.editor.cursor));
        let folded = needle.to_lowercase();
        let mut found = None;
        for (index, (byte, _)) in self.editor.text.char_indices().enumerate() {
            if (down && index < b) || (!down && index + n > a) {
                continue;
            }
            let candidate: String = self.editor.text[byte..].chars().take(n).collect();
            if candidate.chars().count() == n
                && if case_sensitive {
                    candidate == needle
                } else {
                    candidate.to_lowercase() == folded
                }
            {
                found = Some(index);
                if down {
                    break;
                }
            }
        }
        if let Some(index) = found {
            self.anchor = Some(index);
            self.editor.cursor = index + n;
            true
        } else {
            false
        }
    }
    pub fn replace_all(&mut self, needle: &str, replacement: &str, case_sensitive: bool) -> usize {
        let n = needle.chars().count();
        if n == 0 {
            return 0;
        }
        let chars: Vec<char> = self.editor.text.chars().collect();
        let folded = needle.to_lowercase();
        let (mut out, mut i, mut count) = (String::new(), 0, 0);
        let mut styles = Vec::new();
        let replacement = replacement.replace("\r\n", "\n").replace('\r', "\n");
        while i < chars.len() {
            let candidate: String = chars[i..(i + n).min(chars.len())].iter().collect();
            if i + n <= chars.len()
                && if case_sensitive {
                    candidate == needle
                } else {
                    candidate.to_lowercase() == folded
                }
            {
                out.push_str(&replacement);
                if let Some(r) = &self.rich {
                    styles.extend(core::iter::repeat(r.style(i)).take(replacement.chars().count()));
                }
                i += n;
                count += 1;
            } else {
                out.push(chars[i]);
                if let Some(r) = &self.rich {
                    styles.push(r.style(i));
                }
                i += 1;
            }
        }
        if self.rich.is_some() && out.len() > super::rich_text::MAX_DOCUMENT {
            return 0;
        }
        if count > 0 {
            self.checkpoint();
            if let Some(r) = &mut self.rich {
                r.styles = styles;
            }
            self.editor.text = out.replace("\r\n", "\n").replace('\r', "\n");
            self.editor.cursor = self.editor.cursor.min(self.editor.text.chars().count());
            self.anchor = None;
            self.changed();
        }
        count
    }
    pub fn goto_line(&mut self, line: usize) -> bool {
        if line == 0 || line > self.editor.text.split('\n').count() {
            return false;
        }
        self.move_to(line - 1, 0);
        true
    }
    pub fn replace_selected(&mut self, text: &str) {
        if self.selection().is_some() {
            self.insert(text);
        }
    }
    pub fn enable_rich(&mut self) {
        self.rich = Some(RichText::default());
        self.word_wrap = true;
        self.mark_saved();
    }
    pub fn current_style(&self) -> Style {
        self.rich.as_ref().map_or_else(Style::default, |r| {
            self.selection().map_or(r.typing, |(a, _)| r.style(a))
        })
    }
    pub fn format_selection(&mut self, paragraph: bool, change: impl Fn(&mut Style)) {
        if self.rich.is_none() {
            return;
        }
        let (mut a, mut b) = self
            .selection()
            .unwrap_or((self.editor.cursor, self.editor.cursor));
        if paragraph {
            let chars: Vec<_> = self.editor.text.chars().collect();
            while a > 0 && chars[a - 1] != '\n' {
                a -= 1;
            }
            // A selection ending at the next paragraph's start does not format that paragraph.
            if b > a && chars.get(b.wrapping_sub(1)) == Some(&'\n') {
                b -= 1;
            } else {
                while b < chars.len() && chars[b] != '\n' {
                    b += 1;
                }
            }
            // Include the paragraph terminator so empty/list paragraphs retain their properties.
            if chars.get(b) == Some(&'\n') {
                b += 1;
            }
        }
        self.checkpoint();
        if let Some(r) = &mut self.rich {
            change(&mut r.typing);
            for s in r.styles.iter_mut().take(b).skip(a) {
                change(s);
            }
        }
        self.changed();
    }
    pub fn rtf(&self) -> Vec<u8> {
        if let Some(r) = &self.rich {
            return r
                .encode(&self.editor.text, super::rich_text::Format::Rtf)
                .unwrap_or_default();
        }
        let mut out = String::from("{\\rtf1\\ansi\\deff0{\\fonttbl{\\f0 Segoe UI;}}\\f0\\fs22 ");
        if self.bold {
            out.push_str("\\b ");
        }
        if self.italic {
            out.push_str("\\i ");
        }
        for ch in self.editor.text.chars() {
            match ch {
                '\\' | '{' | '}' => {
                    out.push('\\');
                    out.push(ch);
                }
                '\n' => out.push_str("\\par\n"),
                '\r' => {}
                c if c.is_ascii() => out.push(c),
                c => {
                    let mut units = [0; 2];
                    for unit in c.encode_utf16(&mut units) {
                        out.push_str(&format!("\\u{}?", *unit as i16));
                    }
                }
            }
        }
        out.push('}');
        out.into_bytes()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rtf_escapes_syntax_and_utf16_and_undo_restores_text() {
        let mut d = Document::new("test.rtf");
        d.insert("A{\\}\n中😀");
        let r = String::from_utf8(d.rtf()).unwrap();
        assert!(r.contains("\\{\\\\\\}"));
        assert!(r.contains("\\u20013?"));
        assert!(r.contains("\\u-10179?\\u-8704?"));
        d.undo();
        assert_eq!(d.editor.text, "");
    }
}

/// Decode a plain text document without silently corrupting UTF-16 files.
pub fn decode_text(bytes: &[u8]) -> Result<String, &'static str> {
    if let Some(bytes) = bytes.strip_prefix(&[0xff, 0xfe]) {
        return decode_utf16(bytes, true);
    }
    if let Some(bytes) = bytes.strip_prefix(&[0xfe, 0xff]) {
        return decode_utf16(bytes, false);
    }
    let bom = bytes.starts_with(&[0xef, 0xbb, 0xbf]);
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
    if bytes.contains(&0) {
        return Err("Binary files cannot be opened as text.");
    }
    match String::from_utf8(bytes.to_vec()) {
        Ok(text) => Ok(text),
        Err(_) if !bom => Ok(bytes
            .iter()
            .filter_map(|b| char::from_u32(cp1252(*b) as u32))
            .collect()),
        Err(_) => Err("Invalid UTF-8 document."),
    }
}
fn decode_utf16(bytes: &[u8], little_endian: bool) -> Result<String, &'static str> {
    if bytes.len() % 2 != 0 {
        return Err("Truncated UTF-16 document.");
    }
    let units: Vec<_> = bytes
        .chunks_exact(2)
        .map(|p| {
            if little_endian {
                u16::from_le_bytes([p[0], p[1]])
            } else {
                u16::from_be_bytes([p[0], p[1]])
            }
        })
        .collect();
    String::from_utf16(&units).map_err(|_| "Invalid UTF-16 document.")
}
impl Document {
    pub fn load_encoding(&mut self, bytes: &[u8], encoding: Encoding) -> Result<(), &'static str> {
        let text = match encoding {
            Encoding::Ansi => bytes
                .iter()
                .filter_map(|b| char::from_u32(cp1252(*b) as u32))
                .collect(),
            Encoding::Utf8 | Encoding::Utf8Bom => {
                core::str::from_utf8(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes))
                    .map_err(|_| "Invalid UTF-8 document.")?
                    .into()
            }
            Encoding::Utf16Le => {
                decode_utf16(bytes.strip_prefix(&[0xff, 0xfe]).unwrap_or(bytes), true)?
            }
            Encoding::Utf16Be => {
                decode_utf16(bytes.strip_prefix(&[0xfe, 0xff]).unwrap_or(bytes), false)?
            }
        };
        self.install_text(text, encoding, false, false);
        Ok(())
    }
    pub fn load(&mut self, bytes: &[u8], rich_text: bool) -> Result<(), &'static str> {
        if rich_text {
            let imported = super::rich_text::decode(bytes)?;
            self.install_text(imported.text, Encoding::Utf8, false, false);
            self.bold = imported.rich.typing.bold;
            self.italic = imported.rich.typing.italic;
            self.rich = Some(imported.rich);
            self.mark_saved();
            return Ok(());
        }
        let (text, bold, italic) = (decode_text(bytes)?, false, false);
        let encoding = if bytes.starts_with(&[0xff, 0xfe]) {
            Encoding::Utf16Le
        } else if bytes.starts_with(&[0xfe, 0xff]) {
            Encoding::Utf16Be
        } else if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
            Encoding::Utf8Bom
        } else if core::str::from_utf8(bytes).is_err() {
            Encoding::Ansi
        } else {
            Encoding::Utf8
        };
        self.install_text(text, encoding, bold, italic);
        Ok(())
    }
    fn install_text(&mut self, text: String, encoding: Encoding, bold: bool, italic: bool) {
        self.revision = self.revision.wrapping_add(1);
        self.rich = None;
        self.encoding = encoding;
        self.crlf = text.contains("\r\n") || !text.contains('\n');
        self.editor.text = text.replace("\r\n", "\n").replace('\r', "\n");
        self.editor.cursor = self.editor.text.chars().count();
        self.bold = bold;
        self.italic = italic;
        self.dirty = false;
        self.undo.clear();
        self.redo.clear();
        self.anchor = None;
        self.scroll = 0;
        self.drag_anchor = None;
        self.mark_saved();
    }
}

#[cfg(test)]
mod reading_tests {
    use super::*;
    #[test]
    fn saved_rtf_reopens_as_text_with_unicode_and_formatting() {
        let mut original = Document::new("test.rtf");
        original.insert("A{\\}\n中😀");
        original.bold = true;
        original.italic = true;
        let mut loaded = Document::new("test.rtf");
        loaded.load(&original.rtf(), true).unwrap();
        assert_eq!(loaded.editor.text, original.editor.text);
        assert!(loaded.bold && loaded.italic);
        assert!(!loaded.dirty);
        let bytes = br"{\rtf1{\fonttbl{\f0 Arial;}}hello \'93quoted\'94\par world}";
        loaded.load(bytes, true).unwrap();
        assert_eq!(loaded.editor.text, "hello “quoted”\nworld");
    }
    #[test]
    fn malformed_documents_fail_without_destroying_current_text() {
        let mut d = Document::new("test.rtf");
        d.insert("keep");
        for bad in [
            br"{\rtf1 missing end".as_slice(),
            br"{\rtf1 \bin100 no}",
            br"{\rtf1 \u999999?}",
        ] {
            assert!(d.load(bad, true).is_err());
            assert_eq!(d.editor.text, "keep");
        }
        assert_eq!(decode_text(&[0xff, 0xfe, b'A', 0]), Ok("A".into()));
        assert!(decode_text(&[0xff, 0xfe, b'A']).is_err());
        assert!(decode_text(b"MZ\0binary").is_err());
    }
}

#[cfg(test)]
mod editing_tests {
    use super::*;
    #[test]
    fn encoding_and_newlines_survive_edits_and_reload() {
        for encoding in [
            Encoding::Utf8,
            Encoding::Utf8Bom,
            Encoding::Utf16Le,
            Encoding::Utf16Be,
        ] {
            let mut d = Document::new("text.txt");
            d.encoding = encoding;
            d.insert("中😀\nsecond");
            let bytes = d.encoded();
            let mut reopened = Document::new("text.txt");
            reopened.load(&bytes, false).unwrap();
            assert_eq!(reopened.encoding, encoding);
            assert!(reopened.crlf);
            assert_eq!(reopened.editor.text, "中😀\nsecond");
            assert_eq!(reopened.encoded(), bytes);
            reopened.insert("!");
            reopened.undo();
            assert!(!reopened.dirty);
            reopened.redo();
            assert!(reopened.dirty);
        }
    }
    #[test]
    fn selection_find_replace_and_unicode_undo_are_char_based() {
        let mut d = Document::new("text.txt");
        d.insert("one 中😀 TWO one");
        d.move_to(0, 0);
        assert!(d.find("中😀", true));
        assert_eq!(d.selected_text(), "中😀");
        d.replace_selected("三");
        assert_eq!(d.editor.text, "one 三 TWO one");
        d.undo();
        assert_eq!(d.editor.text, "one 中😀 TWO one");
        assert!(d.find("two", false));
        assert_eq!(d.selected_text(), "TWO");
        d.select_all();
        assert_eq!(d.cut(), "one 中😀 TWO one");
        assert_eq!(d.editor.text, "");
        d.undo();
        assert_eq!(d.editor.text, "one 中😀 TWO one");
    }
}

#[cfg(test)]
mod selection_navigation_tests {
    use super::*;
    use crate::applications::explorer::input;
    #[test]
    fn arrows_collapse_selection_and_shift_extends_it() {
        let mut d = Document::new("text.txt");
        d.insert("aé中z");
        d.select_all();
        d.key(input::LEFT);
        assert_eq!(d.editor.cursor, 0);
        assert_eq!(d.selection(), None);
        d.select_all();
        d.key(input::RIGHT);
        assert_eq!(d.editor.cursor, 4);
        assert_eq!(d.selection(), None);
        d.key(input::SHIFT_LEFT);
        d.key(input::SHIFT_LEFT);
        assert_eq!(d.selection(), Some((2, 4)));
        d.key(input::SHIFT_RIGHT);
        assert_eq!(d.selection(), Some((3, 4)));
    }
}
pub(crate) fn cp1252(byte: u8) -> u16 {
    const HIGH: [u16; 32] = [
        0x20ac, 0x81, 0x201a, 0x192, 0x201e, 0x2026, 0x2020, 0x2021, 0x2c6, 0x2030, 0x160, 0x2039,
        0x152, 0x8d, 0x17d, 0x8f, 0x90, 0x2018, 0x2019, 0x201c, 0x201d, 0x2022, 0x2013, 0x2014,
        0x2dc, 0x2122, 0x161, 0x203a, 0x153, 0x9d, 0x17e, 0x178,
    ];
    if (0x80..0xa0).contains(&byte) {
        HIGH[(byte - 0x80) as usize]
    } else {
        byte as u16
    }
}
