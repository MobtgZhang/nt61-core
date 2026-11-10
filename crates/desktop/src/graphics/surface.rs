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
        let offset_x = clip.x - requested.x;
        let offset_y = clip.y - requested.y;
        for row in 0..clip.height {
            for column in 0..clip.width {
                let source_x = source_bounds.x + offset_x + column as i32;
                let source_y = source_bounds.y + offset_y + row as i32;
                if let Some(pixel) = source.pixel(source_x as u32, source_y as u32) {
                    self.set_pixel(
                        (clip.x + column as i32) as u32,
                        (clip.y + row as i32) as u32,
                        pixel,
                    );
                }
            }
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
        if x >= self.width || y >= self.height {
            return;
        }
        let alpha = color >> 24;
        let old = self.pixel(x, y).unwrap_or(0);
        let blended = if alpha == 255 {
            color
        } else {
            let inv = 255u32 - alpha;
            (0..3)
                .map(|shift| {
                    let source = (color >> (shift * 8)) & 0xFF;
                    let destination = (old >> (shift * 8)) & 0xFF;
                    ((source * alpha + destination * inv + 128) / 255) << (shift * 8)
                })
                .reduce(|a, b| a | b)
                .unwrap_or(0)
        };
        self.set_pixel(x, y, blended | 0xFF00_0000);
    }

    pub fn blend_rect(&mut self, bounds: Rect, color: u32) {
        for row in 0..bounds.height {
            for column in 0..bounds.width {
                self.blend_pixel(
                    bounds.x.saturating_add(column as i32) as u32,
                    bounds.y.saturating_add(row as i32) as u32,
                    color,
                );
            }
        }
    }

    pub fn blend_rounded_rect(&mut self, bounds: Rect, radius: u32, color: u32) {
        let radius = radius.min(bounds.width / 2).min(bounds.height / 2);
        for row in 0..bounds.height {
            for column in 0..bounds.width {
                let coverage = rounded_coverage(column, row, bounds.width, bounds.height, radius);
                if coverage != 0 {
                    let alpha = ((color >> 24) as u32 * coverage as u32 / 255) as u8;
                    self.blend_pixel(
                        bounds.x.saturating_add(column as i32) as u32,
                        bounds.y.saturating_add(row as i32) as u32,
                        ((alpha as u32) << 24) | (color & 0x00FF_FFFF),
                    );
                }
            }
        }
    }

    pub fn fill_rect(&mut self, x: i32, y: i32, width: u32, height: u32, color: u32) {
        for row in y..y.saturating_add(height as i32) {
            for column in x..x.saturating_add(width as i32) {
                if row >= 0 && column >= 0 {
                    self.set_pixel(column as u32, row as u32, color);
                }
            }
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

        for y in 0..self.height as usize {
            let mut prefix = alloc::vec!([0u32; 4]; self.width as usize);
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
            let mut prefix = alloc::vec!([0u32; 4]; self.height as usize);
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

    pub fn shadow_rect(&mut self, rect: Rect, radius: u32) {
        let mut shadow = self.clone();
        shadow.fill_rect(rect.x, rect.y, rect.width, rect.height, 0xFF00_0000);
        shadow.box_blur(radius);
        for y in 0..self.height {
            for x in 0..self.width {
                let source = (shadow.pixel(x, y).unwrap_or(0) >> 24) & 0xFF;
                if source != 0 {
                    self.blend_pixel(x, y, source << 24);
                }
            }
        }
    }

    pub fn saturate(&mut self, saturation: u32) {
        for pixel in &mut self.pixels {
            let gray =
                ((*pixel >> 16 & 0xFF) * 299 + (*pixel >> 8 & 0xFF) * 587 + (*pixel & 0xFF) * 114)
                    / 1000;
            let mixed = (0..3).map(|shift| {
                let channel = (*pixel >> (shift * 8)) & 0xFF;
                let adjusted = (255 * gray + saturation * channel) / (255 + saturation);
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
