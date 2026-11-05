#![allow(dead_code)]
// Shared Open Sans glyph renderer for the Ring-3 desktop binaries.
//
// The glyph atlas is produced at build time by
// crates/desktop/tools/gen_assets.py from the Open Sans family in
// resources/fonts/open-sans. Glyphs are blitted with per-pixel alpha
// through the kernel's SYS_DESKTOP_DRAW_RGBA (source-over blending).

#[path = "font_atlas_data.rs"]
pub mod atlas;

use crate::png;
use alloc::vec::Vec;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Ink {
    White,
    Black,
}

pub struct FontPack {
    atlas_white: png::Image,
    atlas_black: png::Image,
}

impl FontPack {
    pub fn decode() -> Option<Self> {
        let atlas_white = png::decode_png(atlas::ATLAS_WHITE).ok()?;
        let atlas_black = png::decode_png(atlas::ATLAS_BLACK).ok()?;
        Some(Self {
            atlas_white,
            atlas_black,
        })
    }

    pub fn ui(&self) -> &[atlas::Glyph] {
        &atlas::GLYPHS_UI
    }

    pub fn brand(&self) -> &[atlas::Glyph] {
        &atlas::GLYPHS_BRAND
    }

    pub fn measure(&self, glyphs: &[atlas::Glyph], text: &str) -> u32 {
        let mut total = 0u32;
        for byte in text.bytes() {
            if (byte as u32) < atlas::FIRST_CHAR || (byte as u32) >= atlas::LAST_CHAR {
                continue;
            }
            total += glyphs[(byte as usize) - atlas::FIRST_CHAR as usize].advance as u32;
        }
        total
    }

    pub fn draw(&self, glyphs: &[atlas::Glyph], text: &str, x: u32, y: u32, ink: Ink) {
        let source = match ink {
            Ink::White => &self.atlas_white,
            Ink::Black => &self.atlas_black,
        };
        let stride = source.stride();
        let mut pen_x = x;
        for byte in text.bytes() {
            if (byte as u32) < atlas::FIRST_CHAR || (byte as u32) >= atlas::LAST_CHAR {
                continue;
            }
            let glyph = &glyphs[(byte as usize) - atlas::FIRST_CHAR as usize];
            if glyph.w != 0 && glyph.h != 0 {
                let mut scratch: Vec<u8> = Vec::new();
                for row in 0..glyph.h as usize {
                    let start =
                        glyph.atlas_y as usize * stride + glyph.atlas_x as usize * 4 + row * stride;
                    let end = start + glyph.w as usize * 4;
                    scratch.extend_from_slice(&source.pixels[start..end]);
                }
                let draw_y = y as i64 + glyph.ink_top as i64;
                if draw_y >= 0 {
                    unsafe {
                        crate::arch::draw_rgba(
                            scratch.as_ptr(),
                            pen_x,
                            draw_y as u32,
                            glyph.w as u32,
                            glyph.h as u32,
                            glyph.w as usize * 4,
                        );
                    }
                }
            }
            pen_x += glyph.advance as u32;
        }
    }

    pub fn draw_labelled(&self, glyphs: &[atlas::Glyph], text: &str, x: u32, y: u32) {
        self.draw(glyphs, text, x + 1, y + 1, Ink::Black);
        self.draw(glyphs, text, x, y, Ink::White);
    }
}
