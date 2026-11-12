extern crate alloc;

use super::png::Image;
use alloc::vec;
use alloc::vec::Vec;

#[derive(Debug)]
pub enum DecodeError {
    Truncated,
    Unsupported,
}

pub(crate) struct DirectoryEntry {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) bit_count: u8,
    pub(crate) payload: Vec<u8>,
}

impl DirectoryEntry {
    fn area(&self) -> u64 {
        u64::from(self.width) * u64::from(self.height)
    }

    fn quality(&self) -> u8 {
        if self.payload.get(0..4) == Some(&[0x89, 0x50, 0x4E, 0x47]) {
            32
        } else {
            self.bit_count
        }
    }
}

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    bytes
        .get(offset..offset + 2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    bytes
        .get(offset..offset + 4)
        .map(|bytes| u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

pub(crate) fn directory_entries(bytes: &[u8]) -> Result<Vec<DirectoryEntry>, DecodeError> {
    if bytes.len() < 6 || bytes[0..2] != [0, 0] {
        return Err(DecodeError::Unsupported);
    }
    let type_id = read_u16(bytes, 2).ok_or(DecodeError::Truncated)?;
    if type_id != 1 && type_id != 2 {
        return Err(DecodeError::Unsupported);
    }
    let count = read_u16(bytes, 4).ok_or(DecodeError::Truncated)? as usize;
    if count == 0 || count > 64 {
        return Err(DecodeError::Unsupported);
    }

    let mut entries = Vec::with_capacity(count);
    for index in 0..count {
        let header_offset = 6 + index * 16;
        let width = *bytes.get(header_offset).ok_or(DecodeError::Truncated)? as u32;
        let width = if width == 0 { 256 } else { width };
        let height = *bytes.get(header_offset + 1).ok_or(DecodeError::Truncated)? as u32;
        let height = if height == 0 { 256 } else { height };
        let color_plane_offset = header_offset + 6;
        let bit_count = *bytes
            .get(color_plane_offset + 2)
            .ok_or(DecodeError::Truncated)?;
        let size = read_u32(bytes, header_offset + 8).ok_or(DecodeError::Truncated)? as usize;
        let offset = read_u32(bytes, header_offset + 12).ok_or(DecodeError::Truncated)? as usize;
        let end = offset.checked_add(size).ok_or(DecodeError::Truncated)?;
        let payload = bytes
            .get(offset..end)
            .ok_or(DecodeError::Truncated)?
            .to_vec();
        entries.push(DirectoryEntry {
            width,
            height,
            bit_count,
            payload,
        });
    }
    Ok(entries)
}

pub(crate) fn decode_bmp(payload: &[u8]) -> Result<Image, DecodeError> {
    if payload.len() < 40 {
        return Err(DecodeError::Truncated);
    }
    let header_size = read_u32(payload, 0).ok_or(DecodeError::Truncated)? as usize;
    if header_size < 40 {
        return Err(DecodeError::Unsupported);
    }
    let width = read_u32(payload, 4).ok_or(DecodeError::Truncated)? as i32;
    let stored_height = read_u32(payload, 8).ok_or(DecodeError::Truncated)? as i32;
    if width <= 0 || stored_height <= 0 || stored_height % 2 != 0 {
        return Err(DecodeError::Unsupported);
    }
    let height = stored_height / 2;
    let bit_count = read_u16(payload, 14).ok_or(DecodeError::Truncated)?;
    let compression = read_u32(payload, 16).ok_or(DecodeError::Truncated)?;
    if compression != 0 {
        return Err(DecodeError::Unsupported);
    }

    let pixel_stride = ((width as usize * bit_count as usize + 31) / 32) * 4;
    let mask_stride = ((width as usize + 31) / 32) * 4;
    let pixel_bytes = pixel_stride * height as usize;
    let palette_bytes = match bit_count {
        1 => 2usize,
        4 => 16usize,
        8 => 256usize,
        _ => 0usize,
    };
    let palette_offset = header_size;
    let pixel_offset = palette_offset + palette_bytes * 4;
    let mask_offset = pixel_offset + pixel_bytes;
    let mut pixels = vec![0u8; width as usize * height as usize * 4];
    for y in 0..height as usize {
        let source_row = pixel_offset + y * pixel_stride;
        let destination_row = (height as usize - y - 1) * width as usize * 4;
        for x in 0..width as usize {
            let (blue, green, red, alpha) = match bit_count {
                32 => {
                    let offset = source_row + x * 4;
                    let bytes = payload
                        .get(offset..offset + 4)
                        .ok_or(DecodeError::Truncated)?;
                    (bytes[0], bytes[1], bytes[2], bytes[3])
                }
                24 => {
                    let offset = source_row + x * 3;
                    let bytes = payload
                        .get(offset..offset + 3)
                        .ok_or(DecodeError::Truncated)?;
                    (bytes[0], bytes[1], bytes[2], 255)
                }
                bit_count @ (1 | 4 | 8) => {
                    let byte_offset = source_row + x * bit_count as usize / 8;
                    let byte = *payload.get(byte_offset).ok_or(DecodeError::Truncated)?;
                    let shift = 8 - bit_count as u32 - (x as u32 * bit_count as u32 % 8);
                    let index = ((byte >> shift) as usize) & ((1u32 << bit_count) as usize - 1);
                    let palette = payload
                        .get(palette_offset + index * 4..palette_offset + index * 4 + 4)
                        .ok_or(DecodeError::Truncated)?;
                    (palette[0], palette[1], palette[2], 255)
                }
                _ => return Err(DecodeError::Unsupported),
            };
            let mask_offset_x = mask_offset + y * mask_stride + x / 8;
            let mask_bit =
                *payload.get(mask_offset_x).ok_or(DecodeError::Truncated)? & (0x80 >> (x % 8));
            let alpha = if bit_count == 32 {
                if alpha == 0 && mask_bit == 0 {
                    255
                } else {
                    alpha
                }
            } else if mask_bit == 0 {
                255
            } else {
                0
            };
            let destination = destination_row + x * 4;
            pixels[destination] = red;
            pixels[destination + 1] = green;
            pixels[destination + 2] = blue;
            pixels[destination + 3] = alpha;
        }
    }
    Ok(Image {
        width: width as u32,
        height: height as u32,
        pixels,
    })
}

pub fn decode_ico(bytes: &[u8]) -> Result<Image, DecodeError> {
    let mut selected = None;
    let mut selected_area = 0u64;
    let mut selected_quality = 0u8;
    for entry in directory_entries(bytes)? {
        let area = entry.area();
        let quality = entry.quality();
        if (quality, area) <= (selected_quality, selected_area) {
            continue;
        }
        let image = if entry.payload.get(0..4) == Some(&[0x89, 0x50, 0x4E, 0x47]) {
            match super::png::decode_png(&entry.payload) {
                Ok(image) => image,
                Err(_) => continue,
            }
        } else {
            match decode_bmp(&entry.payload) {
                Ok(image) => image,
                Err(_) => continue,
            }
        };
        selected = Some(image);
        selected_area = area;
        selected_quality = quality;
    }
    selected.ok_or(DecodeError::Unsupported)
}

pub fn decode_ico_preferred(bytes: &[u8], preferred_size: u32) -> Result<Image, DecodeError> {
    let mut selected = None;
    let mut selected_distance = u32::MAX;
    let mut selected_quality = 0u8;
    let mut selected_area = 0u64;
    for entry in directory_entries(bytes)? {
        let distance = entry.width.abs_diff(preferred_size) + entry.height.abs_diff(preferred_size);
        let area = entry.area();
        let quality = entry.quality();
        let better = distance < selected_distance
            || (distance == selected_distance
                && (quality > selected_quality
                    || (quality == selected_quality && area > selected_area)));
        if !better {
            continue;
        }
        let image = if entry.payload.get(0..4) == Some(&[0x89, 0x50, 0x4E, 0x47]) {
            match super::png::decode_png(&entry.payload) {
                Ok(image) => image,
                Err(_) => continue,
            }
        } else {
            match decode_bmp(&entry.payload) {
                Ok(image) => image,
                Err(_) => continue,
            }
        };
        selected = Some(image);
        selected_distance = distance;
        selected_quality = quality;
        selected_area = area;
    }
    selected.ok_or(DecodeError::Unsupported)
}

#[cfg(test)]
mod tests {
    #[test]
    fn decodes_win7_flag_ico() {
        let bytes = include_bytes!("../../../assets/windows7/icons/shell32/ICON34_1.ico");
        match super::decode_ico(bytes) {
            Ok(image) => assert!(image.width > 0 && image.height > 0),
            Err(error) => panic!("flag ICO decode failed: {error:?}"),
        }
    }
}
