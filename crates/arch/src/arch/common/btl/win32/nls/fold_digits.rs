use alloc::vec::Vec;

const BMP_FOLDS: &[(u16, u16, u8)] = &[
    (0x00b2, 0x00b3, b'2'), (0x00b9, 0x00b9, b'1'), (0x0660, 0x0669, b'0'),
    (0x06f0, 0x06f9, b'0'), (0x07c0, 0x07c9, b'0'), (0x0966, 0x096f, b'0'),
    (0x09e6, 0x09ef, b'0'), (0x0a66, 0x0a6f, b'0'), (0x0ae6, 0x0aef, b'0'),
    (0x0b66, 0x0b6f, b'0'), (0x0be6, 0x0bef, b'0'), (0x0c66, 0x0c6f, b'0'),
    (0x0ce6, 0x0cef, b'0'), (0x0d66, 0x0d6f, b'0'), (0x0e50, 0x0e59, b'0'),
    (0x0ed0, 0x0ed9, b'0'), (0x0f20, 0x0f29, b'0'), (0x1040, 0x1049, b'0'),
    (0x1090, 0x1099, b'0'), (0x1369, 0x1371, b'1'), (0x17e0, 0x17e9, b'0'),
    (0x1810, 0x1819, b'0'), (0x1946, 0x194f, b'0'), (0x19d0, 0x19d9, b'0'),
    (0x1b50, 0x1b59, b'0'), (0x1bb0, 0x1bb9, b'0'), (0x1c40, 0x1c49, b'0'),
    (0x1c50, 0x1c59, b'0'), (0x2070, 0x2070, b'0'), (0x2074, 0x2079, b'4'),
    (0x2080, 0x2089, b'0'), (0x2460, 0x2468, b'1'), (0x2474, 0x247c, b'1'),
    (0x2488, 0x2490, b'1'), (0x24ea, 0x24ea, b'0'), (0x24f5, 0x24fd, b'1'),
    (0x24ff, 0x24ff, b'0'), (0x2776, 0x277e, b'1'), (0x2780, 0x2788, b'1'),
    (0x278a, 0x2792, b'1'), (0x3007, 0x3007, b'0'), (0x3021, 0x3029, b'1'),
    (0xa620, 0xa629, b'0'), (0xa8d0, 0xa8d9, b'0'), (0xa900, 0xa909, b'0'),
    (0xaa50, 0xaa59, b'0'), (0xff10, 0xff19, b'0'),
];

fn fold_unit(unit: u16) -> u16 {
    let index = BMP_FOLDS.partition_point(|&(_, end, _)| end < unit);
    match BMP_FOLDS.get(index) {
        Some(&(start, _, digit)) if unit >= start => u16::from(digit) + (unit - start),
        _ => unit,
    }
}

fn supplementary_digit(high: u16, low: u16) -> Option<u16> {
    let digit = match (high, low) {
        (0xd800, 0xdd07..=0xdd0f) => low - 0xdd07 + 1,
        (0xd800, 0xdf20) => 1,
        (0xd800, 0xdf21) => 5,
        (0xd801, 0xdca0..=0xdca9) => low - 0xdca0,
        (0xd834, 0xdfce..=0xdfff) => (low - 0xdfce) % 10,
        _ => return None,
    };
    Some(u16::from(b'0') + digit)
}

pub(super) fn fold(bytes: &[u8]) -> Vec<u8> {
    let mut units = bytes.chunks_exact(2).map(|unit| u16::from_le_bytes([unit[0], unit[1]])).peekable();
    let mut output = Vec::with_capacity(bytes.len());
    while let Some(unit) = units.next() {
        let digit = units.peek().and_then(|&low| supplementary_digit(unit, low));
        let folded = if let Some(digit) = digit {
            units.next();
            digit
        } else {
            fold_unit(unit)
        };
        output.extend_from_slice(&folded.to_le_bytes());
    }
    output
}
