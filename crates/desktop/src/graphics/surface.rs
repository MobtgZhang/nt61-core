use alloc::vec::Vec;

use super::clip::Rect;

const MAX_SURFACE_PIXELS: usize = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgba(pub u8, pub u8, pub u8, pub u8);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Surface {
    width: u32,
    height: u32,
    pixels: Vec<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipped_span_drawing_matches_scalar_compositing() {
        let mut seed = 0x782E6A19u32;
        let mut next = || { seed = seed.wrapping_mul(1664525).wrapping_add(1013904223); seed };
        for _ in 0..400 {
            let mut original = Surface::new(31, 23);
            for pixel in &mut original.pixels { *pixel = next(); }
            let bounds = Rect::new((next() % 70) as i32 - 35, (next() % 60) as i32 - 30,
                next() % 80, next() % 70);
            let radius = (next() % 50).min(bounds.width / 2).min(bounds.height / 2);
            let color = [0x00264187, 0x01334568, 0x8046E768, 0xFFA98766][(next() % 4) as usize];
            for rounded in [false, true] {
                let mut expected = original.clone(); let mut actual = original.clone();
                for row in 0..bounds.height { for col in 0..bounds.width {
                    let coverage = if rounded { rounded_coverage(col, row, bounds.width, bounds.height, radius) } else {255};
                    if coverage == 0 { continue; }
                    let x = (bounds.x + col as i32) as u32; let y = (bounds.y + row as i32) as u32;
                    if let Some(old) = expected.pixel(x,y) {
                        let alpha = (color >> 24) * coverage / 255;
                        let mut blended = 0xFF00_0000;
                        for shift in [0,8,16] { blended |= (((color >> shift & 255) * alpha
                            + (old >> shift & 255) * (255-alpha) + 128) / 255) << shift; }
                        expected.set_pixel(x,y,blended);
                    }
                }}
                if rounded { actual.blend_rounded_rect(bounds,radius,color); }
                else { actual.blend_rect(bounds,color); }
                assert_eq!(actual,expected,"bounds={bounds:?}, radius={radius}, rounded={rounded}");
            }
            let mut expected = original.clone(); let mut actual = original;
            for row in 0..bounds.height { for col in 0..bounds.width {
                expected.set_pixel((bounds.x + col as i32) as u32,(bounds.y + row as i32) as u32,color);
            }}
            actual.fill_rect(bounds.x,bounds.y,bounds.width,bounds.height,color);
            assert_eq!(actual,expected);
        }
    }

    #[test]
    fn clipped_bulk_copy_preserves_pixels_outside_source() {
        let mut source = Surface::new(9,7);
        for (i,pixel) in source.pixels.iter_mut().enumerate() { *pixel = i as u32; }
        for sx in [-12,-3,0,6,14] { for sy in [-9,-2,0,5,12] {
            for dx in [-12,-2,0,7,19] { for dy in [-10,-1,0,5,19] {
                let mut expected = Surface::new(13,11); expected.pixels.fill(0x789ABCDE);
                let mut actual = expected.clone();
                for row in 0..15 { for col in 0..17 {
                    if let Some(pixel) = source.pixel((sx+col) as u32,(sy+row) as u32) {
                        expected.set_pixel((dx+col) as u32,(dy+row) as u32,pixel);
                    }
                }}
                actual.copy_region(&source,Rect::new(sx,sy,17,15),dx,dy);
                assert_eq!(actual,expected);
            }}
        }}
    }

    #[test]
    fn popup_shadow_fades_outside_bounds_and_clips_at_screen_edge() {
        let mut surface = Surface::new(32, 24);
        surface.fill_rect(0, 0, 32, 24, 0xFFFFFFFF);
        surface.shadow_rect(Rect::new(8, 4, 12, 12), 4);
        let far = surface.pixel(4, 10).unwrap() & 255;
        let near = surface.pixel(7, 10).unwrap() & 255;
        assert!(
            near < far && near > 128,
            "shadow must soften, not form an opaque frame"
        );
        assert_eq!(surface.pixel(3, 10), Some(0xFFFFFFFF));
        assert_eq!(surface.pixel(8, 10), Some(0xFFFFFFFF));
        let mut clipped = Surface::new(32, 24);
        clipped.fill_rect(0, 0, 32, 24, 0xFFFFFFFF);
        clipped.shadow_rect(Rect::new(-4, 4, 12, 12), 4);
        assert_eq!(clipped.pixel(8, 10), surface.pixel(20, 10));
    }

    #[test]
    fn glass_saturation_preserves_color_at_full_strength() {
        let mut surface = Surface::new(1, 1);
        surface.set_pixel(0, 0, 0xFF2080E0);
        surface.saturate(255);
        assert_eq!(surface.pixel(0, 0), Some(0xFF2080E0));
        surface.saturate(0);
        let gray = surface.pixel(0, 0).unwrap();
        assert_eq!(gray & 255, (gray >> 8) & 255);
        assert_eq!(gray & 255, (gray >> 16) & 255);
    }

    #[test]
    fn try_new_rejects_overflowing_dimensions() {
        assert!(Surface::try_new(u32::MAX, 2).is_none());
    }

    #[test]
    fn try_new_rejects_unbounded_allocations() {
        assert!(Surface::try_new(17, 1024 * 1024).is_none());
    }
}

