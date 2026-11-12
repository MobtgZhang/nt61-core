extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;

#[derive(Debug)]
pub enum DecodeError {
    Truncated,
    Unsupported,
}

#[derive(Debug)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Image {}

#[derive(Clone, Default)]
struct HuffmanTable {
    counts: [u16; 17],
    values: Vec<u8>,
}

impl HuffmanTable {
    fn decode(&self, reader: &mut BitReader<'_>) -> Result<u8, DecodeError> {
        let mut code = 0u32;
        let mut first = 0u32;
        let mut index = 0usize;
        for length in 1..=16 {
            code = code.wrapping_mul(2).wrapping_add(reader.read_bit()? as u32);
            let count = self.counts[length] as u32;
            if count != 0 && code >= first && code < first + count {
                return self
                    .values
                    .get(index + (code - first) as usize)
                    .copied()
                    .ok_or(DecodeError::Truncated);
            }
            index += count as usize;
            first = (first + count) << 1;
        }
        Err(DecodeError::Unsupported)
    }
}

struct BitReader<'a> {
    bytes: &'a [u8],
    position: usize,
    bit: u32,
}

impl<'a> BitReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            position: 0,
            bit: 0,
        }
    }

    fn read_bit(&mut self) -> Result<u8, DecodeError> {
        if self.position >= self.bytes.len() {
            return Err(DecodeError::Truncated);
        }
        let byte = self.bytes[self.position];
        let value = (byte >> (7 - self.bit)) & 1;
        self.bit += 1;
        if self.bit == 8 {
            self.bit = 0;
            self.position += 1;
            if self.position < self.bytes.len()
                && self.position > 0
                && self.bytes[self.position] == 0x00
                && self.bytes[self.position - 1] == 0xFF
            {
                self.position += 1;
            }
        }
        Ok(value)
    }

    fn align(&mut self) {
        if self.bit != 0 {
            self.bit = 0;
            self.position += 1;
        }
    }

    fn receive(&mut self, count: u32) -> Result<u32, DecodeError> {
        let mut value = 0u32;
        for _ in 0..count {
            value = value.wrapping_mul(2).wrapping_add(self.read_bit()? as u32);
        }
        Ok(value)
    }

    fn receive_extend(&mut self, count: u32) -> Result<i32, DecodeError> {
        if count == 0 {
            return Ok(0);
        }
        let value = self.receive(count)? as i32;
        if value < (1 << (count - 1)) {
            Ok(value - (1 << count) + 1)
        } else {
            Ok(value)
        }
    }

    fn restart(&mut self) -> Result<(), DecodeError> {
        self.align();
        if self.position + 2 <= self.bytes.len()
            && self.bytes[self.position] == 0xFF
            && (0xD0..=0xD7).contains(&self.bytes[self.position + 1])
        {
            self.position += 2;
            Ok(())
        } else {
            Err(DecodeError::Truncated)
        }
    }
}

#[derive(Clone, Copy, Default)]
struct Component {
    horizontal: u8,
    vertical: u8,
    quantization: usize,
    dc_table: usize,
    ac_table: usize,
    previous_dc: i32,
}

struct Scan {
    components: [Component; 3],
    mcus_x: usize,
    mcus_y: usize,
    restart_interval: u16,
    dc_tables: [HuffmanTable; 2],
    ac_tables: [HuffmanTable; 2],
    quantization: [[u16; 64]; 2],
}

fn be16(bytes: &[u8], offset: usize) -> Result<u16, DecodeError> {
    bytes
        .get(offset..offset + 2)
        .map(|bytes| u16::from_be_bytes([bytes[0], bytes[1]]))
        .ok_or(DecodeError::Truncated)
}

fn read_huffman(bytes: &[u8], offset: usize) -> Result<(HuffmanTable, usize), DecodeError> {
    let lengths_slice = bytes
        .get(offset..offset + 16)
        .ok_or(DecodeError::Truncated)?;
    let mut table = HuffmanTable::default();
    for (index, &length) in lengths_slice.iter().enumerate() {
        table.counts[index + 1] = length as u16;
    }
    let total: usize = table.counts.iter().map(|&count| count as usize).sum();
    let values = bytes
        .get(offset + 16..offset + 16 + total)
        .ok_or(DecodeError::Truncated)?;
    table.values = values.to_vec();
    Ok((table, 16 + total))
}

