//! Shared NT 6.1 integer conversion primitives.

pub(crate) const INVALID_PARAMETER: u32 = 0xc000000d;
pub(crate) const BUFFER_OVERFLOW: u32 = 0x80000005;

pub(crate) fn parse(
    units: impl Iterator<Item = u16>,
    mut radix: u32,
) -> Result<u32, u32> {
    if !matches!(radix, 0 | 2 | 8 | 10 | 16) {
        return Err(INVALID_PARAMETER);
    }
    let mut units = units.peekable();
    let negative = units.peek() == Some(&0x2d);
    if matches!(units.peek(), Some(0x2b | 0x2d)) {
        units.next();
    }
    if radix == 0 {
        radix = 10;
        if units.peek() == Some(&0x30) {
            units.next();
            radix = match units.peek() {
                Some(0x62) => 2,
                Some(0x6f) => 8,
                Some(0x78) => 16,
                _ => 10,
            };
            if radix != 10 {
                units.next();
            }
        }
    }
    let mut result = 0u32;
    for unit in units {
        let digit = match unit {
            0x30..=0x39 => unit - 0x30,
            0x41..=0x46 => unit - 0x41 + 10,
            0x61..=0x66 => unit - 0x61 + 10,
            _ => break,
        };
        if u32::from(digit) >= radix {
            break;
        }
        result = result.wrapping_mul(radix).wrapping_add(u32::from(digit));
    }
    Ok(if negative { result.wrapping_neg() } else { result })
}

pub(crate) fn parse_ansi(units: impl Iterator<Item = u8>, radix: u32) -> Result<u32, u32> {
    parse(units.take_while(|&unit| unit != 0)
        .skip_while(|&unit| (unit as i8) <= 0x20).map(u16::from), radix)
}

pub(crate) fn digits(
    mut value: u32,
    radix: u32,
    storage: &mut [u8; 32],
) -> Result<&[u8], u32> {
    let radix = match radix {
        0 => 10,
        2 | 8 | 10 | 16 => radix,
        _ => return Err(INVALID_PARAMETER),
    };
    let mut start = storage.len();
    loop {
        start -= 1;
        storage[start] = b"0123456789ABCDEF"[(value % radix) as usize];
        value /= radix;
        if value == 0 {
            return Ok(&storage[start..]);
        }
    }
}

pub(crate) fn field(capacity: i32, length: usize) -> Result<(usize, bool), u32> {
    let width = capacity.wrapping_abs();
    if width < length as i32 {
        return Err(BUFFER_OVERFLOW);
    }
    Ok((if capacity < 0 { width as usize - length } else { 0 }, capacity > length as i32))
}
