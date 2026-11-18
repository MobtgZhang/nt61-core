//! Code-drawn media icon: blue glass tile and orange play triangle. No ISO artwork payload.
extern crate alloc;
use alloc::vec::Vec;
pub const RGBA: [u8; 64 * 64 * 4] = raster();
const fn raster() -> [u8; 64 * 64 * 4] {
    let mut out = [0; 64 * 64 * 4];
    let mut y = 0;
    while y < 64 {
        let mut x = 0;
        while x < 64 {
            let dx = x as i32 - 31;
            let dy = y as i32 - 31;
            let i = (y * 64 + x) * 4;
            // Three translucent glass sheets, with the orange disc in front.
            if x >= 8 && x <= 57 && y >= 8 && y <= 56 {
                let border = x <= 10 || x >= 54 || y <= 10 || y >= 54;
                out[i] = if border {
                    82
                } else {
                    160 + (55 - (if y < 55 { y } else { 55 })) as u8
                };
                out[i + 1] = if border {
                    155
                } else {
                    204 + (55 - (if y < 55 { y } else { 55 })) as u8 / 2
                };
                out[i + 2] = 236;
                out[i + 3] = if border { 240 } else { 190 };
                if x == 47 || x == 52 {
                    out[i] = 80;
                    out[i + 1] = 158;
                    out[i + 2] = 204;
                    out[i + 3] = 220;
                }
            }
            if dx * dx + dy * dy < 22 * 22 {
                let ring = dx * dx + dy * dy > 20 * 20;
                out[i] = 255;
                out[i + 1] = if ring { 204 } else { 130 + (53 - y) as u8 };
                out[i + 2] = if ring { 130 } else { 24 };
                out[i + 3] = 255;
                if x >= 25 && x <= 44 && (dy.abs() * 20) < (45 - x) as i32 * 13 {
                    out[i] = 255;
                    out[i + 1] = 255;
                    out[i + 2] = 255;
                }
            }
            x += 1;
        }
        y += 1;
    }
    out
}
pub fn ico() -> Vec<u8> {
    let mut out = Vec::new();
    out.extend([0, 0, 1, 0, 1, 0, 64, 64, 0, 0, 1, 0, 32, 0]);
    let length = 40 + 64 * 64 * 4 + 64 * 8;
    out.extend((length as u32).to_le_bytes());
    out.extend(22u32.to_le_bytes());
    out.extend(40u32.to_le_bytes());
    out.extend(64u32.to_le_bytes());
    out.extend(128u32.to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(32u16.to_le_bytes());
    out.extend([0; 24]);
    for y in (0..64).rev() {
        for x in 0..64 {
            let i = (y * 64 + x) * 4;
            out.extend([RGBA[i + 2], RGBA[i + 1], RGBA[i], RGBA[i + 3]]);
        }
    }
    for y in (0..64).rev() {
        for group in 0..8 {
            let mut mask = 0u8;
            for bit in 0..8 {
                if RGBA[(y * 64 + group * 8 + bit) * 4 + 3] == 0 {
                    mask |= 1 << (7 - bit);
                }
            }
            out.push(mask);
        }
    }
    out
}
