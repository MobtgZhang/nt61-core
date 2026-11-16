//! Glyph outlines and coverage from the installed SFNT faces. Native POINTFX
//! streams retain curves; quadratic curves can also be exported as cubics.
//! Raster coverage uses the shared local fontdue engine. TrueType bytecode
//! hinting and Windows font substitution are not implemented by that engine.
use super::{dc, RealizedFont, GDI_ERROR};
use crate::arch::common::syscall_frame::SyscallFrame;
use crate::ke::sync::Spinlock;
use crate::mm::user_copy::{copy_to_user, probe_user_write, read_user_object};
use alloc::vec::Vec;
use nt61_fonts::{FontFace, OutlineBuilder};

const MAX_OUTLINE_BYTES: usize = 4 * 1024 * 1024;
const MAX_BITMAP_BYTES: usize = 8 * 1024 * 1024;
static UI_RASTERIZER: Spinlock<Option<FontFace>> = Spinlock::new(None);
static MONO_RASTERIZER: Spinlock<Option<FontFace>> = Spinlock::new(None);

type Point = [f64; 2];

#[derive(Clone, Copy)]
struct Matrix([f64; 4]);

impl Matrix {
    fn apply(self, [x, y]: Point) -> Point {
        let [a, b, c, d] = self.0;
        [a * x + c * y, b * x + d * y]
    }

    fn then(self, next: Self) -> Self {
        let first = next.apply([self.0[0], self.0[1]]);
        let second = next.apply([self.0[2], self.0[3]]);
        Self([first[0], first[1], second[0], second[1]])
    }

    fn inverse(self) -> Option<Self> {
        let [a, b, c, d] = self.0;
        let determinant = a * d - b * c;
        if !determinant.is_finite() || determinant == 0.0 {
            return None;
        }
        Some(Self([d / determinant, -b / determinant, -c / determinant, a / determinant]))
    }
}

fn fixed(value: f64) -> Option<i32> {
    let value = libm::round(value * 65536.0);
    (value.is_finite() && value >= i32::MIN as f64 && value <= i32::MAX as f64)
        .then_some(value as i32)
}

fn append_point(bytes: &mut Vec<u8>, point: Point) -> Option<()> {
    bytes.extend_from_slice(&fixed(point[0])?.to_le_bytes());
    bytes.extend_from_slice(&fixed(point[1])?.to_le_bytes());
    Some(())
}

struct Contours {
    bytes: Vec<u8>,
    matrix: Matrix,
    bezier: bool,
    failed: bool,
    header: Option<usize>,
    start: Point,
    current: Point,
}

impl Contours {
    fn reserve(&mut self, amount: usize) -> Option<()> {
        if self.bytes.len().checked_add(amount)? > MAX_OUTLINE_BYTES {
            return None;
        }
        self.bytes.try_reserve(amount).ok()
    }

    fn curve(&mut self, kind: u16, points: &[Point]) -> Option<()> {
        self.reserve(4 + points.len() * 8)?;
        self.bytes.extend_from_slice(&kind.to_le_bytes());
        self.bytes.extend_from_slice(&(points.len() as u16).to_le_bytes());
        for point in points {
            append_point(&mut self.bytes, self.matrix.apply(*point))?;
        }
        self.current = *points.last()?;
        Some(())
    }

    fn finish_contour(&mut self) -> Option<()> {
        let Some(header) = self.header else { return Some(()) };
        if self.current != self.start {
            self.curve(1, &[self.start])?;
        }
        let size = u32::try_from(self.bytes.len() - header).ok()?;
        self.bytes[header..header + 4].copy_from_slice(&size.to_le_bytes());
        self.header = None;
        Some(())
    }
}

impl OutlineBuilder for Contours {
    fn move_to(&mut self, x: f32, y: f32) {
        let result = (|| {
            self.finish_contour()?;
            self.reserve(16)?;
            self.header = Some(self.bytes.len());
            self.bytes.extend_from_slice(&0u32.to_le_bytes());
            self.bytes.extend_from_slice(&24u32.to_le_bytes()); // TT_POLYGON_TYPE
            self.start = [f64::from(x), f64::from(y)];
            self.current = self.start;
            append_point(&mut self.bytes, self.matrix.apply(self.start))
        })();
        self.failed |= result.is_none();
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.failed |= self.curve(1, &[[f64::from(x), f64::from(y)]]).is_none();
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let control = [f64::from(x1), f64::from(y1)];
        let end = [f64::from(x), f64::from(y)];
        let result = if self.bezier {
            let first = core::array::from_fn(|i| self.current[i] + (control[i] - self.current[i]) * (2.0 / 3.0));
            let second = core::array::from_fn(|i| end[i] + (control[i] - end[i]) * (2.0 / 3.0));
            self.curve(3, &[first, second, end])
        } else {
            self.curve(2, &[control, end])
        };
        self.failed |= result.is_none();
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.failed |= self.curve(3, &[
            [f64::from(x1), f64::from(y1)],
            [f64::from(x2), f64::from(y2)],
            [f64::from(x), f64::from(y)],
        ]).is_none();
    }

