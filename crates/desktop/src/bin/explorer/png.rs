extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;

#[derive(Debug)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Image {
    pub fn stride(&self) -> usize {
        self.width as usize * 4
    }
}

#[derive(Default)]
struct BitReader<'a> {
    data: &'a [u8],
    position: usize,
    bit: u32,
}

impl<'a> BitReader<'a> {
    fn read_bit(&mut self) -> Result<u32, DecodeError> {
        if self.position >= self.data.len() {
            return Err(DecodeError::Truncated);
        }
        let value = (self.data[self.position] >> self.bit) & 1;
        self.bit += 1;
        if self.bit == 8 {
            self.bit = 0;
            self.position += 1;
        }
        Ok(value as u32)
    }

    fn read_bits(&mut self, count: u32) -> Result<u32, DecodeError> {
        let mut value = 0u32;
        for index in 0..count {
            value |= self.read_bit()? << index;
        }
        Ok(value)
    }

    fn align(&mut self) {
        if self.bit != 0 {
            self.bit = 0;
            self.position += 1;
        }
    }

    fn read_byte(&mut self) -> Result<u8, DecodeError> {
        self.align();
        if self.position >= self.data.len() {
            return Err(DecodeError::Truncated);
        }
        let byte = self.data[self.position];
        self.position += 1;
        Ok(byte)
    }

    fn read_u16(&mut self) -> Result<u16, DecodeError> {
        let low = self.read_byte()? as u16;
        let high = self.read_byte()? as u16;
        Ok(low | (high << 8))
    }
}

#[derive(Debug)]
pub enum DecodeError {
    Truncated,
    Unsupported,
}

struct Huffman {
    counts: [u16; 16],
    symbols: Vec<u16>,
}

impl Huffman {
    fn new(lengths: &[u8]) -> Huffman {
        let mut counts = [0u16; 16];
        for &length in lengths {
            counts[length as usize] += 1;
        }
        counts[0] = 0;
        let mut symbols = vec![0u16; lengths.len()];
        let mut slots = [0usize; 16];
        let mut running = 0usize;
        for length in 1..16 {
            slots[length] = running;
            running += counts[length] as usize;
        }
        for (symbol, &length) in lengths.iter().enumerate() {
            if length != 0 {
                symbols[slots[length as usize]] = symbol as u16;
                slots[length as usize] += 1;
            }
        }
        Huffman { counts, symbols }
    }

    fn decode(&self, reader: &mut BitReader<'_>) -> Result<u16, DecodeError> {
        let mut code = 0u16;
        let mut first = 0u16;
        let mut index = 0usize;
        for length in 1..16 {
            code |= reader.read_bit()? as u16;
            let count = self.counts[length];
            if count != 0 && code >= first && code < first + count {
                let slot = index + (code - first) as usize;
                return self
                    .symbols
                    .get(slot)
                    .copied()
                    .ok_or(DecodeError::Unsupported);
            }
            index += count as usize;
            first = (first + count) << 1;
            code <<= 1;
        }
        Err(DecodeError::Unsupported)
    }
}

fn fixed_huffman() -> (Huffman, Huffman) {
    let mut lengths = vec![0u8; 288];
    for (index, length) in lengths.iter_mut().enumerate() {
        *length = match index {
            0..=143 => 8,
            144..=255 => 9,
            256..=279 => 7,
            _ => 8,
        };
    }
    let literal = Huffman::new(&lengths);
    let distance = Huffman::new(&[5u8; 30]);
    (literal, distance)
}

fn dynamic_huffman(reader: &mut BitReader<'_>) -> Result<(Huffman, Huffman), DecodeError> {
    const ORDER: [usize; 19] = [
        16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
    ];
    let hlit = reader.read_bits(5)? as usize + 257;
    let hdist = reader.read_bits(5)? as usize + 1;
    let hclen = reader.read_bits(4)? as usize + 4;
    let mut code_lengths = [0u8; 19];
    for &index in ORDER.iter().take(hclen) {
        code_lengths[index] = reader.read_bits(3)? as u8;
    }
    let code_huffman = Huffman::new(&code_lengths);
    let mut literal_lengths = vec![0u8; hlit];
    let mut distance_lengths = vec![0u8; hdist];
    let mut all_lengths = Vec::with_capacity(hlit + hdist);
    while all_lengths.len() < hlit + hdist {
        match code_huffman.decode(reader)? {
            length_symbol @ 0..=15 => all_lengths.push(length_symbol as u8),
            16 => {
                let previous = *all_lengths.last().ok_or(DecodeError::Truncated)?;
                let repeat = 3 + reader.read_bits(2)? as usize;
                for _ in 0..repeat {
                    all_lengths.push(previous);
                }
            }
            17 => {
                for _ in 0..3 + reader.read_bits(3)? {
                    all_lengths.push(0);
                }
            }
            18 => {
                for _ in 0..11 + reader.read_bits(7)? {
                    all_lengths.push(0);
                }
            }
            _ => return Err(DecodeError::Unsupported),
        }
    }
    literal_lengths.copy_from_slice(&all_lengths[..hlit]);
    distance_lengths.copy_from_slice(&all_lengths[hlit..]);
    Ok((
        Huffman::new(&literal_lengths),
        Huffman::new(&distance_lengths),
    ))
}

