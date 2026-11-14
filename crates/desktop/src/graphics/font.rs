use alloc::{rc::Rc, vec::Vec};
use core::cell::RefCell;

use nt61_fonts::{FontFace, FontLineMetrics, GlyphMetrics, RasterizedGlyph};

use crate::graphics::Surface;

pub const UI_FONT: &[u8] = nt61_fonts::UI_FONT;
pub const MONOSPACE_FONT: &[u8] = nt61_fonts::TERMINAL_FONT;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontError {
    InvalidFont,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextError {
    FontUnavailable,
    GlyphUnavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineMetrics {
    pub ascent: i32,
    pub descent: i32,
    pub line_height: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextLayout {
    pub width: u32,
    pub height: u32,
    pub baseline: u32,
    pub line_count: u32,
}

#[derive(Debug)]
pub struct FontLib {
    faces: Vec<FontFace>,
    glyphs: RefCell<Vec<(char, u32, Rc<RasterizedGlyph>)>>,
}

impl FontLib {
    pub fn from_bytes(data: &[u8]) -> Result<Self, FontError> {
        let mut library = Self { faces: Vec::new(), glyphs: RefCell::new(Vec::new()) };
        library.load(data)?;
        Ok(library)
    }

    pub fn ui() -> Result<Self, FontError> {
        Self::from_bytes(UI_FONT)
    }

    pub fn monospace() -> Result<Self, FontError> {
        Self::from_bytes(MONOSPACE_FONT)
    }

    pub fn load(&mut self, data: &[u8]) -> Result<(), FontError> {
        let face = FontFace::from_bytes(data).map_err(|_| FontError::InvalidFont)?;
        self.faces.push(face);
        self.glyphs.get_mut().clear();
        Ok(())
    }

    pub fn face_count(&self) -> usize {
        self.faces.len()
    }

    pub fn face(&self, index: usize) -> Option<&FontFace> {
        self.faces.get(index)
    }

    pub fn has_glyph(&self, ch: char) -> bool {
        self.faces.iter().any(|face| face.has_glyph(ch))
    }

    pub fn line_metrics(&self, pixel_size: u32) -> Option<LineMetrics> {
        let metrics: FontLineMetrics = self.face(0)?.line_metrics(pixel_size)?;
        Some(LineMetrics {
            ascent: metrics.ascent,
            descent: metrics.descent,
            line_height: metrics.line_height,
        })
    }

    pub fn glyph_metrics(&self, ch: char, pixel_size: u32) -> Option<GlyphMetrics> {
        self.face(0)?.metrics(ch, pixel_size)
    }

    pub fn rasterize(&self, ch: char, pixel_size: u32) -> Option<RasterizedGlyph> {
        self.face(0)?.rasterize(ch, pixel_size)
    }

    fn cached_glyph(&self, ch: char, pixel_size: u32) -> Option<Rc<RasterizedGlyph>> {
        if let Some((_, _, glyph)) = self.glyphs.borrow().iter()
            .find(|(key, size, _)| *key == ch && *size == pixel_size) {
            return Some(Rc::clone(glyph));
        }
        let glyph = Rc::new(self.rasterize(ch, pixel_size)?);
        let mut cache = self.glyphs.borrow_mut();
        // Bounded per-renderer storage; coverage is shared across colors and windows.
        if cache.len() == 256 { cache.remove(0); }
        cache.push((ch, pixel_size, Rc::clone(&glyph)));
        Some(glyph)
    }

    pub fn measure(&self, text: &str, pixel_size: u32) -> Option<TextLayout> {
        let mut width = 0_u32;
        let mut line_count = 0_u32;
        for line in text.lines() {
            line_count = line_count.saturating_add(1);
            let mut line_width = 0_u32;
            for ch in line.chars() {
                let metrics = self.glyph_metrics(ch, pixel_size)?;
                line_width = line_width.saturating_add(advance_pixels(metrics.advance_width));
            }
            width = width.max(line_width);
        }
        if line_count == 0 {
            line_count = 1;
        }
        let vertical = self.line_metrics(pixel_size)?;
        let height = vertical
            .line_height
            .saturating_mul(line_count.saturating_sub(1))
            .saturating_add(pixel_size);
        Some(TextLayout {
            width,
            height,
            baseline: vertical.ascent.max(0) as u32,
            line_count,
        })
    }

    /// Long labels are clipped in one pass, without allocating and measuring
    /// every progressively shorter candidate during pointer hover repaints.
    pub fn fitting_prefix<'a>(&self, text: &'a str, pixel_size: u32, width: u32) -> Option<&'a str> {
        let mut used = 0u32;
        for (at, ch) in text.char_indices() {
            if ch == '\n' { return Some(&text[..at]); }
            let advance = advance_pixels(self.glyph_metrics(ch, pixel_size)?.advance_width);
            if advance > width.saturating_sub(used) { return Some(&text[..at]); }
            used += advance;
        }
        Some(text)
    }

    pub fn draw_text(
        &self,
        target: &mut Surface,
        text: &str,
        x: i32,
        y: i32,
        pixel_size: u32,
        color: u32,
    ) -> Result<(), TextError> {
        let vertical = self
            .line_metrics(pixel_size)
            .ok_or(TextError::FontUnavailable)?;
        let mut baseline = y + vertical.ascent;
        for line in text.lines() {
            let mut pen_x = x;
            for ch in line.chars() {
                let glyph = self
                    .cached_glyph(ch, pixel_size)
                    .ok_or(TextError::GlyphUnavailable)?;
                let top_y = baseline - (glyph.metrics.height as i32 + glyph.metrics.ymin);
                let origin_x = pen_x + glyph.metrics.xmin;
                let target_width = target.width() as i32;
                let target_height = target.height() as i32;
                if origin_x < target_width && top_y < target_height {
                    for row in 0..glyph.metrics.height {
                        for column in 0..glyph.metrics.width {
                            let coverage = match glyph.coverage_at(column, row) {
                                Some(0) | None => continue,
                                Some(coverage) => coverage,
                            };
                            let pixel_x = origin_x + column as i32;
                            let pixel_y = top_y + row as i32;
                            if pixel_x < 0 || pixel_y < 0 {
                                continue;
                            }
                            if target.pixel(pixel_x as u32, pixel_y as u32).is_some() {
                                let alpha = ((coverage as u32) << 24) | (color & 0x00FF_FFFF);
                                target.blend_pixel(pixel_x as u32, pixel_y as u32, alpha);
                            }
                        }
                    }
                }
                pen_x += advance_pixels(glyph.metrics.advance_width) as i32;
            }
            baseline += vertical.line_height as i32;
        }
        Ok(())
    }
}

fn advance_pixels(value: f32) -> u32 {
    (value + 0.5).max(0.0) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_ui_font_parses_and_rasterizes() {
        let font = FontLib::ui().expect("embedded UI font must parse");
        assert_eq!(font.face_count(), 1);
        assert!(font.has_glyph('A'));
        assert!(font.rasterize('A', 16).is_some());
        assert!(font.rasterize('\u{FFFF}', 16).is_none());
    }

    #[test]
    fn fitting_prefix_preserves_utf8_and_matches_measured_width() {
        let font = FontLib::ui().unwrap();
        let text = "Explorer café";
        for width in 0..200 {
            let prefix = font.fitting_prefix(text, 14, width).unwrap();
            assert!(text.starts_with(prefix));
            assert!(font.measure(prefix, 14).unwrap().width <= width);
            if let Some(next) = text[prefix.len()..].chars().next() {
                let longer = &text[..prefix.len() + next.len_utf8()];
                assert!(font.measure(longer, 14).unwrap().width > width);
            }
        }
        assert_eq!(font.fitting_prefix("line\nnext", 14, 1000), Some("line"));
    }

    #[test]
    fn text_layout_uses_font_metrics() {
        let font = FontLib::monospace().expect("embedded monospace font must parse");
        let layout = font.measure("AB", 16).expect("layout must be available");
        assert!(layout.width > 0 && layout.height >= 16);
        assert_eq!(layout.line_count, 1);
        assert!(layout.baseline > 0);
    }

    #[test]
    fn text_draws_without_panicking_on_edges() {
        let font = FontLib::ui().expect("embedded UI font must parse");
        let mut surface = Surface::new(16, 16);
        assert!(font
            .draw_text(&mut surface, "A", -16, -16, 12, 0xFF00_0000)
            .is_ok());
        assert!(font
            .draw_text(&mut surface, "A", 32, 32, 12, 0xFF00_0000)
            .is_ok());
    }

    #[test]
    #[ignore = "manual rendering performance measurement"]
    fn repeated_window_labels_benchmark() {
        let font = FontLib::ui().unwrap();
        let mut surface = Surface::new(640, 460);
        let start = std::time::Instant::now();
        for _ in 0..100 {
            for (row, text) in ["Windows Explorer", "Administrator", "Documents", "Computer",
                "Control Panel", "Calculator", "Applications  |  Task  |  Status"].iter().enumerate() {
                font.draw_text(&mut surface, text, 12, row as i32 * 24, 16, 0xFFFFFFFF).unwrap();
            }
        }
        std::println!("100 window label frames: {:?}", start.elapsed());
        assert!(surface.pixels().iter().any(|p| *p != 0));
    }

    #[test]
    fn cached_glyphs_preserve_pixels_and_distinguish_sizes() {
        let font = FontLib::ui().unwrap();
        let first = font.cached_glyph('A', 16).unwrap();
        let again = font.cached_glyph('A', 16).unwrap();
        assert!(Rc::ptr_eq(&first, &again));
        let large = font.cached_glyph('A', 24).unwrap();
        assert!(!Rc::ptr_eq(&first, &large));
        let original = font.rasterize('A', 16).unwrap();
        for y in 0..original.metrics.height {
            for x in 0..original.metrics.width {
                assert_eq!(first.coverage_at(x, y), original.coverage_at(x, y));
            }
        }
        for ch in (32..400).filter_map(char::from_u32) { let _ = font.cached_glyph(ch, 16); }
        assert!(font.glyphs.borrow().len() <= 256);
        let font_data = UI_FONT;
        let mut font = font;
        font.load(font_data).unwrap();
        assert!(font.glyphs.borrow().is_empty());
    }
}
