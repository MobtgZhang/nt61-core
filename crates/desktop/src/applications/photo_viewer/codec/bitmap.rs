//! Checked Windows DIB reader. BI_RGB and BI_BITFIELDS, including top-down rows.
use super::{dimensions, le16};
use crate::graphics::Surface;

fn long(b: &[u8], at: usize) -> Result<u32, &'static str> {
    Ok(u32::from_le_bytes(
        b.get(at..at + 4)
            .ok_or("Incomplete bitmap header.")?
            .try_into()
            .map_err(|_| "Invalid bitmap.")?,
    ))
}
fn channel(value: u32, mask: u32) -> u32 {
    if mask == 0 {
        return 0;
    }
    let shift = mask.trailing_zeros();
    let max = mask >> shift;
    (((value & mask) >> shift) as u64 * 255 / max as u64) as u32
}
pub(super) fn decode(b: &[u8]) -> Result<Surface, &'static str> {
    let base = if b.starts_with(b"BM") { 14 } else { 0 };
    let header = long(b, base)? as usize;
    if !matches!(header, 40 | 52 | 56 | 108 | 124) || b.len() < base + header {
        return Err("Unsupported bitmap header.");
    }
    let w = long(b, base + 4)? as i32;
    let signed_h = long(b, base + 8)? as i32;
    if w <= 0 || signed_h == i32::MIN {
        return Err("Invalid bitmap dimensions.");
    }
    let (w, h) = (w as u32, signed_h.unsigned_abs());
    dimensions(w, h)?;
    let depth = le16(b, base + 14)? as usize;
    let compression = long(b, base + 16)?;
    if le16(b, base + 12)? != 1
        || !matches!(depth, 1 | 4 | 8 | 16 | 24 | 32)
        || !(compression == 0 || compression == 3 && matches!(depth, 16 | 32))
    {
        return Err("Unsupported bitmap compression or depth.");
    }
    let palette_count = if depth <= 8 {
        let used = long(b, base + 32)? as usize;
        if used > 1usize << depth {
            return Err("Invalid bitmap palette.");
        }
        if used == 0 {
            1usize << depth
        } else {
            used
        }
    } else {
        0
    };
    let masks_size = if compression == 3 && header == 40 {
        12
    } else {
        0
    };
    let palette_at = base + header + masks_size;
    let data_start = palette_at + palette_count * 4;
    let offset = if base != 0 {
        long(b, 10)? as usize
    } else {
        data_start
    };
    if offset < data_start || offset > b.len() {
        return Err("Invalid bitmap pixel offset.");
    }
    let stride = (w as usize * depth).div_ceil(32) * 4;
    let end = offset
        .checked_add(
            stride
                .checked_mul(h as usize)
                .ok_or("Invalid bitmap rows.")?,
        )
        .ok_or("Invalid bitmap rows.")?;
    if end > b.len() {
        return Err("Incomplete bitmap pixels.");
    }
    let (red, green, blue, alpha) = if compression == 3 {
        let m = base + 40;
        (
            long(b, m)?,
            long(b, m + 4)?,
            long(b, m + 8)?,
            if header >= 56 { long(b, m + 12)? } else { 0 },
        )
    } else if depth == 16 {
        (0x7c00, 0x03e0, 0x001f, 0)
    } else {
        (0xff0000, 0xff00, 0xff, 0)
    };
    let masks = [red, green, blue, alpha];
    for (i, mask) in masks
        .iter()
        .enumerate()
        .filter(|_| matches!(depth, 16 | 32))
    {
        if *mask == 0 {
            if i < 3 {
                return Err("Invalid bitmap channel mask.");
            }
            continue;
        }
        if mask.trailing_zeros() + mask.count_ones() > depth as u32
            || (mask >> mask.trailing_zeros()).count_ones()
                != (mask >> mask.trailing_zeros()).trailing_ones()
            || masks[..i].iter().any(|other| other & mask != 0)
        {
            return Err("Invalid bitmap channel mask.");
        }
    }
    let mut s = Surface::try_new(w, h).ok_or("Not enough memory to display this bitmap.")?;
    for y in 0..h {
        let row = offset + if signed_h < 0 { y } else { h - 1 - y } as usize * stride;
        for x in 0..w as usize {
            let pixel = if depth <= 8 {
                let bit = x * depth;
                let index =
                    (b[row + bit / 8] >> (8 - depth - bit % 8)) as usize & ((1 << depth) - 1);
                if index >= palette_count {
                    return Err("Invalid bitmap palette index.");
                }
                let p = palette_at + index * 4;
                0xff000000 | (b[p + 2] as u32) << 16 | (b[p + 1] as u32) << 8 | b[p] as u32
            } else if depth == 24 {
                let p = row + x * 3;
                0xff000000 | (b[p + 2] as u32) << 16 | (b[p + 1] as u32) << 8 | b[p] as u32
            } else {
                let p = row + x * depth / 8;
                let value = if depth == 16 {
                    le16(b, p)? as u32
                } else {
                    long(b, p)?
                };
                let a = if alpha == 0 {
                    255
                } else {
                    channel(value, alpha)
                };
                a << 24
                    | channel(value, red) << 16
                    | channel(value, green) << 8
                    | channel(value, blue)
            };
            s.set_pixel(x as u32, y, pixel);
        }
    }
    Ok(s)
}