fn inflate(data: &[u8], capacity: usize) -> Result<Vec<u8>, DecodeError> {
    let mut reader = BitReader {
        data,
        position: 0,
        bit: 0,
    };
    let mut output = Vec::with_capacity(capacity.min(1 << 16));
    loop {
        let final_block = reader.read_bit()? != 0;
        let block_type = reader.read_bits(2)?;
        match block_type {
            0 => {
                reader.align();
                let length = reader.read_u16()? as usize;
                let complement = reader.read_u16()? as usize;
                if length != !complement & 0xffff {
                    return Err(DecodeError::Unsupported);
                }
                for _ in 0..length {
                    output.push(reader.read_byte()?);
                }
            }
            1 | 2 => {
                let (literal, distance) = if block_type == 1 {
                    fixed_huffman()
                } else {
                    dynamic_huffman(&mut reader)?
                };
                loop {
                    let symbol = literal.decode(&mut reader)?;
                    match symbol {
                        0..=255 => output.push(symbol as u8),
                        256 => break,
                        257..=285 => {
                            const LENGTH_BASE: [u16; 29] = [
                                3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43,
                                51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258,
                            ];
                            const LENGTH_EXTRA: [u32; 29] = [
                                0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4,
                                4, 4, 5, 5, 5, 5, 0,
                            ];
                            let index = (symbol - 257) as usize;
                            let base = LENGTH_BASE[index] as u32;
                            let extra = LENGTH_EXTRA[index];
                            let length = base + reader.read_bits(extra)?;
                            let distance_symbol = distance.decode(&mut reader)?;
                            const DISTANCE_BASE: [u16; 30] = [
                                1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257,
                                385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289,
                                16385, 24577,
                            ];
                            const DISTANCE_EXTRA: [u32; 30] = [
                                0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9,
                                10, 10, 11, 11, 12, 12, 13, 13,
                            ];
                            let distance_index = distance_symbol as usize;
                            if distance_index >= 30 {
                                return Err(DecodeError::Unsupported);
                            }
                            let distance_base = DISTANCE_BASE[distance_index] as u32;
                            let distance_extra = DISTANCE_EXTRA[distance_index];
                            let offset =
                                (distance_base + reader.read_bits(distance_extra)?) as usize;
                            if offset > output.len() {
                                return Err(DecodeError::Truncated);
                            }
                            let start = output.len() - offset;
                            for index in 0..length as usize {
                                output.push(output[start + index]);
                            }
                        }
                        _ => return Err(DecodeError::Unsupported),
                    }
                }
            }
            _ => return Err(DecodeError::Unsupported),
        }
        if final_block {
            break;
        }
    }
    Ok(output)
}

fn chunk<'a>(data: &'a [u8], position: &mut usize) -> Result<(&'a [u8], &'a [u8]), DecodeError> {
    if *position + 8 > data.len() {
        return Err(DecodeError::Truncated);
    }
    let length = u32::from_be_bytes(data[*position..*position + 4].try_into().unwrap()) as usize;
    let start = *position + 8;
    let end = start.checked_add(length).ok_or(DecodeError::Truncated)?;
    if end > data.len() {
        return Err(DecodeError::Truncated);
    }
    let kind = &data[*position + 4..*position + 8];
    let payload = &data[start..end];
    *position = end + 4;
    Ok((kind, payload))
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let a = a as i16;
    let b = b as i16;
    let c = c as i16;
    let p = a + b - c;
    let pa = (p - a).abs();
    let pb = (p - b).abs();
    let pc = (p - c).abs();
    let value = if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    };
    value as u8
}

