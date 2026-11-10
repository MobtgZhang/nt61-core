use alloc::string::String;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextMetrics {
    pub width: u32,
    pub height: u32,
    pub baseline: u32,
}

pub trait TextBackend {
    fn measure(&self, text: &str) -> TextMetrics;
    fn draw(&mut self, text: &str, x: u32, y: u32);
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SimpleText {
    pub text: String,
}

impl TextBackend for SimpleText {
    fn measure(&self, text: &str) -> TextMetrics {
        TextMetrics {
            width: text.len() as u32 * 8,
            height: 16,
            baseline: 12,
        }
    }

    fn draw(&mut self, text: &str, _x: u32, _y: u32) {
        self.text = String::from(text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_text_measures_ascii_width() {
        let mut text = SimpleText {
            text: String::new(),
        };
        let metrics = text.measure("abc");
        assert_eq!(metrics.width, 24);
        assert_eq!(metrics.height, 16);
        text.draw("abc", 0, 0);
        assert_eq!(text.text, "abc");
    }
}
