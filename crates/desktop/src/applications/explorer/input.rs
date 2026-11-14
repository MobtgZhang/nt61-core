//! Decode the console byte stream without confusing printable punctuation with VKs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Decoder {
    bytes: [u8; 8],
    length: usize,
    started: u64,
}
pub const UP: u8 = 0x81;
pub const DOWN: u8 = 0x82;
pub const LEFT: u8 = 0x83;
pub const RIGHT: u8 = 0x84;
pub const HOME: u8 = 0x85;
pub const END: u8 = 0x86;
pub const PAGE_UP: u8 = 0x87;
pub const PAGE_DOWN: u8 = 0x88;
pub const DELETE: u8 = 0x89;
pub const RENAME: u8 = 0x8A;
pub const REFRESH: u8 = 0x8B;
impl Decoder {
    pub fn feed(&mut self, key: u8, now: u64) -> Option<u8> {
        if self.length == 0 {
            if key == 27 {
                self.bytes[0] = key;
                self.length = 1;
                self.started = now;
                return None;
            }
            return Some(key);
        }
        if self.length == 1 && !matches!(key, b'[' | b'O') {
            self.length = 0;
            return self.feed(key, now);
        }
        if self.length >= self.bytes.len() {
            self.length = 0;
            return None;
        }
        self.bytes[self.length] = key;
        self.length += 1;
        let result = match &self.bytes[..self.length] {
            b"\x1b[A" => Some(UP),
            b"\x1b[B" => Some(DOWN),
            b"\x1b[C" => Some(RIGHT),
            b"\x1b[D" => Some(LEFT),
            b"\x1b[H" | b"\x1b[1~" => Some(HOME),
            b"\x1b[F" | b"\x1b[4~" => Some(END),
            b"\x1b[5~" => Some(PAGE_UP),
            b"\x1b[6~" => Some(PAGE_DOWN),
            b"\x1b[3~" => Some(DELETE),
            b"\x1bOQ" | b"\x1b[12~" => Some(RENAME),
            b"\x1b[15~" => Some(REFRESH),
            _ => None,
        };
        if result.is_some() || key == b'~' || (self.length > 2 && key.is_ascii_alphabetic()) {
            self.length = 0;
        }
        result
    }
    pub fn expire(&mut self, now: u64) -> Option<u8> {
        if self.length > 0 && now.saturating_sub(self.started) >= 100 {
            let escape = self.length == 1;
            self.length = 0;
            return escape.then_some(27);
        }
        None
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sequences_and_escape_are_distinct_from_printable_bytes() {
        let mut d = Decoder::default();
        for (stream, want) in [
            (b"\x1b[A".as_slice(), UP),
            (b"\x1b[6~", PAGE_DOWN),
            (b"\x1bOQ", RENAME),
        ] {
            let mut got = None;
            for key in stream {
                got = d.feed(*key, 10);
            }
            assert_eq!(got, Some(want));
        }
        assert_eq!(d.feed(b'&', 10), Some(b'&'));
        assert_eq!(d.feed(27, 20), None);
        assert_eq!(d.expire(70), None);
        assert_eq!(d.expire(120), Some(27));
    }
}
