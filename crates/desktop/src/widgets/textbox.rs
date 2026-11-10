use alloc::string::String;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextBox {
    pub id: u32,
    pub text: String,
    pub focused: bool,
    pub cursor: usize,
}

impl TextBox {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            text: String::new(),
            focused: false,
            cursor: 0,
        }
    }

    pub fn insert(&mut self, codepoint: char) {
        let byte_index = self
            .text
            .char_indices()
            .nth(self.cursor)
            .map(|(index, _)| index)
            .unwrap_or(self.text.len());
        self.text.insert(byte_index, codepoint);
        self.cursor = self.cursor.saturating_add(1);
    }

    pub fn backspace(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        let previous = self
            .text
            .char_indices()
            .nth(self.cursor - 1)
            .map(|(index, _)| index)
            .unwrap_or(0);
        let current = self
            .text
            .char_indices()
            .nth(self.cursor)
            .map(|(index, _)| index)
            .unwrap_or(self.text.len());
        self.text.replace_range(previous..current, "");
        self.cursor = self.cursor.saturating_sub(1);
        true
    }

    pub fn move_cursor_left(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        self.cursor -= 1;
        true
    }

    pub fn move_cursor_right(&mut self) -> bool {
        if self.cursor >= self.text.chars().count() {
            return false;
        }
        self.cursor += 1;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn textbox_insert_backspace_and_cursor_navigation() {
        let mut textbox = TextBox::new(1);
        textbox.insert('中');
        textbox.insert('a');
        assert_eq!(textbox.text, "中a");
        assert_eq!(textbox.cursor, 2);
        assert!(textbox.move_cursor_left());
        assert!(textbox.backspace());
        assert_eq!(textbox.text, "a");
        assert_eq!(textbox.cursor, 0);
        assert!(!textbox.backspace());
        assert!(textbox.move_cursor_right());
        assert!(!textbox.move_cursor_right());
    }
}
