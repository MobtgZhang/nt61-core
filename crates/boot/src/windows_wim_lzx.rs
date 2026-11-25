const MAIN_SYMBOLS: usize = 256 + 30 * 8;

struct Bits<'a> {
    input: &'a [u8],
    position: usize,
    word: u16,
    left: u32,
}

impl<'a> Bits<'a> {
    fn bytes(&mut self, count: usize) -> Option<&'a [u8]> {
        let end = self.position.checked_add(count)?;
        let bytes = self.input.get(self.position..end)?;
        self.position = end;
        Some(bytes)
    }

    fn read(&mut self, count: u32) -> Option<usize> {
        let mut value = 0;
        let mut remaining = count;
        while remaining != 0 {
            if self.left == 0 {
                self.word = u16::from_le_bytes(self.bytes(2)?.try_into().ok()?);
                self.left = 16;
            }
            let take = remaining.min(self.left);
            self.left -= take;
            value = (value << take) | ((usize::from(self.word) >> self.left) & ((1 << take) - 1));
            remaining -= take;
        }
        Some(value)
    }
}

struct Huffman<const SYMBOLS: usize> {
    count: [usize; 17],
    first: [usize; 17],
    symbols: [u16; SYMBOLS],
}

impl<const SYMBOLS: usize> Huffman<SYMBOLS> {
    fn new(lengths: &[u8; SYMBOLS]) -> Option<Self> {
        let mut count = [0; 17];
        for &length in lengths.iter().filter(|&&length| length != 0) {
            *count.get_mut(usize::from(length))? += 1;
        }
        let mut first = [0; 17];
        let mut remaining = 1usize;
        for length in 1..count.len() {
            remaining = (2 * remaining).checked_sub(count[length])?;
            first[length] = first[length - 1] + count[length - 1];
        }
        let mut next = first;
        let mut symbols = [0; SYMBOLS];
        for (symbol, &length) in lengths
            .iter()
            .enumerate()
            .filter(|(_, length)| **length != 0)
        {
            let slot = &mut next[usize::from(length)];
            symbols[*slot] = symbol as u16;
            *slot += 1;
        }
        Some(Self {
            count,
            first,
            symbols,
        })
    }

    fn symbol(&self, bits: &mut Bits<'_>) -> Option<usize> {
        let mut code = 0;
        for length in 1..self.count.len() {
            code = (code << 1) | bits.read(1)?;
            if code < self.count[length] {
                return Some(usize::from(self.symbols[self.first[length] + code]));
            }
            code -= self.count[length];
        }
        None
    }
}

fn read_lengths(bits: &mut Bits<'_>, lengths: &mut [u8]) -> Option<()> {
    let mut precode_lengths = [0; 20];
    for length in &mut precode_lengths {
        *length = bits.read(4)? as u8;
    }
    let precode = Huffman::new(&precode_lengths)?;
    let mut index = 0;
    while index < lengths.len() {
        let (run, length) = match precode.symbol(bits)? {
            17 => (4 + bits.read(4)?, 0),
            18 => (20 + bits.read(5)?, 0),
            19 => {
                let run = 4 + bits.read(1)?;
                let delta = precode.symbol(bits)?;
                if delta > 16 {
                    return None;
                }
                (run, (usize::from(lengths[index]) + 17 - delta) % 17)
            }
            delta => (1, (usize::from(lengths[index]) + 17 - delta) % 17),
        };
        lengths.get_mut(index..index + run)?.fill(length as u8);
        index += run;
    }
    Some(())
}

fn undo_x86_translation(output: &mut [u8]) {
    const FILE_SIZE: i32 = 12_000_000;
    let mut index = 0;
    while index < output.len().saturating_sub(10) {
        if output[index] != 0xe8 {
            index += 1;
            continue;
        }
        let address = &mut output[index + 1..index + 5];
        let target = i32::from_le_bytes(address.try_into().unwrap());
        let position = index as i32;
        if (0..FILE_SIZE).contains(&target) {
            address.copy_from_slice(&(target - position).to_le_bytes());
        } else if (-position..0).contains(&target) {
            address.copy_from_slice(&(target + FILE_SIZE).to_le_bytes());
        }
        index += 5;
    }
}

