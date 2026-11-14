use alloc::vec::Vec;

use nt61_desktop::graphics::font::FontLib;
use nt61_desktop::graphics::Surface;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Ink {
    White,
    Black,
}

pub struct FontPack {
    font: FontLib,
    size: u32,
}

#[allow(dead_code)]
impl FontPack {
    pub fn new(size: u32) -> Option<Self> {
        let font = FontLib::ui().ok()?;
        Some(Self { font, size })
    }

    pub fn measure(&self, text: &str) -> u32 {
        self.font
            .measure(text, self.size)
            .map(|layout| layout.width)
            .unwrap_or(0)
    }

    pub fn draw(&self, text: &str, x: u32, y: u32, ink: Ink) {
        let layout = match self.font.measure(text, self.size) {
            Some(layout) => layout,
            None => return,
        };
        let mut surface = Surface::new(layout.width.max(1), self.size);
        let color = match ink {
            Ink::White => 0xFFFF_FFFF,
            Ink::Black => 0xFF00_0000,
        };
        if self
            .font
            .draw_text(&mut surface, text, 0, 0, self.size, color)
            .is_err()
        {
            return;
        }
        let mut pixels = Vec::with_capacity(surface.pixels().len() * 4);
        for pixel in surface.pixels() {
            pixels.push((pixel >> 16) as u8);
            pixels.push((pixel >> 8) as u8);
            pixels.push(*pixel as u8);
            pixels.push((pixel >> 24) as u8);
        }
        unsafe {
            crate::arch::draw_rgba(
                pixels.as_ptr(),
                x,
                y,
                surface.width(),
                surface.height(),
                surface.width() as usize * 4,
            );
        }
    }

    pub fn draw_labelled(&self, text: &str, x: u32, y: u32) {
        self.draw(text, x + 1, y + 1, Ink::Black);
        self.draw(text, x, y, Ink::White);
    }

    pub fn draw_onto(&self, surface: &mut Surface, text: &str, x: i32, y: i32, ink: Ink) {
        let color = match ink {
            Ink::White => 0xFFFF_FFFF,
            Ink::Black => 0xFF00_0000,
        };
        let _ = self.font.draw_text(surface, text, x, y, self.size, color);
    }

    pub fn draw_color(&self, surface: &mut Surface, text: &str, x: i32, y: i32, color: u32) {
        let _ = self.font.draw_text(surface, text, x, y, self.size, color);
    }

    pub fn draw_fit(
        &self,
        surface: &mut Surface,
        text: &str,
        x: i32,
        y: i32,
        width: u32,
        color: u32,
    ) {
        if self.measure(text) <= width {
            self.draw_color(surface, text, x, y, color);
            return;
        }
        let dots = self.measure("...");
        if dots > width { return; }
        if let Some(prefix) = self.font.fitting_prefix(text, self.size, width - dots) {
            let shortened = alloc::format!("{prefix}...");
            self.draw_color(surface, &shortened, x, y, color);
        }
    }

    pub fn draw_labelled_onto(&self, surface: &mut Surface, text: &str, x: i32, y: i32) {
        self.draw_onto(surface, text, x + 1, y + 1, Ink::Black);
        self.draw_onto(surface, text, x, y, Ink::White);
    }
}