fn jpeg_error(message: &str) -> DecodeError {
    let _ = message;
    DecodeError::Unsupported
}

fn idct(block: &[i32; 64], output: &mut [u8; 64]) {
    const COS: [[f32; 8]; 8] = [
        [0.125, 0.125, 0.125, 0.125, 0.125, 0.125, 0.125, 0.125],
        [
            0.16073, 0.136054, 0.09576, 0.047325, -0.000742, -0.049281, -0.095064, -0.134825,
        ],
        [
            0.150883, 0.0625, -0.053225, -0.149866, -0.149866, -0.053225, 0.0625, 0.150883,
        ],
        [
            0.136054, -0.047325, -0.134825, 0.095064, 0.16073, -0.09576, -0.136054, 0.049281,
        ],
        [0.125, -0.125, -0.125, 0.125, 0.125, -0.125, -0.125, 0.125],
        [
            0.09576, -0.16073, 0.136054, 0.049281, -0.136054, 0.047325, 0.150883, -0.149866,
        ],
        [
            0.0625, -0.149866, 0.150883, -0.053225, -0.0625, 0.149866, -0.150883, 0.053225,
        ],
        [
            0.047325, -0.09576, 0.049281, 0.136054, -0.150883, 0.149866, -0.136054, 0.095064,
        ],
    ];

    for row in 0..8 {
        for column in 0..8 {
            let mut value = 0.0;
            for u in 0..8 {
                for v in 0..8 {
                    value += COS[u][row] * COS[v][column] * block[v * 8 + u] as f32;
                }
            }
            output[row * 8 + column] = (value + 128.0).clamp(0.0, 255.0) as u8;
        }
    }
}

fn decode_scan(bytes: &[u8], scan: &mut Scan) -> Result<Vec<Vec<i32>>, DecodeError> {
    let mut coefficients = scan
        .components
        .iter()
        .map(|component| {
            let width = scan.mcus_x * component.horizontal as usize * 8;
            let height = scan.mcus_y * component.vertical as usize * 8;
            vec![0i32; (width / 8) * (height / 8) * 64]
        })
        .collect::<Vec<_>>();
    let mut reader = BitReader::new(bytes);
    for mcu_y in 0..scan.mcus_y {
        for mcu_x in 0..scan.mcus_x {
            for component_index in 0..3 {
                let component = scan.components[component_index];
                let block_x_count = component.horizontal as usize;
                let block_y_count = component.vertical as usize;
                for block_y in 0..block_y_count {
                    for block_x in 0..block_x_count {
                        let mut block = [0i32; 64];
                        let dc_table = &scan.dc_tables[component.dc_table.min(1)];
                        let zero_run = dc_table.decode(&mut reader)?;
                        if zero_run > 11 {
                            return Err(DecodeError::Unsupported);
                        }
                        let difference = reader.receive_extend(zero_run as u32 & 0x0F)? as i32;
                        scan.components[component_index].previous_dc += difference;
                        block[0] = component.previous_dc
                            * scan.quantization[component.quantization.min(1)][0] as i32;
                        let ac_table = &scan.ac_tables[component.ac_table.min(1)];
                        let mut coefficient_index = 1;
                        while coefficient_index < 64 {
                            let symbol = ac_table.decode(&mut reader)?;
                            if symbol == 0 {
                                break;
                            }
                            let run = (symbol >> 4) as usize;
                            let length = (symbol & 0x0F) as u32;
                            coefficient_index += run;
                            if coefficient_index > 63 || length > 15 {
                                return Err(DecodeError::Unsupported);
                            }
                            let value = reader.receive_extend(length)?;
                            block[coefficient_index] = value
                                * scan.quantization[component.quantization.min(1)]
                                    [coefficient_index] as i32;
                            coefficient_index += 1;
                        }
                        let mut pixels = [0u8; 64];
                        idct(&block, &mut pixels);
                        let width = scan.mcus_x * block_x_count * 8;
                        for pixel_y in 0..8 {
                            for pixel_x in 0..8 {
                                let source = pixel_y * 8 + pixel_x;
                                let destination_y = (mcu_y * block_y_count + block_y) * 8 + pixel_y;
                                let destination_x = (mcu_x * block_x_count + block_x) * 8 + pixel_x;
                                coefficients[component_index]
                                    [destination_y * width + destination_x] = pixels[source] as i32;
                            }
                        }
                    }
                }
            }
            if scan.restart_interval != 0
                && (mcu_y * scan.mcus_x + mcu_x + 1) % scan.restart_interval as usize == 0
            {
                for component in &mut scan.components {
                    component.previous_dc = 0;
                }
                reader.restart()?;
            }
        }
    }
    Ok(coefficients)
}

