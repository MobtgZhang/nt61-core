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

#[derive(Debug)]
pub enum DecodeError {
    Truncated,
    Unsupported,
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

pub fn decode_bmp(bytes: &[u8]) -> Result<Image, DecodeError> {
    if bytes.len() < 54 || &bytes[0..2] != b"BM" {
        return Err(DecodeError::Unsupported);
    }
    let pixel_offset = read_u32(bytes, 10).ok_or(DecodeError::Truncated)? as usize;
    let _header_size = read_u32(bytes, 14).ok_or(DecodeError::Truncated)? as usize;
    let width = read_u32(bytes, 18).ok_or(DecodeError::Truncated)? as i32;
    let stored_height = read_u32(bytes, 22).ok_or(DecodeError::Truncated)? as i32;
    let bit_count = read_u16(bytes, 28).ok_or(DecodeError::Truncated)?;
    let compression = read_u32(bytes, 30).ok_or(DecodeError::Truncated)?;
    if width <= 0 || stored_height <= 0 || compression != 0 {
        return Err(DecodeError::Unsupported);
    }
    let height = stored_height as usize;
    let width = width as usize;
    let pixel_stride = (width * bit_count as usize).div_ceil(32) * 4;
    let mut pixels = alloc::vec![0u8; width * height * 4];
    for source_row in 0..height {
        let destination_row = (height - source_row - 1) * width * 4;
        for x in 0..width {
            let source_offset =
                pixel_offset + source_row * pixel_stride + x * (bit_count as usize / 8);
            let destination = destination_row + x * 4;
            let chunk = bytes
                .get(source_offset..source_offset + bit_count as usize / 8)
                .ok_or(DecodeError::Truncated)?;
            match bit_count {
                24 => {
                    pixels[destination] = chunk[2];
                    pixels[destination + 1] = chunk[1];
                    pixels[destination + 2] = chunk[0];
                    pixels[destination + 3] = 255;
                }
                32 => {
                    pixels[destination] = chunk[2];
                    pixels[destination + 1] = chunk[1];
                    pixels[destination + 2] = chunk[0];
                    pixels[destination + 3] = chunk[3];
                }
                _ => return Err(DecodeError::Unsupported),
            }
        }
    }
    Ok(Image {
        width: width as u32,
        height: height as u32,
        pixels,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_win7_user_bmp() {
        let image = decode_bmp(include_bytes!(
            "../assets/windows7/User Account Pictures/user.bmp"
        ))
        .expect("user BMP decode");
        assert_eq!(image.width, 128);
        assert_eq!(image.height, 128);
        assert_eq!(image.pixels.len(), 128 * 128 * 4);
    }
}
