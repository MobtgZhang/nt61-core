use super::clip::Rect;
use super::surface::Surface;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgba(pub u8, pub u8, pub u8, pub u8);

pub fn blit_rgba(destination: &mut Surface, source: &[u8], source_stride: usize, bounds: Rect) {
    let destination_rect = Rect::new(0, 0, destination.width(), destination.height());
    let Some(clip) = destination_rect.intersection(&bounds) else {
        return;
    };
    // `bounds` may start outside the destination.  The first visible pixel is
    // therefore offset by the distance between the clipped and requested rect.
    let offset_x = clip.x - bounds.x;
    let offset_y = clip.y - bounds.y;
    for row in 0..clip.height {
        for column in 0..clip.width {
            let source_x = offset_x + column as i32;
            let source_y = offset_y + row as i32;
            if let Some(pixel) = source_pixel(source, source_stride, source_x, source_y) {
                destination.blend_pixel(
                    (clip.x + column as i32) as u32,
                    (clip.y + row as i32) as u32,
                    pixel,
                );
            }
        }
    }
}

fn source_pixel(source: &[u8], source_stride: usize, source_x: i32, source_y: i32) -> Option<u32> {
    if source_x < 0 || source_y < 0 {
        return None;
    }
    let row = usize::try_from(source_y).ok()?;
    let column = usize::try_from(source_x).ok()?;
    let offset = row
        .checked_mul(source_stride)?
        .checked_add(column.checked_mul(4)?)?;
    let chunk = source.get(offset..offset.checked_add(4)?)?;
    Some(u32::from_le_bytes([chunk[2], chunk[1], chunk[0], chunk[3]]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blit_clips_negative_and_out_of_bounds_regions() {
        let mut destination = Surface::new(2, 2);
        let source = [
            0xFF, 0x00, 0x00, 0xFF, 0xFF, 0x00, 0x00, 0xFF, 0xFF, 0x00, 0x00, 0xFF, 0xFF, 0x00,
            0x00, 0xFF,
        ];
        blit_rgba(&mut destination, &source, 4, Rect::new(-1, -1, 2, 2));
        assert_eq!(destination.pixel(0, 0), Some(0xFFFF_0000));
        blit_rgba(&mut destination, &source, 4, Rect::new(2, 2, 2, 2));
        assert_eq!(destination.pixel(1, 1), Some(0));
    }

    #[test]
    fn blit_rejects_entirely_negative_bounds() {
        let mut destination = Surface::new(1, 1);
        let source = [0xFF, 0, 0, 0xFF];
        blit_rgba(&mut destination, &source, 4, Rect::new(-2, -2, 1, 1));
        assert_eq!(destination.pixel(0, 0), Some(0));
    }

    #[test]
    fn blit_clips_when_source_bounds_exceed_source_data() {
        let mut destination = Surface::new(2, 1);
        let source = [0x80, 0x40, 0x20, 0xFF];
        blit_rgba(&mut destination, &source, 4, Rect::new(0, 0, 4, 1));
        assert_eq!(destination.pixel(0, 0), Some(0xFF80_4020));
        assert_eq!(destination.pixel(1, 0), Some(0));
    }
}