    fn close(&mut self) {
        self.failed |= self.finish_contour().is_none();
    }
}

fn matrix(font: &RealizedFont, dc: &dc::DcObject, mat: [i32; 4], ignore_rotation: bool) -> Option<Matrix> {
    let scale = Matrix([
        font.horizontal_numerator as f64 / font.horizontal_denominator as f64,
        0.0, 0.0, font.em_height as f64 / f64::from(font.description.units_per_em),
    ]);
    let angle = if ignore_rotation { 0 } else if dc.graphics_mode == 1 {
        font.requested.escapement
    } else { font.requested.orientation };
    let radians = f64::from(angle % 3600) * core::f64::consts::PI / 1800.0;
    let rotation = Matrix([libm::cos(radians), libm::sin(radians), -libm::sin(radians), libm::cos(radians)]);
    let world = super::super::transforms::world_to_device(dc)?;
    // GDI logical/device Y points down; glyph coordinates point up. Origins
    // are deliberately excluded: an outline is relative to the baseline.
    let device = Matrix([f64::from(world.m11), -f64::from(world.m12),
        -f64::from(world.m21), f64::from(world.m22)]);
    let caller = Matrix(mat.map(|value| f64::from(value) / 65536.0));
    let result = scale.then(rotation).then(device).then(caller);
    result.0.iter().all(|value| value.is_finite()).then_some(result)
}

pub(super) struct Glyph {
    pub(super) metrics: [u8; 20],
    pub(super) bounds: [i32; 4], // left, bottom, right, top, in upward device coordinates
    outline: Vec<u8>,
    matrix: Matrix,
    font_data: &'static [u8],
    monospace: bool,
    index: u16,
    em_height: i64,
}

fn realize(hdc: u64, ch: u16, format: u32, mat: [i32; 4], ignore_rotation: bool) -> Option<Glyph> {
    let dc = dc::get_dc(hdc)?;
    let font = RealizedFont::from_dc(&dc)?;
    realize_font(&font, &dc, ch, format, mat, ignore_rotation)
}

pub(super) fn realize_font(font: &RealizedFont, dc: &dc::DcObject, ch: u16,
    format: u32, mat: [i32; 4], ignore_rotation: bool) -> Option<Glyph>
{
    let matrix = matrix(&font, &dc, mat, ignore_rotation)?;
    let index = font.glyph(ch, format & 0x80 != 0);
    let advance = matrix.apply([f64::from(font.font.glyph_metrics(index)?.advance), 0.0]);
    let mut contours = Contours { bytes: Vec::new(), matrix, bezier: format & 0xf == 3,
        failed: false, header: None, start: [0.0; 2], current: [0.0; 2] };
    let bounds = font.font.outline(index, &mut contours);
    contours.finish_contour()?;
    if contours.failed { return None; }
    let mut minimum = [0.0; 2];
    let mut maximum = [0.0; 2];
    if let Some(bounds) = bounds {
        let corners = [
            [f64::from(bounds.x_min), f64::from(bounds.y_min)],
            [f64::from(bounds.x_min), f64::from(bounds.y_max)],
            [f64::from(bounds.x_max), f64::from(bounds.y_min)],
            [f64::from(bounds.x_max), f64::from(bounds.y_max)],
        ].map(|point| matrix.apply(point));
        minimum = corners[0];
        maximum = corners[0];
        for point in corners {
            for axis in 0..2 {
                minimum[axis] = minimum[axis].min(point[axis]);
                maximum[axis] = maximum[axis].max(point[axis]);
            }
        }
    }
    let integer = |value: f64| -> Option<i32> {
        (value.is_finite() && value >= i32::MIN as f64 && value <= i32::MAX as f64).then_some(value as i32)
    };
    let left = integer(libm::floor(minimum[0]))?;
    let bottom = integer(libm::floor(minimum[1]))?;
    let right = integer(libm::ceil(maximum[0]))?;
    let top = integer(libm::ceil(maximum[1]))?;
    let mut metrics = [0u8; 20];
    for (slot, value) in metrics[..16].chunks_exact_mut(4).zip([
        right.checked_sub(left)?, top.checked_sub(bottom)?, left, top,
    ]) {
        slot.copy_from_slice(&value.to_le_bytes());
    }
    metrics[16..18].copy_from_slice(&i16::try_from(integer(libm::round(advance[0]))?).ok()?.to_le_bytes());
    metrics[18..20].copy_from_slice(&i16::try_from(integer(libm::round(-advance[1]))?).ok()?.to_le_bytes());
    Some(Glyph { metrics, bounds: [left, bottom, right, top], outline: contours.bytes,
        matrix, font_data: font.font.data(), monospace: font.monospace, index,
        em_height: font.em_height })
}

