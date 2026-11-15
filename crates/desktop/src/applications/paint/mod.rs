//! Raster document and pointer tools. Image dimensions are independent of the viewport.
use crate::graphics::Surface;
mod bitmap;
mod raster;
pub mod ui;
use alloc::{string::String, vec, vec::Vec};
pub use bitmap::encode_bmp;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Pencil,
    Brush,
    Eraser,
    Fill,
    Picker,
    Line,
    Rectangle,
    Ellipse,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Paint {
    pub canvas: Surface,
    pub color: u32,
    pub background: u32,
    pub tool: Tool,
    pub size: u32,
    pub dirty: bool,
    pub file_id: Option<u32>,
    pub filename: String,
    pub zoom: u32,
    pub scroll: (u32, u32),
    pub view_tab: bool,
    pub grid: bool,
    undo: Vec<Surface>,
    redo: Vec<Surface>,
    last: Option<(i32, i32)>,
    origin: Option<(i32, i32)>,
    stroke_color: u32,
    saved: u64,
}
pub const MAX_PIXELS: u32 = 1024 * 1024;
const HISTORY_BYTES: usize = 8 * 1024 * 1024;
pub const COLORS: &[u32] = &[
    0xFF000000, 0xFF7F7F7F, 0xFF880015, 0xFFED1C24, 0xFFFF7F27, 0xFFFFF200, 0xFF22B14C, 0xFF00A2E8,
    0xFF3F48CC, 0xFFA349A4, 0xFFFFFFFF, 0xFFC3C3C3, 0xFFB97A57, 0xFFFFAEC9, 0xFFFFC90E, 0xFFEFE4B0,
    0xFFB5E61D, 0xFF99D9EA, 0xFF7092BE, 0xFFC8BFE7,
];
impl Paint {
    pub fn new(width: u32, height: u32) -> Self {
        let (width, height) = if width == 0
            || height == 0
            || width.checked_mul(height).is_none_or(|n| n > MAX_PIXELS)
        {
            (480, 240)
        } else {
            (width, height)
        };
        let mut canvas = Surface::new(width, height);
        canvas.fill_rect(0, 0, width, height, 0xFFFFFFFF);
        let mut p = Self {
            canvas,
            color: COLORS[0],
            background: 0xFFFFFFFF,
            tool: Tool::Pencil,
            size: 2,
            dirty: false,
            file_id: None,
            filename: "Untitled.bmp".into(),
            zoom: 100,
            scroll: (0, 0),
            view_tab: false,
            grid: false,
            undo: Vec::new(),
            redo: Vec::new(),
            last: None,
            origin: None,
            stroke_color: COLORS[0],
            saved: 0,
        };
        p.mark_saved();
        p
    }
    fn fingerprint(&self) -> u64 {
        self.canvas.pixels().iter().fold(
            ((self.canvas.width() as u64) << 32) | self.canvas.height() as u64,
            |h, p| (h ^ *p as u64).wrapping_mul(0x100000001b3),
        )
    }
    pub fn mark_saved(&mut self) {
        self.saved = self.fingerprint();
        self.dirty = false;
    }
    fn changed(&mut self) {
        self.dirty = self.fingerprint() != self.saved;
    }
    fn checkpoint(&mut self) {
        self.end();
        self.redo.clear();
        while !self.undo.is_empty()
            && (self.undo.len() >= 16
                || self
                    .undo
                    .iter()
                    .map(|s| s.pixels().len() * 4)
                    .sum::<usize>()
                    + self.canvas.pixels().len() * 4
                    > HISTORY_BYTES)
        {
            self.undo.remove(0);
        }
        self.undo.push(self.canvas.clone());
    }

    pub fn begin(&mut self, x: i32, y: i32) {
        self.begin_button(x, y, false);
    }
    pub fn begin_button(&mut self, x: i32, y: i32, right: bool) {
        if !self.inside(x, y) {
            return;
        }
        if self.tool == Tool::Picker {
            let c = self.canvas.pixel(x as u32, y as u32).unwrap();
            if right {
                self.background = c;
            } else {
                self.color = c;
            }
            return;
        }
        self.checkpoint();
        self.stroke_color = if right { self.background } else { self.color };
        self.origin = Some((x, y));
        self.last = None;
        self.stroke(x, y);
    }
    pub fn active(&self) -> bool {
        self.origin.is_some()
    }
    pub fn end(&mut self) {
        if self.origin.take().is_some() {
            self.changed();
        }
        self.last = None;
    }
    pub fn undo(&mut self) {
        self.end();
        if let Some(canvas) = self.undo.pop() {
            self.redo.push(core::mem::replace(&mut self.canvas, canvas));
            self.changed();
        }
    }
    pub fn redo(&mut self) {
        self.end();
        if let Some(canvas) = self.redo.pop() {
            self.undo.push(core::mem::replace(&mut self.canvas, canvas));
            self.changed();
        }
    }
    pub fn clear(&mut self) {
        self.checkpoint();
        self.canvas.fill_rect(
            0,
            0,
            self.canvas.width(),
            self.canvas.height(),
            self.background,
        );
        self.changed();
    }
    fn inside(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && x < self.canvas.width() as i32 && y < self.canvas.height() as i32
    }

    pub fn rotate(&mut self) {
        self.checkpoint();
        let (w, h) = (self.canvas.width(), self.canvas.height());
        let mut out = Surface::new(h, w);
        for y in 0..h {
            for x in 0..w {
                out.set_pixel(h - 1 - y, x, self.canvas.pixel(x, y).unwrap());
            }
        }
        self.canvas = out;
        self.scroll = (0, 0);
        self.changed();
    }
    pub fn flip(&mut self, horizontal: bool) {
        self.checkpoint();
        let (w, h) = (self.canvas.width(), self.canvas.height());
        let mut out = Surface::new(w, h);
        for y in 0..h {
            for x in 0..w {
                out.set_pixel(
                    if horizontal { w - 1 - x } else { x },
                    if horizontal { y } else { h - 1 - y },
                    self.canvas.pixel(x, y).unwrap(),
                );
            }
        }
        self.canvas = out;
        self.changed();
    }
    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), &'static str> {
        if width == 0 || height == 0 || width.checked_mul(height).is_none_or(|n| n > MAX_PIXELS) {
            return Err("Invalid image dimensions.");
        }
        self.checkpoint();
        let mut out = Surface::new(width, height);
        for y in 0..height {
            for x in 0..width {
                out.set_pixel(
                    x,
                    y,
                    self.canvas
                        .pixel(
                            x * self.canvas.width() / width,
                            y * self.canvas.height() / height,
                        )
                        .unwrap(),
                );
            }
        }
        self.canvas = out;
        self.scroll = (0, 0);
        self.changed();
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strokes_are_continuous_undo_and_bmp_round_trip() {
        let mut p = Paint::new(20, 20);
        p.begin(1, 1);
        p.stroke(16, 16);
        p.end();
        assert_eq!(p.canvas.pixel(8, 8), Some(COLORS[0]));
        let b = encode_bmp(&p.canvas);
        let decoded = crate::bmp::decode_bmp(&b).expect("bmp");
        assert_eq!(decoded.width, 20);
        p.undo();
        assert_eq!(p.canvas.pixel(8, 8), Some(0xFFFFFFFF));
        p.tool = Tool::Fill;
        p.color = COLORS[3];
        p.begin(0, 0);
        assert_eq!(p.canvas.pixel(19, 19), Some(COLORS[3]));
    }
}