pub fn decode_chunk(input: &[u8], output: &mut [u8]) -> Option<()> {
    if output.is_empty() || output.len() > 32768 {
        return None;
    }
    let mut bits = Bits {
        input,
        position: 0,
        word: 0,
        left: 0,
    };
    let mut recent = [1usize; 3];
    let mut main_lengths = [0; MAIN_SYMBOLS];
    let mut match_lengths = [0; 249];
    let mut position = 0;
    while position < output.len() {
        let block = bits.read(3)?;
        let size = if bits.read(1)? == 1 {
            32768
        } else {
            bits.read(16)?
        };
        let end = position + size;
        if size == 0 || end > output.len() {
            return None;
        }
        if block == 3 {
            if bits.left == 0 {
                bits.bytes(2)?;
            }
            bits.left = 0;
            for offset in &mut recent {
                *offset = u32::from_le_bytes(bits.bytes(4)?.try_into().ok()?) as usize;
                if *offset == 0 {
                    return None;
                }
            }
            output[position..end].copy_from_slice(bits.bytes(size)?);
            if size % 2 != 0 && end != output.len() {
                bits.bytes(1)?;
            }
            position = end;
            continue;
        }

        let mut aligned_lengths = [0; 8];
        match block {
            1 => (),
            2 => {
                for length in &mut aligned_lengths {
                    *length = bits.read(3)? as u8;
                }
            }
            _ => return None,
        }
        let aligned = Huffman::new(&aligned_lengths)?;
        read_lengths(&mut bits, &mut main_lengths[..256])?;
        read_lengths(&mut bits, &mut main_lengths[256..])?;
        read_lengths(&mut bits, &mut match_lengths)?;
        let main = Huffman::new(&main_lengths)?;
        let lengths = Huffman::new(&match_lengths)?;

        while position < end {
            let symbol = main.symbol(&mut bits)?;
            if symbol < 256 {
                output[position] = symbol as u8;
                position += 1;
                continue;
            }
            let header = symbol - 256;
            let slot = header / 8;
            let mut length = 2 + header % 8;
            if length == 9 {
                length += lengths.symbol(&mut bits)?;
            }
            let distance = if slot < recent.len() {
                recent.swap(0, slot);
                recent[0]
            } else {
                let extra = (slot / 2 - 1) as u32;
                let base = ((2 + slot % 2) << extra) - 2;
                let tail = if block == 2 && extra >= 3 {
                    (bits.read(extra - 3)? << 3) | aligned.symbol(&mut bits)?
                } else {
                    bits.read(extra)?
                };
                let distance = base + tail;
                recent.rotate_right(1);
                recent[0] = distance;
                distance
            };
            if distance == 0 || distance > position || length > end - position {
                return None;
            }
            for index in position..position + length {
                output[index] = output[index - distance];
            }
            position += length;
        }
    }
    undo_x86_translation(output);
    Some(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn decodes_lzx_matches_and_reverses_x86_call_translation() {
        let input = [
            0x25, 0x20, 0x00, 0x83, 0x00, 0x00, 0x00, 0x00, 0x40, 0x14, 0x20, 0x00, 0x36, 0x0f,
            0x57, 0xc8, 0x2e, 0x10, 0x89, 0x42, 0x07, 0x39, 0xdb, 0x4b, 0x6c, 0x73, 0x19, 0x66,
            0x23, 0x91, 0x85, 0x42, 0x14, 0x42, 0x10, 0x2a, 0x5c, 0xa1, 0x4a, 0x85, 0x2a, 0x04,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xc0, 0x84, 0xc0, 0xe6, 0x23, 0x73, 0xe4,
            0xbf, 0xef, 0x80, 0x19, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x98, 0x19, 0x7b, 0x87,
            0xcf, 0x25, 0xfb, 0xbe, 0xaa, 0xe4, 0xa5, 0x7a, 0xfa, 0x8d, 0x5b, 0x6b, 0xb5, 0x26,
            0x2e, 0xf6, 0xa2, 0xe5, 0x79, 0x27, 0x57, 0x7d, 0xd4, 0xf3, 0x3a, 0x70, 0x87, 0x1e,
            0x38, 0x5a, 0x41, 0x04, 0xc1, 0x42, 0xd0, 0x88, 0x39, 0x6e, 0x07, 0x1e, 0x37, 0x1b,
            0x22, 0x84, 0x40, 0x41, 0x66, 0xc0, 0x17, 0x36, 0x20, 0xa2,
        ];
        let mut output = [0; 600];
        assert_eq!(super::decode_chunk(&input, &mut output), Some(()));
        for line in output.chunks_exact(25) {
            assert_eq!(line, b"WinRE loader data\0\0\0\xe8\x7b\0\0\0");
        }
        assert_eq!(super::decode_chunk(&input[..100], &mut output), None);
        assert_eq!(super::decode_chunk(&input, &mut output[..599]), None);
    }
}
