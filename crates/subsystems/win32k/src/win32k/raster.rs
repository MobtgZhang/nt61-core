//! Clipped raster operations on 32-bit surfaces.
//!
//! Native callers retain the DC table guard throughout rendering. Surface
//! bindings are borrowed from the display subsystem. Source samples are captured
//! before any destination write, including overlapping copies of the same DC.

extern crate alloc;

use super::clipping;
use super::dc::{DcObject, PointL};
use super::objects::{self, GdiBrush, GdiObjectType, Rect};
use super::region_geometry::{Region, RegionCombineMode};
use super::surface::{self, GdiSurface, PIXEL_FORMAT_32BPP, PIXEL_FORMAT_32BPP_ARGB};
use alloc::vec::Vec;

const MAX_CAPTURE_PIXELS: usize = 16 * 1024 * 1024;

#[path = "raster/extended.rs"]
pub mod extended;
#[path = "raster/stretch.rs"]
pub mod stretch;
#[path = "raster/alpha.rs"]
pub mod alpha;
#[path = "raster/mask.rs"]
pub mod mask;
#[path = "raster/parallelogram.rs"]
pub mod parallelogram;
#[path = "raster/dib.rs"]
pub mod dib;
#[path = "raster/pattern.rs"]
pub mod pattern;

fn bound_surface(dc: &DcObject) -> Option<&GdiSurface> {
    let pointer = if dc.surface == 0 {
        surface::get_primary_surface()
    } else {
        dc.surface as *mut GdiSurface
    };
    // The DC guard serializes binding changes; the display subsystem owns it.
    let surface = unsafe { pointer.as_ref()? };
    if !matches!(surface.format, PIXEL_FORMAT_32BPP | PIXEL_FORMAT_32BPP_ARGB)
        || surface.bits.is_null()
        || surface.width <= 0
        || surface.height <= 0
        || surface.pitch <= 0
        || surface.width > surface.pitch / 4
    {
        return None;
    }
    Some(surface)
}

fn pixel_pointer(surface: &GdiSurface, x: i32, y: i32) -> Option<*mut u32> {
    if x < 0 || y < 0 || x >= surface.width || y >= surface.height {
        return None;
    }
    let offset = (y as usize)
        .checked_mul(surface.pitch as usize)?
        .checked_add((x as usize).checked_mul(4)?)?;
    // Dimensions and pitch above validate each pixel in the bound allocation.
    Some(unsafe { surface.bits.add(offset).cast() })
}

fn read_pixel(surface: &GdiSurface, x: i32, y: i32) -> Option<u32> {
    Some(unsafe { pixel_pointer(surface, x, y)?.read_unaligned() })
}

fn write_pixel(dc: &DcObject, surface: &GdiSurface, x: i32, y: i32, color: u32) {
    if let Some(pointer) = pixel_pointer(surface, x, y) {
        unsafe {
            pointer.write_unaligned(color);
        }
        dc.drawing_bounds.record_pixel(x, y);
    }
}

// COLORREF stores red in its low byte; framebuffer RGB stores blue there.
fn swap_red_blue(color: u32) -> u32 {
    ((color & 0xff) << 16) | (color & 0xff00) | ((color >> 16) & 0xff)
}

/// Prepared text draws retain one validated surface binding and reuse their
/// captured clip. Glyph coverage is 0..64, as returned by GGO_GRAY8_BITMAP.
pub(crate) struct CoveragePainter<'a> {
    dc: &'a DcObject,
    surface: &'a GdiSurface,
}

impl<'a> CoveragePainter<'a> {
    pub(crate) fn new(dc: &'a DcObject) -> Option<Self> {
        Some(Self { dc, surface: bound_surface(dc)? })
    }

    pub(crate) fn fill(&mut self, region: &Region, color: u32) {
        let color = swap_red_blue(color);
        for rect in region.rectangles() {
            for y in rect.top..rect.bottom {
                for x in rect.left..rect.right {
                    write_pixel(self.dc, self.surface, x, y, color);
                }
            }
        }
    }

