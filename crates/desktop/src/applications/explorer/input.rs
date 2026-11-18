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
pub const FIND_NEXT:u8=0x8C;
pub const FULLSCREEN:u8=0x8F;
pub const REPLACE:u8=0x8E;
pub const HELP:u8=0x8D;
pub const SHIFT_UP:u8=0x90;
pub const SHIFT_DOWN:u8=0x91;
pub const SHIFT_LEFT:u8=0x92;
pub const SHIFT_RIGHT:u8=0x93;
pub const SHIFT_HOME:u8=0x94;
pub const SHIFT_END:u8=0x95;
pub const CTRL_HOME:u8=0x96;
pub const CTRL_END:u8=0x97;
pub const BACK:u8=0x98;
pub const FORWARD:u8=0x99;
pub const ALT_FILE:u8=0xA0;
pub const ALT_EDIT:u8=0xA1;
pub const ALT_FORMAT:u8=0xA2;
pub const ALT_VIEW:u8=0xA3;
pub const ALT_HELP:u8=0xA4;
pub const ALT_WINDOWS:u8=0xA5;
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
            return match key.to_ascii_lowercase(){b'f'=>Some(ALT_FILE),b'e'=>Some(ALT_EDIT),b'o'=>Some(ALT_FORMAT),b'v'=>Some(ALT_VIEW),b'h'=>Some(ALT_HELP),b'w'=>Some(ALT_WINDOWS),_=>self.feed(key,now)};
        }
        if self.length >= self.bytes.len() {
            self.length = 0;
            return None;
        }
        self.bytes[self.length] = key;
        self.length += 1;
        let result = match &self.bytes[..self.length] {
            b"\x1bOP"|b"\x1b[11~"=>Some(HELP),
            b"\x1bOR"|b"\x1b[13~"=>Some(FIND_NEXT),
            b"\x1b[1;2A"=>Some(SHIFT_UP),b"\x1b[1;2B"=>Some(SHIFT_DOWN),
            b"\x1b[1;2C"=>Some(SHIFT_RIGHT),b"\x1b[1;2D"=>Some(SHIFT_LEFT),
            b"\x1b[1;2H"=>Some(SHIFT_HOME),b"\x1b[1;2F"=>Some(SHIFT_END),
            b"\x1b[1;5H"=>Some(CTRL_HOME),b"\x1b[1;5F"=>Some(CTRL_END),
            b"\x1b[1;3D" => Some(BACK),
            b"\x1b[1;3C" => Some(FORWARD),
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
            b"\x1b[23~" => Some(FULLSCREEN),
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
            (b"\x1b[1;3D", BACK),
            (b"\x1b[1;3C", FORWARD),
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