impl Glyph {
    pub(super) fn bitmap_layout(&self, format: u32) -> Option<(usize, usize)> {
        let [left, bottom, right, top] = self.bounds;
        let width = usize::try_from(right.checked_sub(left)?).ok()?;
        let height = usize::try_from(top.checked_sub(bottom)?).ok()?;
        let stride = if format == 1 { width.checked_add(31)? / 32 * 4 }
            else { width.checked_add(3)? / 4 * 4 };
        let size = stride.checked_mul(height)?;
        (size <= MAX_BITMAP_BYTES).then_some((stride, size))
    }

    pub(super) fn bitmap(&self, format: u32, stride: usize, size: usize) -> Option<Vec<u8>> {
        let mut output = Vec::new();
        output.try_reserve_exact(size).ok()?;
        output.resize(size, 0);
        if size == 0 { return Some(output); }
        let inverse = self.matrix.inverse()?;
        // Bound fontdue's source bitmap before calling its allocator. Coverage
        // is supersampled to preserve detail through rotation and shearing.
        let em_size = self.em_height.checked_mul(4)?.max(64);
        if em_size > 2048 { return None; }
        let cache = if self.monospace { &MONO_RASTERIZER } else { &UI_RASTERIZER };
        let source = {
            let mut rasterizer = cache.lock();
            if rasterizer.is_none() {
                *rasterizer = Some(FontFace::from_bytes(self.font_data).ok()?);
            }
            rasterizer.as_ref()?.rasterize_indexed(self.index, em_size as f32)?
        };
        let units = f64::from(nt61_fonts::SfntFont::parse(self.font_data)?.description()?.units_per_em);
        let scale = em_size as f64 / units;
        let [left, bottom, right, top] = self.bounds;
        let width = (right - left) as usize;
        let height = (top - bottom) as usize;
        let maximum = match format { 1 => 1, 4 => 4, 5 => 16, 6 => 64, _ => return None };
        for row in 0..height {
            for column in 0..width {
                let mut coverage = 0u32;
                for sy in 0..4 {
                    for sx in 0..4 {
                        let point = inverse.apply([
                            f64::from(left) + column as f64 + (f64::from(sx) + 0.5) / 4.0,
                            f64::from(top) - row as f64 - (f64::from(sy) + 0.5) / 4.0,
                        ]);
                        // An almost singular transform can map a finite output
                        // sample far beyond the source. Bound floats before
                        // converting: saturating float casts followed by signed
                        // subtraction can overflow even for a tiny bitmap.
                        let x = libm::floor(point[0] * scale) - f64::from(source.metrics.xmin);
                        let y = libm::floor(point[1] * scale) - f64::from(source.metrics.ymin);
                        if x >= 0.0 && x < source.metrics.width as f64
                            && y >= 0.0 && y < source.metrics.height as f64
                        {
                            let row = source.metrics.height - 1 - y as usize;
                            coverage += u32::from(source.coverage_at(x as usize, row).unwrap_or(0));
                        }
                    }
                }
                let value = ((coverage * maximum + 2040) / 4080) as u8;
                if format == 1 {
                    if value != 0 { output[row * stride + column / 8] |= 0x80 >> (column % 8); }
                } else { output[row * stride + column] = value; }
            }
        }
        Some(output)
    }
}

pub extern "C" fn nt_gdi_get_glyph_outline(frame: &SyscallFrame) -> u64 {
    let result = (|| {
        let format = frame.args[2] as u32;
        if format & !0x18f != 0 || format & 0xf > 6 { return None; }
        let base = format & 0xf;
        // FIXED is a signed 16.16 number with the fractional WORD first.
        // Capture before output so MAT2 and caller buffers may overlap.
        let mat: [i32; 4] = read_user_object(frame.args[6]).ok()?;
        probe_user_write(frame.args[3], 20).ok()?;
        let glyph = realize(frame.args[0], frame.args[1] as u16, format, mat, frame.args[7] != 0)?;
        let (stride, required) = match base {
            0 => (0, 0),
            2 | 3 => (0, glyph.outline.len()),
            _ => glyph.bitmap_layout(base)?,
        };
        let requested = frame.args[5] != 0 && frame.args[4] as u32 != 0 && base != 0;
        if requested {
            if (frame.args[4] as u32 as usize) < required { return None; }
            if required != 0 { probe_user_write(frame.args[5], required).ok()?; }
        }
        let bitmap = if requested && base != 2 && base != 3 {
            Some(glyph.bitmap(base, stride, required)?)
        } else { None };
        if requested && required != 0 {
            copy_to_user(frame.args[5], bitmap.as_deref().unwrap_or(&glyph.outline)).ok()?;
        }
        copy_to_user(frame.args[3], &glyph.metrics).ok()?;
        Some(required as u64)
    })();
    result.unwrap_or(GDI_ERROR)
}
