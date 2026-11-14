//! Convert the composed desktop into a persistent RGBA staging buffer.
use super::{Rect, Surface};

/// Returns the bounding rectangle of changed pixels. The first frame must
/// start with a zeroed staging buffer; subsequent frames reuse that buffer.
pub fn update_rgba(surface: &Surface, rgba: &mut [u8]) -> Option<Rect> {
    if rgba.len() != surface.pixels().len().checked_mul(4)? { return None; }
    let w = surface.width() as usize;
    if w == 0 { return None; }
    let (mut left, mut top, mut right, mut bottom) = (w, surface.height() as usize, 0, 0);
    for (index, (pixel, color)) in rgba.chunks_exact_mut(4).zip(surface.pixels()).enumerate() {
        let next = [(*color >> 16) as u8, (*color >> 8) as u8, *color as u8, (*color >> 24) as u8];
        if pixel == next { continue; }
        pixel.copy_from_slice(&next);
        let (x, y) = (index % w, index / w);
        left = left.min(x); top = top.min(y); right = right.max(x + 1); bottom = bottom.max(y + 1);
    }
    (right > left && bottom > top).then(|| Rect::new(left as i32, top as i32, (right-left) as u32, (bottom-top) as u32))
}

/// Cursor RGBA over an opaque ARGB desktop. Preserve channel order for
/// antialiased edges as well as fully opaque cursor pixels.
pub fn cursor_over(background: u32, source: &[u8]) -> [u8; 4] {
    let alpha = source[3] as u32;
    let inverse = 255 - alpha;
    [(((background >> 16 & 255) * inverse + source[0] as u32 * alpha + 127) / 255) as u8,
     (((background >> 8 & 255) * inverse + source[1] as u32 * alpha + 127) / 255) as u8,
     (((background & 255) * inverse + source[2] as u32 * alpha + 127) / 255) as u8, 255]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unchanged_frame_submits_nothing_and_hover_submits_only_damage() {
        let mut surface = Surface::new(1024, 768);
        surface.fill_rect(0,0,1024,768,0xff123456);
        let mut rgba = alloc::vec![0;1024*768*4];
        assert_eq!(update_rgba(&surface, &mut rgba), Some(Rect::new(0,0,1024,768)));
        assert_eq!(update_rgba(&surface, &mut rgba), None);
        surface.fill_rect(12,12,96,88,0xff89abcd);
        assert_eq!(update_rgba(&surface, &mut rgba), Some(Rect::new(12,12,96,88)));
        assert_eq!(&rgba[(12*1024+12)*4..][..4], &[0x89,0xab,0xcd,255]);
    }
    #[test]
    fn cursor_edges_preserve_red_and_blue() {
        assert_eq!(cursor_over(0xff0000ff, &[255,0,0,128]), [128,0,127,255]);
        assert_eq!(cursor_over(0xff123456, &[255,0,0,0]), [0x12,0x34,0x56,255]);
        assert_eq!(cursor_over(0xff123456, &[250,1,5,255]), [250,1,5,255]);
    }
}
