//! STRINGBITMAP layout and monochrome packing, separated from font realization.
//!
//! Win7's GreGetStringBitmapW uses RFONT/GLYPHDATA fixed-point metrics, rather
//! than GetGlyphOutline's ink rectangle. Callers must supply realized metrics;
//! this module does not infer them from an SFNT outline or expose a syscall.
extern crate alloc;

use alloc::vec::Vec;

const MAX_BYTES: usize = 0x2710000;

/// Metrics used by Win7's vInitSimple and GreGetStringBitmapW. Horizontal
/// quantities are signed 28.4 fixed point; cell dimensions are device pixels.
pub(super) struct Metrics {
    pub a: i32,
    pub ab: i32,
    pub advance: i32,
    pub cell_height: i32,
    pub ascent: i32,
    /// Zero selects proportional layout; positive values are fixed cell width.
    pub character_increment: i32,
    /// FONTOBJ.flFontType FO_CLEARTYPE_X expands the ESTROBJ background.
    /// This controls layout only; supplied glyph pixels are still monochrome.
    pub clear_type_x: bool,
}

pub(super) struct Layout {
    pub width: usize,
    pub height: usize,
    pub stride: usize,
    pub required: usize,
    horizontal_shift: i32,
    ascent: i32,
}

impl Layout {
    pub fn new(metrics: Metrics) -> Option<Self> {
        let (mut left, mut right) = if metrics.character_increment == 0 {
            (metrics.a >> 4, metrics.ab.checked_add(15)? >> 4)
        } else if metrics.character_increment > 0 {
            (0, metrics.character_increment)
        } else {
            return None;
        };
        if metrics.clear_type_x {
            left = left.checked_sub(1)?;
            right = right.checked_add(1)?;
        }
        let c = metrics.advance.checked_sub(metrics.ab)? >> 4;
        let width = (metrics.a >> 4)
            .checked_sub(left)?
            .checked_add(right)?
            .checked_add(c)?;
        let width = usize::try_from(width).ok()?;
        let height = usize::try_from(metrics.cell_height).ok()?;
        let stride = width.checked_add(7)? / 8;
        let required = stride.checked_mul(height)?.checked_add(8)?;
        if required > MAX_BYTES {
            return None;
        }
        Some(Self {
            width,
            height,
            stride,
            required,
            horizontal_shift: left.min(0).checked_neg()?,
            ascent: metrics.ascent,
        })
    }

    /// Pack a top-down, high-bit-first glyph into Win7's byte-aligned cell.
    /// Row padding is ignored. Requests outside the supplied cell fail before
    /// allocation; the caller must resolve font metrics and linking first.
    pub fn pack(
        &self,
        origin: [i32; 2],
        width: usize,
        height: usize,
        source_stride: usize,
        source: &[u8],
    ) -> Option<Vec<u8>> {
        let row_bytes = width.checked_add(7)? / 8;
        if source_stride < row_bytes || source.len() < source_stride.checked_mul(height)? {
            return None;
        }
        let x = usize::try_from(origin[0].checked_add(self.horizontal_shift)?).ok()?;
        let y = usize::try_from(origin[1].checked_add(self.ascent)?).ok()?;
        if width != 0
            && height != 0
            && (x.checked_add(width)? > self.width || y.checked_add(height)? > self.height)
        {
            return None;
        }
        let mut output = Vec::new();
        output.try_reserve_exact(self.required).ok()?;
        output.extend_from_slice(&u32::try_from(self.width).ok()?.to_le_bytes());
        output.extend_from_slice(&u32::try_from(self.height).ok()?.to_le_bytes());
        output.resize(self.required, 0);
        if width == 0 || height == 0 {
            return Some(output);
        }
        for row in 0..height {
            for column in 0..width {
                if source[row * source_stride + column / 8] & (0x80 >> (column % 8)) != 0 {
                    let destination = 8 + (y + row) * self.stride + (x + column) / 8;
                    output[destination] |= 0x80 >> ((x + column) % 8);
                }
            }
        }
        Some(output)
    }
}

#[cfg(test)]
mod tests {
    use super::{Layout, Metrics};

    #[test]
    fn layout_and_pixels_match_original_instructions() {
        let mut capture: &[u8] = include_bytes!("../../../tests/data/win7-stringbitmap-layout.bin");
        let mut count = 0;
        while !capture.is_empty() {
            let mut word = || {
                let result = i32::from_le_bytes(capture[..4].try_into().unwrap());
                capture = &capture[4..];
                result
            };
            let flags = word();
            let fixed = flags & 1 != 0;
            let a = word();
            let b = word();
            let c = word();
            let height = word();
            let expected_size = word() as usize;
            let advance = a + b + c;
            let layout = Layout::new(Metrics {
                a: a * 16,
                ab: (a + b) * 16,
                advance: advance * 16,
                cell_height: height + 4,
                ascent: height + 2,
                character_increment: if fixed { advance } else { 0 },
                clear_type_x: flags & 2 != 0,
            })
            .unwrap();
            let stride = (b as usize + 7) / 8;
            let source: alloc::vec::Vec<u8> = (0..height as usize)
                .flat_map(|row| {
                    (0..stride).map(move |column| (row * 71 + column * 37 + 0x96) as u8)
                })
                .collect();
            let actual = layout
                .pack(
                    [a, -height - 1],
                    b as usize,
                    height as usize,
                    stride,
                    &source,
                )
                .unwrap();
            assert_eq!(layout.required, expected_size);
            assert_eq!(actual, &capture[..expected_size]);
            capture = &capture[expected_size..];
            count += 1;
        }
        assert_eq!(count, 320);
    }

    #[test]
    fn layout_preserves_fractional_bearings_and_rejects_invalid_sizes() {
        let layout = Layout::new(Metrics {
            a: -1,
            ab: 15,
            advance: 32,
            cell_height: 4,
            ascent: 3,
            character_increment: 0,
            clear_type_x: false,
        })
        .unwrap();
        assert_eq!((layout.width, layout.height, layout.required), (2, 4, 12));
        let pixels = layout.pack([-1, -3], 1, 1, 1, &[0x80]).unwrap();
        assert_eq!(&pixels[8..], &[0x80, 0, 0, 0]);
        assert!(layout.pack([2, 0], 1, 1, 1, &[0x80]).is_none());
        assert!(layout.pack([0, 0], 9, 1, 1, &[0xff]).is_none());
        assert!(Layout::new(Metrics {
            a: 0,
            ab: i32::MAX,
            advance: i32::MAX,
            cell_height: 4,
            ascent: 3,
            character_increment: 0,
            clear_type_x: false,
        })
        .is_none());
        assert!(Layout::new(Metrics {
            a: 0,
            ab: 16,
            advance: 16,
            cell_height: -1,
            ascent: 3,
            character_increment: 0,
            clear_type_x: false,
        })
        .is_none());
    }
}
