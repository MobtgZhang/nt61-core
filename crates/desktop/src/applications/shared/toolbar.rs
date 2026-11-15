//! Original accessory toolbar icons. Regenerate with generate-application-toolbars.py.
use crate::graphics::{Rect, Surface};
#[derive(Clone, Copy, Debug)]
#[repr(u32)]
pub enum Icon {
    Paste,
    Cut,
    Copy,
    Save,
    Undo,
    Redo,
    Bold,
    Italic,
    Underline,
    Strike,
    Color,
    Highlight,
    AlignLeft,
    AlignCenter,
    AlignRight,
    IndentLeft,
    IndentRight,
    Bullets,
    Find,
    Replace,
    Calendar,
    SelectAll,
    ZoomIn,
    ZoomOut,
    ActualSize,
    Ruler,
    Status,
    Wrap,
    Grow,
    Shrink,
    Pencil,
    Brush,
    Eraser,
    Fill,
    Picker,
    Line,
    Rectangle,
    Ellipse,
    Rotate,
    FlipH,
    FlipV,
    BrushSize,
    Clear,
    Resize,
    Grid,
}
const COUNT: u32 = 45;
pub fn draw(frame: &mut Surface, atlas: &[u8], icon: Icon, size: u32, bounds: Rect) {
    for y in 0..bounds.height {
        for x in 0..bounds.width {
            let source_x = icon as u32 * size + x * size / bounds.width;
            let source_y = y * size / bounds.height;
            let at = ((source_y * size * COUNT + source_x) * 4) as usize;
            if let Some(pixel) = atlas.get(at..at + 4) {
                let target_x = bounds.x + x as i32;
                let target_y = bounds.y + y as i32;
                if target_x >= 0 && target_y >= 0 {
                    frame.blend_pixel(
                        target_x as u32,
                        target_y as u32,
                        u32::from_le_bytes([pixel[2], pixel[1], pixel[0], pixel[3]]),
                    );
                }
            }
        }
    }
}