pub fn decode_png(data: &[u8]) -> Result<Image, DecodeError> {
    if data.len() < 8 || &data[..8] != b"\x89PNG\r\n\x1a\n" {
        return Err(DecodeError::Unsupported);
    }
    let mut position = 8;
    let mut width = 0u32;
    let mut height = 0u32;
    let mut depth = 0u8;
    let mut colortype = 0u8;
    let mut palette: Vec<u8> = Vec::new();
    let mut trns: Vec<u8> = Vec::new();
    let mut compressed = Vec::new();
    while position < data.len() {
        let (kind, payload) = chunk(data, &mut position)?;
        match kind {
            b"IHDR" if payload.len() >= 13 => {
                width = u32::from_be_bytes(payload[0..4].try_into().unwrap());
                height = u32::from_be_bytes(payload[4..8].try_into().unwrap());
                depth = payload[8];
                colortype = payload[9];
                let ok = match (depth, colortype) {
                    (8, 2 | 6) => true,
                    (2 | 4 | 8, 3) => true,
                    _ => false,
                };
                if !ok || payload[12] != 0 {
                    return Err(DecodeError::Unsupported);
                }
            }
            b"PLTE" => palette = payload.to_vec(),
            b"tRNS" => trns = payload.to_vec(),
            b"IDAT" => compressed.extend_from_slice(payload),
            b"IEND" => break,
            _ => {}
        }
    }
    if width == 0 || height == 0 {
        return Err(DecodeError::Unsupported);
    }
    if compressed.len() < 6 {
        return Err(DecodeError::Truncated);
    }
    let width = width as usize;
    let height = height as usize;
    let (row_bytes, bpp) = match colortype {
        2 => (width * 3, 3usize),
        6 => (width * 4, 4usize),
        3 => ((width * depth as usize + 7) / 8, 1usize),
        _ => return Err(DecodeError::Unsupported),
    };
    let capacity = (row_bytes + 1) * height;
    let raw = inflate(&compressed[2..compressed.len() - 4], capacity)?;

    let mut pixels = vec![0u8; width * 4 * height];
    let mut previous = vec![0u8; row_bytes];
    let mut current = vec![0u8; row_bytes];
    for row in 0..height {
        let filter = raw[row * (row_bytes + 1)];
        current.copy_from_slice(&raw[row * (row_bytes + 1) + 1..(row + 1) * (row_bytes + 1)]);
        for column in 0..row_bytes {
            let left = if column >= bpp {
                current[column - bpp]
            } else {
                0
            };
            let above = previous[column];
            let above_left = if column >= bpp {
                previous[column - bpp]
            } else {
                0
            };
            let predictor = match filter {
                0 => 0,
                1 => left,
                2 => above,
                3 => ((left as u16 + above as u16) / 2) as u8,
                4 => paeth(left, above, above_left),
                _ => return Err(DecodeError::Unsupported),
            };
            current[column] = current[column].wrapping_add(predictor);
        }
        let out = &mut pixels[row * width * 4..(row + 1) * width * 4];
        match colortype {
            2 => {
                for (x, chunk) in current.chunks_exact(3).enumerate() {
                    out[x * 4..x * 4 + 4].copy_from_slice(&[chunk[0], chunk[1], chunk[2], 0xFF]);
                }
            }
            6 => out.copy_from_slice(&current),
            _ => {
                for x in 0..width {
                    let index = match depth {
                        2 => (current[x / 4] >> (6 - 2 * (x % 4))) & 3,
                        4 => (current[x / 2] >> (4 - 4 * (x % 2))) & 15,
                        _ => current[x],
                    } as usize;
                    let base = index * 3;
                    if base + 2 >= palette.len() + 1 && base + 2 > palette.len().saturating_sub(1) {
                        // fall through only when palette entry missing
                    }
                    if base + 2 < palette.len() {
                        out[x * 4] = palette[base];
                        out[x * 4 + 1] = palette[base + 1];
                        out[x * 4 + 2] = palette[base + 2];
                    } else {
                        out[x * 4..x * 4 + 3].fill(0xFF);
                    }
                    out[x * 4 + 3] = trns.get(index).copied().unwrap_or(0xFF);
                }
            }
        }
        previous.copy_from_slice(&current);
    }
    Ok(Image {
        width: width as u32,
        height: height as u32,
        pixels,
    })
}
