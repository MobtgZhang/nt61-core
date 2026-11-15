//! Raster strokes, shape previews and flood fill.
use super::*;
impl Paint {
    pub(super) fn line(&mut self, a: (i32, i32), b: (i32, i32), color: u32, size: u32) {
        let (mut x, mut y) = a;
        let dx = (b.0 - x).abs();
        let dy = -(b.1 - y).abs();
        let sx = if x < b.0 { 1 } else { -1 };
        let sy = if y < b.1 { 1 } else { -1 };
        let mut e = dx + dy;
        loop {
            self.canvas
                .fill_rect(x - size as i32 / 2, y - size as i32 / 2, size, size, color);
            if (x, y) == b {
                break;
            }
            let twice = 2 * e;
            if twice >= dy {
                e += dy;
                x += sx;
            }
            if twice <= dx {
                e += dx;
                y += sy;
            }
        }
    }
    pub fn stroke(&mut self, x: i32, y: i32) {
        let Some(origin) = self.origin else {
            return;
        };
        let x = x.clamp(0, self.canvas.width() as i32 - 1);
        let y = y.clamp(0, self.canvas.height() as i32 - 1);
        let color = self.stroke_color;
        match self.tool {
            Tool::Fill => {
                if self.last.is_none() {
                    self.flood(x as u32, y as u32);
                }
                self.last = Some((x, y));
            }
            Tool::Picker => {}
            Tool::Line | Tool::Rectangle | Tool::Ellipse => {
                if let Some(base) = self.undo.last() {
                    self.canvas = base.clone();
                }
                let (l, r, t, b) = (
                    origin.0.min(x),
                    origin.0.max(x),
                    origin.1.min(y),
                    origin.1.max(y),
                );
                match self.tool {
                    Tool::Line => self.line(origin, (x, y), color, self.size),
                    Tool::Rectangle => {
                        for (a, b) in [
                            ((l, t), (r, t)),
                            ((r, t), (r, b)),
                            ((r, b), (l, b)),
                            ((l, b), (l, t)),
                        ] {
                            self.line(a, b, color, self.size);
                        }
                    }
                    _ => {
                        let rx = (r - l) as i64;
                        let ry = (b - t) as i64;
                        if rx == 0 || ry == 0 {
                            self.line((l, t), (r, b), color, self.size);
                        } else {
                            // Find the implicit boundary with binary search. Work scales
                            // with the perimeter; large previews must not scan the area.
                            let extent = |axis: i128, other: i128, delta: i128| -> i32 {
                                let (mut lo, mut hi) = (0i128, axis);
                                let limit = axis * axis * (other * other - delta * delta);
                                while lo < hi {
                                    let mid = (lo + hi + 1) / 2;
                                    if mid * mid * other * other <= limit {
                                        lo = mid;
                                    } else {
                                        hi = mid - 1;
                                    }
                                }
                                lo as i32
                            };
                            for yy in t..=b {
                                let xx = extent(rx as i128, ry as i128, (2 * yy - t - b) as i128);
                                for px in [(l + r - xx) / 2, (l + r + xx) / 2] {
                                    self.canvas.fill_rect(
                                        px - self.size as i32 / 2,
                                        yy - self.size as i32 / 2,
                                        self.size,
                                        self.size,
                                        color,
                                    );
                                }
                            }
                            for xx in l..=r {
                                let yy = extent(ry as i128, rx as i128, (2 * xx - l - r) as i128);
                                for py in [(t + b - yy) / 2, (t + b + yy) / 2] {
                                    self.canvas.fill_rect(
                                        xx - self.size as i32 / 2,
                                        py - self.size as i32 / 2,
                                        self.size,
                                        self.size,
                                        color,
                                    );
                                }
                            }
                        }
                    }
                }
                self.last = Some((x, y));
            }
            _ => {
                let size = if self.tool == Tool::Eraser {
                    self.size.max(9)
                } else if self.tool == Tool::Brush {
                    self.size.max(6)
                } else {
                    self.size
                };
                let color = if self.tool == Tool::Eraser {
                    self.background
                } else {
                    color
                };
                self.line(self.last.unwrap_or((x, y)), (x, y), color, size);
                self.last = Some((x, y));
            }
        }
        self.dirty = true;
    }
    pub(super) fn flood(&mut self, x: u32, y: u32) {
        let old = self.canvas.pixel(x, y).unwrap();
        if old == self.stroke_color {
            return;
        }
        let (w, h) = (self.canvas.width(), self.canvas.height());
        let mut stack = vec![(x, y)];
        self.canvas.set_pixel(x, y, self.stroke_color);
        while let Some((x, y)) = stack.pop() {
            for (nx, ny) in [
                (x.wrapping_sub(1), y),
                (x + 1, y),
                (x, y.wrapping_sub(1)),
                (x, y + 1),
            ] {
                if nx < w && ny < h && self.canvas.pixel(nx, ny) == Some(old) {
                    self.canvas.set_pixel(nx, ny, self.stroke_color);
                    stack.push((nx, ny));
                }
            }
        }
    }
}
