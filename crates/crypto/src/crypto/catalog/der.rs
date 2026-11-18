//! Bounded DER reader for the offline SP1 catalog profile (X.690).
use super::{Error, Result};

#[derive(Clone, Copy, Debug)]
pub(super) struct Value<'a> {
    pub tag: u8,
    pub full: &'a [u8],
    pub body: &'a [u8],
}

#[derive(Clone, Copy)]
pub(super) struct Reader<'a> {
    bytes: &'a [u8],
}
impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes }
    }
    pub fn empty(&self) -> bool {
        self.bytes.is_empty()
    }
    pub fn peek(&self) -> Option<u8> {
        self.bytes.first().copied()
    }
    pub fn finish(self) -> Result<()> {
        if self.empty() {
            Ok(())
        } else {
            Err(Error::Der)
        }
    }
    pub fn read(&mut self) -> Result<Value<'a>> {
        let bytes = self.bytes;
        let tag = *bytes.first().ok_or(Error::Der)?;
        if tag & 31 == 31 || tag == 0 {
            return Err(Error::Der);
        }
        let first = *bytes.get(1).ok_or(Error::Der)?;
        let (header, length) = if first < 128 {
            (2, first as usize)
        } else {
            let n = (first & 127) as usize;
            if n == 0 || n > 4 || bytes.get(2) == Some(&0) {
                return Err(Error::Der);
            }
            let mut len = 0usize;
            for &b in bytes.get(2..2 + n).ok_or(Error::Der)? {
                len = len
                    .checked_mul(256)
                    .and_then(|v| v.checked_add(b as usize))
                    .ok_or(Error::Der)?;
            }
            if len < 128 {
                return Err(Error::Der);
            }
            (2 + n, len)
        };
        let end = header.checked_add(length).ok_or(Error::Der)?;
        let full = bytes.get(..end).ok_or(Error::Der)?;
        self.bytes = &bytes[end..];
        Ok(Value {
            tag,
            full,
            body: &full[header..],
        })
    }
    pub fn expect(&mut self, tag: u8) -> Result<Value<'a>> {
        let value = self.read()?;
        if value.tag == tag {
            Ok(value)
        } else {
            Err(Error::Der)
        }
    }
}
pub(super) fn one(bytes: &[u8], tag: u8) -> Result<Value<'_>> {
    let mut r = Reader::new(bytes);
    let v = r.expect(tag)?;
    r.finish()?;
    Ok(v)
}
pub(super) fn positive(v: Value<'_>) -> Result<&[u8]> {
    if v.tag != 2
        || v.body.is_empty()
        || v.body[0] & 128 != 0
        || (v.body.len() > 1 && v.body[0] == 0 && v.body[1] & 128 == 0)
    {
        return Err(Error::Der);
    }
    Ok(if v.body.len() > 1 && v.body[0] == 0 {
        &v.body[1..]
    } else {
        v.body
    })
}
pub(super) fn integer(v: Value<'_>) -> Result<u32> {
    let mut n = 0u32;
    for &b in positive(v)? {
        n = n
            .checked_mul(256)
            .and_then(|n| n.checked_add(b as u32))
            .ok_or(Error::Der)?;
    }
    Ok(n)
}
pub(super) fn algorithm(v: Value<'_>) -> Result<&[u8]> {
    if v.tag != 0x30 {
        return Err(Error::Der);
    }
    let mut r = Reader::new(v.body);
    let oid = r.expect(6)?.body;
    if !r.empty() {
        r.expect(5)?
            .body
            .is_empty()
            .then_some(())
            .ok_or(Error::Der)?;
    }
    r.finish()?;
    Ok(oid)
}
pub(super) fn bit_string(v: Value<'_>) -> Result<&[u8]> {
    if v.tag != 3 || v.body.first() != Some(&0) {
        return Err(Error::Der);
    }
    Ok(&v.body[1..])
}

/// Parse the exact RFC 5280 UTC/GeneralizedTime forms, returning sortable
/// YYYYMMDDhhmmss, independent of timezone and kernel clock services.
pub(super) fn time(v: Value<'_>) -> Result<u64> {
    let (year, tail) = match (v.tag, v.body.len()) {
        (0x17, 13) => {
            let year = digits(&v.body[..2])?;
            (
                if year >= 50 { 1900 + year } else { 2000 + year },
                &v.body[2..],
            )
        }
        (0x18, 15) => (digits(&v.body[..4])?, &v.body[4..]),
        _ => return Err(Error::Der),
    };
    if tail[10] != b'Z' {
        return Err(Error::Der);
    }
    let m = digits(&tail[..2])?;
    let d = digits(&tail[2..4])?;
    let h = digits(&tail[4..6])?;
    let min = digits(&tail[6..8])?;
    let sec = digits(&tail[8..10])?;
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if leap {
                29
            } else {
                28
            }
        }
        _ => 0,
    };
    if year == 0 || d == 0 || d > days || h > 23 || min > 59 || sec > 59 {
        return Err(Error::Der);
    }
    Ok(year * 10_000_000_000 + m * 100_000_000 + d * 1_000_000 + h * 10_000 + min * 100 + sec)
}
fn digits(bytes: &[u8]) -> Result<u64> {
    bytes.iter().try_fold(0, |n, &b| {
        if b.is_ascii_digit() {
            Ok(n * 10 + (b - b'0') as u64)
        } else {
            Err(Error::Der)
        }
    })
}