    pub(crate) fn paint(&mut self, clip: &Region, origin: PointL,
        width: usize, height: usize, stride: usize, coverage: &[u8], color: u32)
    {
        let color = swap_red_blue(color);
        for rect in clip.rectangles() {
            let left = i64::from(rect.left).max(i64::from(origin.x));
            let top = i64::from(rect.top).max(i64::from(origin.y));
            let right = i64::from(rect.right).min(i64::from(origin.x) + width as i64);
            let bottom = i64::from(rect.bottom).min(i64::from(origin.y) + height as i64);
            for y in top..bottom {
                for x in left..right {
                    let row = (y - i64::from(origin.y)) as usize;
                    let column = (x - i64::from(origin.x)) as usize;
                    let Some(&alpha) = coverage.get(row * stride + column) else { continue };
                    let alpha = u32::from(alpha).min(64);
                    if alpha == 0 { continue; }
                    let Some(destination) = read_pixel(self.surface, x as i32, y as i32) else { continue };
                    let mut output = destination & 0xff00_0000;
                    for shift in [0, 8, 16] {
                        let foreground = (color >> shift) & 255;
                        let background = (destination >> shift) & 255;
                        output |= ((foreground * alpha + background * (64 - alpha) + 32) / 64) << shift;
                    }
                    write_pixel(self.dc, self.surface, x as i32, y as i32, output);
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
struct BrushSample {
    foreground: u32,
    background: Option<u32>,
    hatch: Option<i32>,
    origin: PointL,
}

impl BrushSample {
    fn solid(color: u32) -> Self {
        Self { foreground: color, background: None, hatch: None, origin: PointL::default() }
    }

    fn at(self, x: i32, y: i32) -> Option<u32> {
        let Some(style) = self.hatch else { return Some(self.foreground) };
        let x = (i64::from(x) - i64::from(self.origin.x)).rem_euclid(8);
        let y = (i64::from(y) - i64::from(self.origin.y)).rem_euclid(8);
        let foreground = match style {
            0 => y == 3,
            1 => x == 4,
            2 => x == y,
            3 => (x + y) % 8 == 7,
            4 => x == 4 || y == 3,
            5 => x == y || (x + y) % 8 == 7,
            _ => return None,
        };
        if foreground { Some(self.foreground) } else { self.background }
    }
}

fn brush_sample(dc: &DcObject, handle: u64) -> Option<Option<BrushSample>> {
    if handle == 0 { return Some(Some(BrushSample::solid(0xffffff))); }
    objects::with_handle_table(|table| {
        let entry = table.get(handle)?;
        if entry.get_type()? != GdiObjectType::Brush || entry.obj_ptr == 0 { return None; }
        // SAFETY: the object table guard pins and serializes the brush.
        let brush = unsafe { &*(entry.obj_ptr as *const GdiBrush) };
        sample_brush(dc, brush)
    })
}

fn sample_brush(dc: &DcObject, brush: &GdiBrush) -> Option<Option<BrushSample>> {
    let foreground = swap_red_blue(brush.color);
    match brush.style {
        value if value == objects::BrushStyle::Null as u32 => Some(None),
        value if value == objects::BrushStyle::Solid as u32 => Some(Some(BrushSample::solid(foreground))),
        value if value == objects::BrushStyle::Hatched as u32 && (0..=5).contains(&brush.hatch) => {
            Some(Some(BrushSample {
                foreground,
                background: (dc.bk_mode == super::dc::OPAQUE).then(|| swap_red_blue(dc.bg_color)),
                hatch: Some(brush.hatch),
                origin: dc.attributes.brush_origin,
            }))
        }
        _ => None,
    }
}

pub fn solid_brush_color(handle: u64) -> Option<u32> {
    objects::with_handle_table(|table| {
        let entry = table.get(handle)?;
        if entry.get_type()? != GdiObjectType::Brush || entry.obj_ptr == 0 {
            return None;
        }
        let brush = unsafe { &*(entry.obj_ptr as *const GdiBrush) };
        // Pattern/hatch brushes need an origin-aware sampler, not a solid fill.
        if brush.style != objects::BrushStyle::Solid as u32 {
            return None;
        }
        Some(swap_red_blue(brush.color))
    })
}

/// Evaluate the complete ROP3 truth table. Bits are indexed by P, S, D.
pub fn rop3(destination: u32, source: u32, pattern: u32, operation: u32) -> u32 {
    let table = (operation >> 16) as u8;
    let mut result = 0;
    for index in 0..8 {
        if table & (1 << index) != 0 {
            let p = if index & 4 != 0 { pattern } else { !pattern };
            let s = if index & 2 != 0 { source } else { !source };
            let d = if index & 1 != 0 {
                destination
            } else {
                !destination
            };
            result |= p & s & d;
        }
    }
    result & 0x00ff_ffff
}

fn uses_source(operation: u32) -> bool {
    let table = (operation >> 16) as u8;
    (table ^ (table >> 2)) & 0x33 != 0
}

fn uses_pattern(operation: u32) -> bool {
    let table = (operation >> 16) as u8;
    (table ^ (table >> 4)) & 0x0f != 0
}

// This path handles translations exactly. Scaled/rotated blits need resampling.
fn translation(dc: &DcObject) -> Option<PointL> {
    let t = dc.world_transform;
    if t.m11 != 1.0
        || t.m22 != 1.0
        || t.m12 != 0.0
        || t.m21 != 0.0
        || dc.window_ext.cx == 0
        || dc.window_ext.cy == 0
        || dc.window_ext.cx != dc.viewport_ext.cx
        || dc.window_ext.cy != dc.viewport_ext.cy
    {
        return None;
    }
    clipping::logical_point(dc, PointL::new(0, 0))
}

pub fn bit_blt(
    destination: &DcObject,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    source: Option<&DcObject>,
    source_x: i32,
    source_y: i32,
    operation: u32,
) -> bool {
    if width == 0 || height == 0 {
        return true;
    }
    if width < 0 || height < 0 {
        return false;
    }
    let Some(offset) = translation(destination) else {
        return false;
    };
    let (Some(x), Some(y)) = (x.checked_add(offset.x), y.checked_add(offset.y)) else {
        return false;
    };
    let (Some(right), Some(bottom)) = (x.checked_add(width), y.checked_add(height)) else {
        return false;
    };
    let Ok(requested) = Region::rectangle(Rect::new(x, y, right, bottom)) else {
        return false;
    };
    let Ok(clip) = clipping::effective_region(destination) else {
        return false;
    };
    let Ok(draw) = requested.combine(&clip, RegionCombineMode::Intersection) else {
        return false;
    };
    let Some(destination_surface) = bound_surface(destination) else {
        return false;
    };
    let pattern = if uses_pattern(operation) {
        let Some(brush) = brush_sample(destination, destination.brush) else {
            return false;
        };
        brush
    } else {
        Some(BrushSample::solid(0))
    };
    let source_data = if uses_source(operation) {
        let Some(source) = source else { return false };
        let Some(offset) = translation(source) else {
            return false;
        };
        let (Some(sx), Some(sy)) = (
            source_x.checked_add(offset.x),
            source_y.checked_add(offset.y),
        ) else {
            return false;
        };
        let Some(surface) = bound_surface(source) else {
            return false;
        };
        Some((surface, sx, sy))
    } else {
        None
    };

    // Capture only the clipped destination spans. Out-of-source pixels stay
    // untouched. No write occurs on allocation/size failure.
    let count = draw.rectangles().iter().try_fold(0usize, |sum, rect| {
        let area = (rect.right - rect.left) as usize * (rect.bottom - rect.top) as usize;
        sum.checked_add(area)
    });
    let Some(count) = count.filter(|&n| n <= MAX_CAPTURE_PIXELS) else {
        return false;
    };
    let mut samples = Vec::new();
    if source_data.is_some() {
        if samples.try_reserve_exact(count).is_err() {
            return false;
        }
        for rect in draw.rectangles() {
            for dy in rect.top..rect.bottom {
                for dx in rect.left..rect.right {
                    let (surface, sx, sy) = source_data.unwrap();
                    let pixel = i32::try_from(i64::from(sx) + i64::from(dx) - i64::from(x))
                        .ok()
                        .zip(i32::try_from(i64::from(sy) + i64::from(dy) - i64::from(y)).ok())
                        .and_then(|(sx, sy)| read_pixel(surface, sx, sy));
                    samples.push(pixel);
                }
            }
        }
    }
    let mut index = 0;
    for rect in draw.rectangles() {
        for y in rect.top..rect.bottom {
            for x in rect.left..rect.right {
                let sample = if source_data.is_some() {
                    samples[index]
                } else {
                    Some(0)
                };
                index += 1;
                if let (Some(source), Some(dest), Some(pattern)) = (
                    sample, read_pixel(destination_surface, x, y), pattern.and_then(|brush| brush.at(x, y)))
                {
                    write_pixel(
                        destination,
                        destination_surface,
                        x,
                        y,
                        rop3(dest, source, pattern, operation),
                    );
                }
            }
        }
    }
    true
}

pub fn paint_region(dc: &DcObject, geometry: &Region, color: u32, operation: u32) -> bool {
    paint_sampled_region(dc, geometry, BrushSample::solid(color), operation)
}

fn paint_sampled_region(dc: &DcObject, geometry: &Region, brush: BrushSample, operation: u32) -> bool {
    let Ok(clip) = clipping::effective_region(dc) else {
        return false;
    };
    let Ok(draw) = geometry.combine(&clip, RegionCombineMode::Intersection) else {
        return false;
    };
    let Some(surface) = bound_surface(dc) else {
        return false;
    };
    for rect in draw.rectangles() {
        for y in rect.top..rect.bottom {
            for x in rect.left..rect.right {
                if let (Some(dest), Some(color)) = (read_pixel(surface, x, y), brush.at(x, y)) {
                    write_pixel(dc, surface, x, y, rop3(dest, 0, color, operation));
                }
            }
        }
    }
    true
}

pub fn get_pixel(dc: &DcObject, x: i32, y: i32) -> u32 {
    let Some(point) = clipping::logical_point(dc, PointL::new(x, y)) else {
        return u32::MAX;
    };
    if !clipping::effective_region(dc).is_ok_and(|r| r.contains_point(point.x, point.y)) {
        return u32::MAX;
    }
    bound_surface(dc)
        .and_then(|s| read_pixel(s, point.x, point.y))
        .map(swap_red_blue)
        .unwrap_or(u32::MAX)
}

pub fn set_pixel(dc: &DcObject, x: i32, y: i32, color: u32) -> bool {
    let Some(point) = clipping::logical_point(dc, PointL::new(x, y)) else {
        return false;
    };
    if !clipping::effective_region(dc).is_ok_and(|r| r.contains_point(point.x, point.y)) {
        return false;
    }
    let Some(surface) = bound_surface(dc) else {
        return false;
    };
    write_pixel(dc, surface, point.x, point.y, swap_red_blue(color));
    true
}

use crate::arch::common::syscall_frame::SyscallFrame;

fn logical_region(dc: &DcObject, region: &Region) -> Option<Region> {
    let mut rectangles = Vec::new();
    rectangles
        .try_reserve_exact(region.rectangles().len())
        .ok()?;
    for rect in region.rectangles() {
        rectangles.push(clipping::logical_rect(dc, *rect)?);
    }
    Region::from_rectangles(&rectangles).ok()
}

pub extern "C" fn nt_gdi_fill_rgn(frame: &SyscallFrame) -> u64 {
    let Some(region) = super::region::snapshot(frame.args[1]) else {
        return 0;
    };
    let Some(dc) = super::dc::get_dc(frame.args[0]) else {
        return 0;
    };
    let Some(brush) = brush_sample(&dc, frame.args[2]) else { return 0 };
    let Some(region) = logical_region(&dc, &region) else {
        return 0;
    };
    brush.map_or(true, |brush| paint_sampled_region(&dc, &region, brush, super::gdi_ops::PATCOPY)) as u64
}

pub extern "C" fn nt_gdi_invert_rgn(frame: &SyscallFrame) -> u64 {
    let Some(region) = super::region::snapshot(frame.args[1]) else {
        return 0;
    };
    let Some(dc) = super::dc::get_dc(frame.args[0]) else {
        return 0;
    };
    let Some(region) = logical_region(&dc, &region) else {
        return 0;
    };
    paint_region(&dc, &region, 0, super::gdi_ops::DSTINVERT) as u64
}

pub extern "C" fn nt_gdi_frame_rgn(frame: &SyscallFrame) -> u64 {
    let Some(region) = super::region::snapshot(frame.args[1]) else {
        return 0;
    };
    let Ok(frame_region) = region.frame(frame.args[3] as i32, frame.args[4] as i32) else {
        return 0;
    };
    let Some(dc) = super::dc::get_dc(frame.args[0]) else {
        return 0;
    };
    let Some(brush) = brush_sample(&dc, frame.args[2]) else {
        return 0;
    };
    let Some(frame_region) = logical_region(&dc, &frame_region) else {
        return 0;
    };
    brush.map_or(true, |brush| {
        paint_sampled_region(&dc, &frame_region, brush, super::gdi_ops::PATCOPY)
    }) as u64
}

pub extern "C" fn nt_gdi_set_pixel(frame: &SyscallFrame) -> u64 {
    let Some(dc) = super::dc::get_dc(frame.args[0]) else {
        return u32::MAX as u64;
    };
    let color = frame.args[3] as u32 & 0x00ff_ffff;
    if set_pixel(&dc, frame.args[1] as i32, frame.args[2] as i32, color) {
        color as u64
    } else {
        u32::MAX as u64
    }
}

pub extern "C" fn nt_gdi_get_pixel(frame: &SyscallFrame) -> u64 {
    let Some(dc) = super::dc::get_dc(frame.args[0]) else {
        return u32::MAX as u64;
    };
    get_pixel(&dc, frame.args[1] as i32, frame.args[2] as i32) as u64
}

pub(super) fn pen_color(handle: u64) -> Option<Option<u32>> {
    if handle == 0 {
        return Some(Some(0));
    }
    objects::with_handle_table(|table| {
        let entry = table.get(handle)?;
        if entry.get_type()? != GdiObjectType::Pen || entry.obj_ptr == 0 {
            return None;
        }
        let pen = unsafe { &*(entry.obj_ptr as *const objects::GdiPen) };
        if pen.style == objects::PenStyle::Null as u32 {
            return Some(None);
        }
        if pen.style != objects::PenStyle::Solid as u32 || pen.width > 1 {
            return None;
        }
        Some(Some(swap_red_blue(pen.color)))
    })
}

pub(super) fn rop2_operation(mode: i32) -> Option<u32> {
    if !(1..=16).contains(&mode) {
        return None;
    }
    let table = (mode - 1) as u32;
    Some(((table & 3) | ((table & 3) << 2) | ((table & 12) << 2) | ((table & 12) << 4)) << 16)
}

pub fn line_to(dc: &mut DcObject, x: i32, y: i32) -> bool {
    let Some(color) = pen_color(dc.pen) else {
        return false;
    };
    let Some(operation) = rop2_operation(dc.rop_mode) else {
        return false;
    };
    let Some(start) = clipping::logical_point(dc, PointL::new(dc.cur_x, dc.cur_y)) else {
        return false;
    };
    let Some(end) = clipping::logical_point(dc, PointL::new(x, y)) else {
        return false;
    };
    let Ok(clip) = clipping::effective_region(dc) else {
        return false;
    };
    let Some(surface) = bound_surface(dc) else {
        return false;
    };
    let dx = (i64::from(end.x) - i64::from(start.x)).abs();
    let dy = -(i64::from(end.y) - i64::from(start.y)).abs();
    // Bound work while holding the DC lock, without changing the requested line.
    if dx.max(-dy) > 2_097_152 {
        return false;
    }
    if let Some(color) = color {
        let sx = if start.x < end.x { 1 } else { -1 };
        let sy = if start.y < end.y { 1 } else { -1 };
        let (mut px, mut py) = (start.x, start.y);
        let mut error = dx + dy;
        // GDI LineTo includes the start and excludes the final pixel.
        while px != end.x || py != end.y {
            if clip.contains_point(px, py) {
                if let Some(old) = read_pixel(surface, px, py) {
                    write_pixel(dc, surface, px, py, rop3(old, 0, color, operation));
                }
            }
            let twice = 2 * error;
            if twice >= dy {
                error += dy;
                px += sx;
            }
            if twice <= dx {
                error += dx;
                py += sy;
            }
        }
    }
    dc.cur_x = x;
    dc.cur_y = y;
    true
}

pub fn rectangle(dc: &DcObject, rect: Rect) -> bool {
    let Some(rect) = clipping::logical_rect(dc, rect) else {
        return false;
    };
    let Some(pen) = pen_color(dc.pen) else {
        return false;
    };
    let Some(brush) = brush_sample(dc, dc.brush) else {
        return false;
    };
    let Some(operation) = rop2_operation(dc.rop_mode) else {
        return false;
    };
    let Ok(outer) = Region::rectangle(rect) else {
        return false;
    };
    let inner = if rect.right - rect.left > 2 && rect.bottom - rect.top > 2 {
        Region::rectangle(Rect::new(
            rect.left + 1,
            rect.top + 1,
            rect.right - 1,
            rect.bottom - 1,
        ))
    } else {
        Ok(Region::default())
    };
    let Ok(inner) = inner else { return false };
    let Ok(outline) = outer.combine(&inner, RegionCombineMode::Difference) else {
        return false;
    };
    // Validate the surface and clip before drawing either part.
    if bound_surface(dc).is_none() || clipping::effective_region(dc).is_err() {
        return false;
    }
    if let Some(brush) = brush {
        let fill = if pen.is_some() { &inner } else { &outer };
        if !paint_sampled_region(dc, fill, brush, operation) {
            return false;
        }
    }
    if let Some(color) = pen {
        if !paint_region(dc, &outline, color, operation) {
            return false;
        }
    }
    true
}

/// Rasterize a device-coordinate path with a one-pixel solid pen. Accumulating
/// coverage first prevents XOR from drawing shared vertices or joins twice.
pub(super) fn path_outline(path: &super::path::GdiPath) -> Option<Region> {
    use super::path::{PT_CLOSEFIGURE, PT_LINETO, PT_MOVETO};
    const MAX_STROKE_WORK: usize = 1_048_576;
    let mut pixels: Vec<(i32, i32)> = Vec::new();
    let mut segment = |start: PointL, end: PointL| -> Option<()> {
        let dx = (i64::from(end.x) - i64::from(start.x)).abs();
        let dy = -(i64::from(end.y) - i64::from(start.y)).abs();
        let count = usize::try_from(dx.max(-dy)).ok()?;
        if pixels.len().checked_add(count)? > MAX_STROKE_WORK {
            return None;
        }
        pixels.try_reserve(count).ok()?;
        let sx = if start.x < end.x { 1 } else { -1 };
        let sy = if start.y < end.y { 1 } else { -1 };
        let (mut x, mut y) = (start.x, start.y);
        let mut error = dx + dy;
        while x != end.x || y != end.y {
            pixels.push((y, x));
            let twice = 2 * error;
            if twice >= dy {
                error += dy;
                x += sx;
            }
            if twice <= dx {
                error += dx;
                y += sy;
            }
        }
        Some(())
    };
    let mut first = None;
    let mut previous = None;
    for entry in &path.entries {
        match entry.kind & !PT_CLOSEFIGURE {
            PT_MOVETO => {
                first = Some(entry.point);
            }
            PT_LINETO => {
                segment(previous?, entry.point)?;
            }
            _ => return None,
        }
        if entry.kind & PT_CLOSEFIGURE != 0 {
            segment(entry.point, first?)?;
        }
        previous = Some(entry.point);
    }
    pixels.sort_unstable();
    pixels.dedup();
    let mut rows: Vec<Rect> = Vec::new();
    for (y, x) in pixels {
        let right = x.checked_add(1)?;
        if let Some(last) = rows
            .last_mut()
            .filter(|last| last.top == y && last.right == x)
        {
            last.right = right;
        } else {
            rows.try_reserve(1).ok()?;
            rows.push(Rect::new(x, y, right, y.checked_add(1)?));
        }
    }
    Region::from_scanlines(&rows).ok()
}

pub(super) fn paint_path(
    dc: &DcObject,
    path: &super::path::GdiPath,
    fill: bool,
    stroke: bool,
) -> bool {
    let Some(flattened) = path.flattened() else {
        return false;
    };
    let Some(brush) = (if fill {
        brush_sample(dc, dc.brush)
    } else {
        Some(None)
    }) else {
        return false;
    };
    let Some(pen) = (if stroke {
        pen_color(dc.pen)
    } else {
        Some(None)
    }) else {
        return false;
    };
    let Some(operation) = rop2_operation(dc.rop_mode) else {
        return false;
    };
    let fill_region = if brush.is_some() {
        flattened.region(dc.poly_fill_mode)
    } else {
        Some(Region::default())
    };
    let outline = if pen.is_some() {
        path_outline(&flattened)
    } else {
        Some(Region::default())
    };
    let (Some(fill_region), Some(outline)) = (fill_region, outline) else {
        return false;
    };
    let Ok(fill_region) = fill_region.combine(&outline, RegionCombineMode::Difference) else {
        return false;
    };
    // Compute both clipped regions before writing either part. Failed geometry
    // allocation must not leave a half-filled StrokeAndFillPath on screen.
    let Ok(clip) = clipping::effective_region(dc) else {
        return false;
    };
    let (Ok(fill_region), Ok(outline)) = (
        fill_region.combine(&clip, RegionCombineMode::Intersection),
        outline.combine(&clip, RegionCombineMode::Intersection),
    ) else {
        return false;
    };
    let Some(surface) = bound_surface(dc) else {
        return false;
    };
    for (region, brush) in [(&fill_region, brush), (&outline, pen.map(BrushSample::solid))] {
        if let Some(brush) = brush {
            for rect in region.rectangles() {
                for y in rect.top..rect.bottom {
                    for x in rect.left..rect.right {
                        if let (Some(old), Some(color)) = (read_pixel(surface, x, y), brush.at(x, y)) {
                            write_pixel(dc, surface, x, y, rop3(old, 0, color, operation));
                        }
                    }
                }
            }
        }
    }
    true
}