impl Surface {
    pub fn new(width: u32, height: u32) -> Self {
        Self::try_new(width, height).expect("surface allocation must be valid")
    }

    pub fn try_new(width: u32, height: u32) -> Option<Self> {
        let pixel_count = (width as usize).checked_mul(height as usize)?;
        if pixel_count > MAX_SURFACE_PIXELS {
            return None;
        }
        Some(Self {
            width,
            height,
            pixels: alloc::vec![0; pixel_count],
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn pixels(&self) -> &[u32] {
        &self.pixels
    }

    /// Replace the contents with an equally sized surface without another
    /// allocation. Shells can reuse this between frames to avoid heap churn.
    pub fn copy_from(&mut self, source: &Surface) {
        if self.width == source.width && self.height == source.height {
            self.pixels.copy_from_slice(source.pixels());
        }
    }

    /// Copy a rectangular source region to `(x, y)` while clipping both
    /// surfaces. The source and destination may be different surfaces.
    pub fn copy_region(&mut self, source: &Surface, source_bounds: Rect, x: i32, y: i32) {
        let destination_bounds = Rect::new(0, 0, self.width, self.height);
        let requested = Rect::new(x, y, source_bounds.width, source_bounds.height);
        let Some(clip) = destination_bounds.intersection(&requested) else {
            return;
        };
        let source_x = i64::from(source_bounds.x) + i64::from(clip.x) - i64::from(x);
        let source_y = i64::from(source_bounds.y) + i64::from(clip.y) - i64::from(y);
        let left_skip = (-source_x).max(0).min(i64::from(clip.width));
        let count = (i64::from(source.width) - source_x).min(i64::from(clip.width)) - left_skip;
        if count <= 0 { return; }
        for row in 0..clip.height {
            let sy = source_y + i64::from(row);
            if sy < 0 || sy >= i64::from(source.height) { continue; }
            let src = sy as usize * source.width as usize + (source_x + left_skip) as usize;
            let dst = (clip.y as usize + row as usize) * self.width as usize
                + clip.x as usize + left_skip as usize;
            self.pixels[dst..dst + count as usize]
                .copy_from_slice(&source.pixels[src..src + count as usize]);
        }
    }

    pub fn pixel(&self, x: u32, y: u32) -> Option<u32> {
        if x >= self.width || y >= self.height {
            None
        } else {
            Some(self.pixels[y as usize * self.width as usize + x as usize])
        }
    }

    pub fn set_pixel(&mut self, x: u32, y: u32, color: u32) {
        if x < self.width && y < self.height {
            self.pixels[y as usize * self.width as usize + x as usize] = color;
        }
    }

    pub fn blend_pixel(&mut self, x: u32, y: u32, color: u32) {
        if x < self.width && y < self.height {
            let pixel = &mut self.pixels[y as usize * self.width as usize + x as usize];
            *pixel = composite(*pixel, color);
        }
    }

    pub fn blend_rect(&mut self, bounds: Rect, color: u32) {
        let Some(clip) = self.clip_rect(bounds) else { return; };
        let stride = self.width as usize;
        for y in clip.y as usize..clip.y as usize + clip.height as usize {
            let start = y * stride + clip.x as usize;
            blend_span(&mut self.pixels[start..start + clip.width as usize], color);
        }
    }

    pub fn blend_rounded_rect(&mut self, bounds: Rect, radius: u32, color: u32) {
        let Some(clip) = self.clip_rect(bounds) else { return; };
        let radius = radius.min(bounds.width / 2).min(bounds.height / 2);
        let first = (i64::from(clip.x) - i64::from(bounds.x)) as u32;
        let last = first + clip.width;
        let middle_start = radius.max(first).min(last);
        let middle_end = (bounds.width - radius).min(last).max(middle_start);
        let stride = self.width as usize;
        for y in clip.y as u32..clip.y as u32 + clip.height {
            let row = (i64::from(y) - i64::from(bounds.y)) as u32;
            let start = y as usize * stride + clip.x as usize;
            let pixels = &mut self.pixels[start..start + clip.width as usize];
            // Coverage is constant across the central span, including the
            // antialiased top/bottom rows. Only the curved edges need per-pixel math.
            for column in (first..middle_start).chain(middle_end..last) {
                let coverage = rounded_coverage(column, row, bounds.width, bounds.height, radius);
                if coverage != 0 {
                    let source = (((color >> 24) * coverage / 255) << 24) | (color & 0x00FF_FFFF);
                    let pixel = &mut pixels[(column - first) as usize];
                    *pixel = composite(*pixel, source);
                }
            }
            if middle_start < middle_end {
                let coverage = rounded_coverage(middle_start, row, bounds.width, bounds.height, radius);
                if coverage != 0 {
                    let source = (((color >> 24) * coverage / 255) << 24) | (color & 0x00FF_FFFF);
                    blend_span(&mut pixels[(middle_start - first) as usize..(middle_end - first) as usize], source);
                }
            }
        }
    }

    fn clip_rect(&self, bounds: Rect) -> Option<Rect> {
        // Wide arithmetic also bounds work for hostile or mostly off-screen rectangles.
        let left = i64::from(bounds.x).max(0);
        let top = i64::from(bounds.y).max(0);
        let right = (i64::from(bounds.x) + i64::from(bounds.width)).min(i64::from(self.width));
        let bottom = (i64::from(bounds.y) + i64::from(bounds.height)).min(i64::from(self.height));
        (left < right && top < bottom).then(|| Rect::new(left as i32, top as i32,
            (right - left) as u32, (bottom - top) as u32))
    }

    /// Alpha-composite a rounded outline without changing the enclosed pixels.
    pub fn stroke_rounded_rect(&mut self, bounds: Rect, radius: u32, thickness: u32, color: u32) {
        let thickness = thickness.min(bounds.width / 2).min(bounds.height / 2);
        if thickness == 0 { return; }
        let radius = radius.min(bounds.width / 2).min(bounds.height / 2);
        let inner_width = bounds.width - thickness * 2;
        let inner_height = bounds.height - thickness * 2;
        for row in 0..bounds.height {
            let corner = radius.max(thickness);
            let (left_end, right_start) = if row >= corner && row < bounds.height - corner {
                (thickness, bounds.width - thickness)
            } else { (bounds.width, bounds.width) };
            for column in (0..left_end).chain(right_start..bounds.width) {
                let outer = rounded_coverage(column, row, bounds.width, bounds.height, radius);
                let inner = if column >= thickness && row >= thickness &&
                    column < bounds.width - thickness && row < bounds.height - thickness {
                    rounded_coverage(column - thickness, row - thickness,
                        inner_width, inner_height, radius.saturating_sub(thickness))
                } else { 0 };
                let coverage = outer.saturating_sub(inner);
                if coverage != 0 {
                    let alpha = (color >> 24) * coverage / 255;
                    self.blend_pixel(bounds.x.saturating_add(column as i32) as u32,
                        bounds.y.saturating_add(row as i32) as u32,
                        (alpha << 24) | (color & 0x00FF_FFFF));
                }
            }
        }
    }

    pub fn fill_rect(&mut self, x: i32, y: i32, width: u32, height: u32, color: u32) {
        let Some(clip) = self.clip_rect(Rect::new(x, y, width, height)) else { return; };
        let stride = self.width as usize;
        for y in clip.y as usize..clip.y as usize + clip.height as usize {
            let start = y * stride + clip.x as usize;
            self.pixels[start..start + clip.width as usize].fill(color);
        }
    }

    pub fn vertical_gradient(&mut self, top: u32, bottom: u32) {
        let height = self.height.max(1);
        for (row, scale) in (0..self.height).map(|row| {
            let denominator = height.saturating_sub(1).max(1);
            (row, (row as u64 * 255) / denominator as u64)
        }) {
            self.fill_rect(
                0,
                row as i32,
                self.width,
                1,
                lerp_color(top, bottom, scale as u32),
            );
        }
    }

    pub fn rounded_rect(&mut self, rect: Rect, radius: u32, color: u32) {
        for row in 0..rect.height {
            for column in 0..rect.width {
                let x = rect.x + column as i32;
                let y = rect.y + row as i32;
                let corner_distance = if column < radius && row < radius {
                    distance_to_corner(column, row, radius)
                } else if column >= rect.width.saturating_sub(radius) && row < radius {
                    distance_to_corner(rect.width - column - 1, row, radius)
                } else if column < radius && row >= rect.height.saturating_sub(radius) {
                    distance_to_corner(column, rect.height - row - 1, radius)
                } else if column >= rect.width.saturating_sub(radius)
                    && row >= rect.height.saturating_sub(radius)
                {
                    distance_to_corner(rect.width - column - 1, rect.height - row - 1, radius)
                } else {
                    radius * radius
                };
                if corner_distance <= radius * radius {
                    self.set_pixel(x as u32, y as u32, color);
                }
            }
        }
    }

    pub fn box_blur(&mut self, radius: u32) {
        if radius == 0 || self.pixels.is_empty() {
            return;
        }
        let source = self.clone();
        let radius_usize = radius as usize;
        let mut prefix = alloc::vec![[0u32; 4]; self.width.max(self.height) as usize];

        for y in 0..self.height as usize {
            let mut running = [0u32; 4];
            for x in 0..self.width as usize {
                let pixel = source.pixel(x as u32, y as u32).unwrap_or(0);
                for channel in 0..4 {
                    running[channel] += (pixel >> (channel * 8)) & 0xFF;
                }
                prefix[x] = running;
            }
            for x in 0..self.width as usize {
                let left = x.saturating_sub(radius_usize);
                let right = (x + radius_usize).min(self.width as usize - 1);
                let mut sums = prefix[right];
                if left > 0 {
                    for channel in 0..4 {
                        sums[channel] -= prefix[left - 1][channel];
                    }
                }
                let count = (right - left + 1) as u32;
                let pixel = (0..4)
                    .map(|channel| (sums[channel] / count) << (channel * 8))
                    .fold(0, |accumulator, channel| accumulator | channel);
                self.set_pixel(x as u32, y as u32, pixel);
            }
        }

        let source = self.clone();
        for x in 0..self.width as usize {
            let mut running = [0u32; 4];
            for y in 0..self.height as usize {
                let pixel = source.pixel(x as u32, y as u32).unwrap_or(0);
                for channel in 0..4 {
                    running[channel] += (pixel >> (channel * 8)) & 0xFF;
                }
                prefix[y] = running;
            }
            for y in 0..self.height as usize {
                let left = y.saturating_sub(radius_usize);
                let right = (y + radius_usize).min(self.height as usize - 1);
                let mut sums = prefix[right];
                if left > 0 {
                    for channel in 0..4 {
                        sums[channel] -= prefix[left - 1][channel];
                    }
                }
                let count = (right - left + 1) as u32;
                let pixel = (0..4)
                    .map(|channel| (sums[channel] / count) << (channel * 8))
                    .fold(0, |accumulator, channel| accumulator | channel);
                self.set_pixel(x as u32, y as u32, pixel);
            }
        }
    }

    /// Box-filter the rectangular alpha mask analytically. Only the visible
    /// halo is touched; no popup-sized allocation or full-surface blur is needed.
    pub fn shadow_rect(&mut self, rect: Rect, radius: u32) {
        if radius == 0 || rect.width == 0 || rect.height == 0 {
            return;
        }
        let r = i64::from(radius);
        let left = i64::from(rect.x);
        let top = i64::from(rect.y);
        let right = left + i64::from(rect.width);
        let bottom = top + i64::from(rect.height);
        let kernel = 2 * r + 1;
        for y in (top - r).max(0)..(bottom + r).min(i64::from(self.height)) {
            let vertical = ((y + r + 1).min(bottom) - (y - r).max(top)).max(0);
            for x in (left - r).max(0)..(right + r).min(i64::from(self.width)) {
                if x >= left && x < right && y >= top && y < bottom {
                    continue;
                }
                let horizontal = ((x + r + 1).min(right) - (x - r).max(left)).max(0);
                let alpha =
                    (140 * (horizontal as u64) * (vertical as u64) / (kernel as u64).pow(2)) as u32;
                if alpha != 0 {
                    self.blend_pixel(x as u32, y as u32, alpha << 24);
                }
            }
        }
    }

    /// Interpolate from grayscale (0) to the original color (255).
    pub fn saturate(&mut self, saturation: u32) {
        let saturation = saturation.min(255);
        for pixel in &mut self.pixels {
            let gray =
                ((*pixel >> 16 & 0xFF) * 299 + (*pixel >> 8 & 0xFF) * 587 + (*pixel & 0xFF) * 114)
                    / 1000;
            let mixed = (0..3).map(|shift| {
                let channel = (*pixel >> (shift * 8)) & 0xFF;
                let adjusted = (gray * (255 - saturation) + saturation * channel + 127) / 255;
                adjusted.min(255) << (shift * 8)
            });
            *pixel = mixed.reduce(|a, b| a | b).unwrap_or(0) | 0xFF00_0000;
        }
    }

    pub fn tint(&mut self, tint: u32, strength: u32) {
        for pixel in &mut self.pixels {
            let mixed = (0..3).map(|shift| {
                let source = (*pixel >> (shift * 8)) & 0xFF;
                let overlay = (tint >> (shift * 8)) & 0xFF;
                ((source * (255 - strength) + overlay * strength + 128) / 255) << (shift * 8)
            });
            *pixel = mixed.reduce(|a, b| a | b).unwrap_or(0) | 0xFF00_0000;
        }
    }
}

#[inline]
fn composite(old: u32, color: u32) -> u32 {
    let alpha = color >> 24;
    if alpha == 255 { return color; }
    let inv = 255 - alpha;
    let channel = |shift: u32| (((color >> shift & 255) * alpha
        + (old >> shift & 255) * inv + 128) / 255) << shift;
    0xFF00_0000 | channel(0) | channel(8) | channel(16)
}

fn blend_span(pixels: &mut [u32], color: u32) {
    if color >> 24 == 255 { pixels.fill(color); }
    else { for pixel in pixels { *pixel = composite(*pixel, color); } }
}

pub fn lerp_color(top: u32, bottom: u32, scale: u32) -> u32 {
    let channels = (0..3)
        .map(|shift| {
            let top = (top >> (shift * 8)) & 0xFF;
            let bottom = (bottom >> (shift * 8)) & 0xFF;
            ((top * (255 - scale) + bottom * scale + 128) / 255) << (shift * 8)
        })
        .fold(0u32, |accumulator, value| accumulator | value);
    channels | 0xFF00_0000
}

fn distance_to_corner(x: u32, y: u32, radius: u32) -> u32 {
    let x = x.saturating_sub(radius) as u64;
    let y = y.saturating_sub(radius) as u64;
    (x * x + y * y) as u32
}

fn rounded_coverage(column: u32, row: u32, width: u32, height: u32, radius: u32) -> u32 {
    if radius == 0 {
        return 255;
    }
    let right = width.saturating_sub(radius);
    let bottom = height.saturating_sub(radius);
    let dx = if column < radius {
        radius as i64 - column as i64
    } else if column >= right {
        column as i64 - (right.saturating_sub(1)) as i64
    } else {
        0
    };
    let dy = if row < radius {
        radius as i64 - row as i64
    } else if row >= bottom {
        row as i64 - (bottom.saturating_sub(1)) as i64
    } else {
        0
    };
    let distance_squared = (dx * dx + dy * dy) as u32;
    let inner_squared = radius.saturating_sub(1).pow(2);
    if distance_squared <= inner_squared {
        255
    } else if distance_squared <= radius * radius {
        160
    } else {
        0
    }
}