fn sample(pixels: &[i32], width: usize, height: usize, x: u32, y: u32) -> u8 {
    let x = (x as usize).min(width - 1);
    let y = (y as usize).min(height - 1);
    pixels[y * width + x].clamp(0, 255) as u8
}

fn ycbcr_to_rgba(y: u8, cb: u8, cr: u8) -> [u8; 4] {
    let y = y as f32;
    let cb = cb as f32 - 128.0;
    let cr = cr as f32 - 128.0;
    let red = y + 1.40200 * cr;
    let green = y - 0.34414 * cb - 0.71414 * cr;
    let blue = y + 1.77200 * cb;
    [
        red.clamp(0.0, 255.0) as u8,
        green.clamp(0.0, 255.0) as u8,
        blue.clamp(0.0, 255.0) as u8,
        255,
    ]
}

pub fn decode_jpeg(data: &[u8]) -> Result<Image, DecodeError> {
    if data.get(0..2) != Some(&[0xFF, 0xD8]) {
        return Err(DecodeError::Unsupported);
    }

    let mut offset = 2usize;
    let mut quantization = [[0u16; 64]; 2];
    let mut dc_tables = [HuffmanTable::default(), HuffmanTable::default()];
    let mut ac_tables = [HuffmanTable::default(), HuffmanTable::default()];
    let mut components = [Component::default(); 3];
    let mut maximum_horizontal = 0u8;
    let mut maximum_vertical = 0u8;
    let mut mcus_x = 0usize;
    let mut mcus_y = 0usize;
    let mut width = 0u32;
    let mut height = 0u32;
    let mut restart_interval = 0u16;

    while offset < data.len() {
        if data.get(offset) != Some(&0xFF) {
            return Err(DecodeError::Truncated);
        }
        let marker = *data.get(offset + 1).ok_or(DecodeError::Truncated)?;
        offset += 2;
        if marker == 0xD9 {
            return Err(DecodeError::Truncated);
        }
        if (0xD0..=0xD7).contains(&marker) {
            continue;
        }
        if marker == 0xD8 || marker == 0x01 {
            continue;
        }
        let segment_length = be16(data, offset)? as usize;
        let segment_end = offset + segment_length;
        if segment_end > data.len() {
            return Err(DecodeError::Truncated);
        }

        match marker {
            0xC0 => {
                height = be16(data, offset + 3)? as u32;
                width = be16(data, offset + 5)? as u32;
                let component_count = *data.get(offset + 7).ok_or(DecodeError::Truncated)? as usize;
                if component_count != 3 {
                    return Err(jpeg_error("only YCbCr JPEG is supported"));
                }
                for index in 0..component_count {
                    let component_offset = offset + 8 + index * 3;
                    let component_id = *data.get(component_offset).ok_or(DecodeError::Truncated)?;
                    let sampling = data.get(component_offset + 1).copied().unwrap_or(0);
                    let quantization =
                        data.get(component_offset + 2).copied().unwrap_or(0) as usize;
                    let component_index = match component_id {
                        1 => 0,
                        2 => 1,
                        3 => 2,
                        _ => return Err(DecodeError::Unsupported),
                    };
                    components[component_index] = Component {
                        horizontal: sampling >> 4,
                        vertical: sampling & 0x0F,
                        quantization,
                        dc_table: 0,
                        ac_table: 0,
                        previous_dc: 0,
                    };
                    maximum_horizontal = maximum_horizontal.max(sampling >> 4);
                    maximum_vertical = maximum_vertical.max(sampling & 0x0F);
                }
                mcus_x = (width as usize + 8 * maximum_horizontal as usize - 1)
                    / (8 * maximum_horizontal as usize);
                mcus_y = (height as usize + 8 * maximum_vertical as usize - 1)
                    / (8 * maximum_vertical as usize);
            }
            0xC4 => {
                let mut position = offset + 2;
                while position < segment_end {
                    let table_id = data.get(position).copied().unwrap_or(0);
                    let class = (table_id >> 4) as usize;
                    let destination = (table_id & 0x0F) as usize;
                    let (table, consumed) = read_huffman(data, position + 1)?;
                    if class == 0 {
                        dc_tables[destination.min(1)] = table;
                    } else {
                        ac_tables[destination.min(1)] = table;
                    }
                    position += 1 + consumed;
                }
            }
            0xDB => {
                let mut position = offset + 2;
                while position < segment_end {
                    let precision = (data[position] >> 4) as usize;
                    let destination = (data[position] & 0x0F) as usize;
                    position += 1;
                    let byte_count = if precision == 0 { 64 } else { 128 };
                    for index in 0..64 {
                        let value = if precision == 0 {
                            u16::from(*data.get(position + index).ok_or(DecodeError::Truncated)?)
                        } else {
                            let pair = data
                                .get(position + index * 2..position + index * 2 + 2)
                                .ok_or(DecodeError::Truncated)?;
                            u16::from_be_bytes([pair[0], pair[1]])
                        };
                        quantization[destination.min(1)][index] = value;
                    }
                    position += byte_count;
                }
            }
            0xDD => {
                restart_interval = be16(data, offset + 2)?;
            }
            0xDA => {
                let scan_component_count =
                    *data.get(offset + 2).ok_or(DecodeError::Truncated)? as usize;
                for index in 0..scan_component_count {
                    let scan_component_offset = offset + 3 + index * 2;
                    let component_id = *data
                        .get(scan_component_offset)
                        .ok_or(DecodeError::Truncated)?;
                    let tables = data
                        .get(scan_component_offset + 1)
                        .copied()
                        .ok_or(DecodeError::Truncated)?;
                    let component_index = match component_id {
                        1 => 0,
                        2 => 1,
                        3 => 2,
                        _ => return Err(DecodeError::Unsupported),
                    };
                    components[component_index].dc_table = (tables >> 4) as usize;
                    components[component_index].ac_table = (tables & 0x0F) as usize;
                }

                let mut scan = Scan {
                    components,
                    mcus_x,
                    mcus_y,
                    restart_interval,
                    dc_tables,
                    ac_tables,
                    quantization,
                };
                let mut position = segment_end;
                while position + 1 < data.len() {
                    if data[position] == 0xFF {
                        let next = data[position + 1];
                        if next == 0 {
                            position += 2;
                        } else if (0xD0..=0xD7).contains(&next) {
                            position += 2;
                        } else {
                            break;
                        }
                    } else {
                        position += 1;
                    }
                }
                let scan_bytes = data
                    .get(segment_end..position)
                    .ok_or(DecodeError::Truncated)?;
                let coefficients = decode_scan(scan_bytes, &mut scan)?;
                let mut output = vec![0u8; width as usize * height as usize * 4];
                for y in 0..height as usize {
                    for x in 0..width as usize {
                        let mut channels = [0u8; 3];
                        for component_index in 0..3 {
                            let component = &scan.components[component_index];
                            let component_width = scan.mcus_x * component.horizontal as usize * 8;
                            let component_height = scan.mcus_y * component.vertical as usize * 8;
                            let scale_x =
                                maximum_horizontal as usize / component.horizontal.max(1) as usize;
                            let scale_y =
                                maximum_vertical as usize / component.vertical.max(1) as usize;
                            channels[component_index] = sample(
                                &coefficients[component_index],
                                component_width,
                                component_height,
                                (x / scale_x) as u32,
                                (y / scale_y) as u32,
                            );
                        }
                        output[(y * width as usize + x) * 4..(y * width as usize + x) * 4 + 4]
                            .copy_from_slice(&ycbcr_to_rgba(channels[0], channels[1], channels[2]));
                    }
                }
                return Ok(Image {
                    width,
                    height,
                    pixels: output,
                });
            }
            _ => {}
        }
        offset = segment_end;
    }
    Err(DecodeError::Truncated)
}
