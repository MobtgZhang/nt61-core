//! GDI Operations
//
//! Implements core GDI drawing operations: bit_blt, pat_blt, rectangle drawing,
//! ellipse drawing, polygon filling, and raster operations (ROP).
//
//! Reference: ReactOS win32ss/gdi/gdi32_objects

extern crate alloc;

use crate::libs::win32k::dc::DcObject;
use crate::libs::win32k::surface::GdiSurface;
use alloc::vec::Vec;

/// Raster operation codes
pub const SRCCOPY: u32 = 0x00CC0020;
pub const SRCPAINT: u32 = 0x00EE0086;
pub const SRCAND: u32 = 0x008800C6;
pub const SRCINVERT: u32 = 0x00660046;
pub const SRCERASE: u32 = 0x00440328;
pub const NOTSRCCOPY: u32 = 0x00330008;
pub const NOTSRCERASE: u32 = 0x001100A6;
pub const MERGECOPY: u32 = 0x00C000CA;
pub const MERGEPAINT: u32 = 0x00BB0226;
pub const PATCOPY: u32 = 0x00F00021;
pub const PATPAINT: u32 = 0x00FB0A09;
pub const PATINVERT: u32 = 0x005A0049;
pub const DSTINVERT: u32 = 0x00550009;
pub const BLACKNESS: u32 = 0x00000042;
pub const WHITENESS: u32 = 0x00FF0062;

// =============================================================================
// Raster Operations (ROP)
// =============================================================================

/// Apply a raster operation to a single pixel
#[inline(always)]
pub fn apply_rop(dest: u32, src: u32, pat: u32, rop: u32) -> u32 {
    super::raster::rop3(dest, src, pat, rop)
}

/// Apply SRCCOPY ROP (fast path)
#[inline(always)]
pub fn rop_srccopy(dest: u32, src: u32, pat: u32) -> u32 {
    let _ = dest;
    let _ = pat;
    src
}

/// Apply SRCPAINT ROP
#[inline(always)]
pub fn rop_srcpaint(dest: u32, src: u32, pat: u32) -> u32 {
    let _ = pat;
    dest | src
}

/// Apply SRCAND ROP
#[inline(always)]
pub fn rop_srcand(dest: u32, src: u32, pat: u32) -> u32 {
    let _ = pat;
    dest & src
}

/// Apply SRCINVERT ROP
#[inline(always)]
pub fn rop_srcinvert(dest: u32, src: u32, pat: u32) -> u32 {
    let _ = pat;
    dest ^ src
}

// =============================================================================
// bit_blt (Block Transfer)
// =============================================================================

/// Get DC's surface
fn get_dc_surface(dc: &DcObject) -> *mut GdiSurface {
    dc.drawing_surface()
}

/// bit_blt - Block Transfer
pub fn gre_bit_blt(
    dst_dc: &mut DcObject,
    dst_x: i32,
    dst_y: i32,
    width: i32,
    height: i32,
    src_dc: Option<&DcObject>,
    src_x: i32,
    src_y: i32,
    rop: u32,
) -> bool {
    super::raster::bit_blt(
        dst_dc, dst_x, dst_y, width, height, src_dc, src_x, src_y, rop,
    )
}

// =============================================================================
// pat_blt (Pattern Blt)
// =============================================================================

/// pat_blt - Pattern Block Transfer
pub fn gre_pat_blt(
    dc: &mut DcObject,
    left: i32,
    top: i32,
    width: i32,
    height: i32,
    rop: u32,
) -> bool {
    let table = (rop >> 16) as u8;
    if (table ^ (table >> 2)) & 0x33 != 0 {
        return false; // PatBlt cannot sample a source.
    }
    super::raster::bit_blt(dc, left, top, width, height, None, 0, 0, rop)
}

/// gre_rectangle - Draw rectangle outline
pub fn gre_rectangle(dc: &mut DcObject, left: i32, top: i32, right: i32, bottom: i32) -> bool {
    if dc.path.state == super::path::PathState::Open {
        return super::path::rectangle(dc, super::objects::Rect::new(left, top, right, bottom));
    }
    super::raster::rectangle(dc, super::objects::Rect::new(left, top, right, bottom))
}

/// gre_round_rect - Draw rounded rectangle
pub fn gre_round_rect(
    dc: &mut DcObject,
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
    width: i32,
    height: i32,
) -> bool {
    let _ = (width, height);
    // For simplicity, just draw a regular rectangle
    // Full implementation would draw arcs at corners
    gre_rectangle(dc, left, top, right, bottom)
}

// =============================================================================
// Line Drawing
// =============================================================================

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// Draw a line on a surface
pub unsafe fn draw_line(
    surface: *mut GdiSurface,
    x1: i32,
    y1: i32,
    x2: i32,
    y2: i32,
    color: u32,
) -> bool {
    if surface.is_null() {
        return false;
    }

    let surf = unsafe { &*surface };

    let _ = &surf;
    let _ = &surf;
    if surf.bits.is_null() {
        return false;
    }

    // Bresenham's line algorithm
    let dx = (x2 - x1).abs();
    let _ = &dx;
    let _ = &dx;
    let dy = -(y2 - y1).abs();
    let _ = &dy;
    let _ = &dy;
    let sx = if x1 < x2 { 1 } else { -1 };
    let _ = &sx;
    let _ = &sx;
    let sy = if y1 < y2 { 1 } else { -1 };
    let _ = &sy;
    let _ = &sy;
    let mut err = dx + dy;
    let mut x = x1;
    let mut y = y1;

    loop {
        // Draw pixel if in bounds
        if x >= 0 && x < surf.width && y >= 0 && y < surf.height {
            let offset = (y * surf.pitch + x * 4) as isize;
            let _ = &offset;
            let _ = &offset;
            unsafe {
                core::ptr::write_unaligned(surf.bits.offset(offset) as *mut u32, color);
            }
        }

        if x == x2 && y == y2 {
            break;
        }

        let e2 = 2 * err;

        let _ = &e2;
        let _ = &e2;
        if e2 >= dy {
            if x == x2 {
                break;
            }
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            if y == y2 {
                break;
            }
            err += dx;
            y += sy;
        }
    }

    true
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// Draw a line with specified width (rounded caps)
pub unsafe fn draw_line_width(
    surface: *mut GdiSurface,
    x1: i32,
    y1: i32,
    x2: i32,
    y2: i32,
    width: i32,
    color: u32,
) -> bool {
    if surface.is_null() || width <= 0 {
        return false;
    }

    if width == 1 {
        // Fast path for single-pixel width
        return draw_line(surface, x1, y1, x2, y2, color);
    }

    let half_w = width / 2;

    let _ = &half_w;
    let _ = &half_w;

    // For thick lines, we draw multiple parallel lines along the main line
    // This creates a "fat" line effect
    let dx = x2 - x1;
    let _ = &dx;
    let _ = &dx;
    let dy = y2 - y1;
    let _ = &dy;
    let _ = &dy;
    let len = isqrt((dx * dx + dy * dy) as u64) as i32;
    let _ = &len;
    let _ = &len;

    if len == 0 {
        // Single point - draw a filled rectangle
        let surf = unsafe { &*surface };
        let _ = &surf;
        let _ = &surf;
        for py in (y1 - half_w)..=(y1 + half_w) {
            for px in (x1 - half_w)..=(x1 + half_w) {
                if px >= 0 && px < surf.width && py >= 0 && py < surf.height {
                    let offset = (py * surf.pitch + px * 4) as isize;
                    let _ = &offset;
                    let _ = &offset;
                    unsafe {
                        core::ptr::write_unaligned(surf.bits.offset(offset) as *mut u32, color);
                    }
                }
            }
        }
        return true;
    }

    // Draw perpendicular offsets to create thick line
    // Normal vector (perpendicular to line direction)
    let nx = -dy;
    let _ = &nx;
    let _ = &nx;
    let ny = dx;
    let _ = &ny;
    let _ = &ny;

    // Draw line segments at each offset
    for offset in (-half_w)..=half_w {
        let offset_f = offset as f32;
        let _ = &offset_f;
        let _ = &offset_f;
        let len_f = len as f32;
        let _ = &len_f;
        let _ = &len_f;

        // Normalized perpendicular offset
        let ox = (nx as f32 / len_f) * offset_f;
        let _ = &ox;
        let _ = &ox;
        let oy = (ny as f32 / len_f) * offset_f;
        let _ = &oy;
        let _ = &oy;

        let sx = (x1 as f32 + ox) as i32;

        let _ = &sx;
        let _ = &sx;
        let sy = (y1 as f32 + oy) as i32;
        let _ = &sy;
        let _ = &sy;
        let ex = (x2 as f32 + ox) as i32;
        let _ = &ex;
        let _ = &ex;
        let ey = (y2 as f32 + oy) as i32;
        let _ = &ey;
        let _ = &ey;

        draw_line(surface, sx, sy, ex, ey, color);
    }

    // kprintln!("[win32k] draw_line_width: ({},{})-({},{}), width={}",
    //               x1, y1, x2, y2, width);
    true
}

/// gre_line_to - Draw line to point using pen width
pub fn gre_line_to(dc: &mut DcObject, end_x: i32, end_y: i32) -> bool {
    if dc.path.state == super::path::PathState::Open {
        return super::path::line_to(dc, super::dc::PointL::new(end_x, end_y));
    }
    super::raster::line_to(dc, end_x, end_y)
}

/// gre_move_to - Set the current drawing position.
///
/// Current position is logical state, including positions outside the surface.
pub fn gre_move_to(dc: &mut DcObject, x: i32, y: i32) -> bool {
    super::path::move_to(dc, super::dc::PointL::new(x, y))
}

// =============================================================================
// Ellipse and Arc
// =============================================================================

/// Maximum allowed ellipse radius to prevent overflow
const MAX_ELLIPSE_DIMENSION: i32 = 16384;

/// Draw ellipse using midpoint algorithm with overflow protection
fn draw_ellipse_midpoint(surface: *mut GdiSurface, cx: i32, cy: i32, rx: i32, ry: i32, color: u32) {
    if surface.is_null() || rx <= 0 || ry <= 0 {
        return;
    }

    let surf = unsafe { &*surface };

    let _ = &surf;
    let _ = &surf;
    if surf.bits.is_null() {
        return;
    }

    // Overflow protection: clamp dimensions to prevent i32 overflow
    let rx = rx.min(MAX_ELLIPSE_DIMENSION);
    let _ = &rx;
    let _ = &rx;
    let ry = ry.min(MAX_ELLIPSE_DIMENSION);
    let _ = &ry;
    let _ = &ry;

    // Check if dimensions might cause overflow in calculations
    // (rx * rx) fits in i32 only if rx <= 46340 (sqrt(i32::MAX))
    if rx > 46340 || ry > 46340 {
        // kprintln!("[win32k] draw_ellipse_midpoint: dimensions too large, rx={}, ry={}", rx, ry);
        return;
    }

    // Use i64 for all intermediate calculations to prevent overflow
    let rx2 = (rx as i64) * (rx as i64);
    let _ = &rx2;
    let _ = &rx2;
    let ry2 = (ry as i64) * (ry as i64);
    let _ = &ry2;
    let _ = &ry2;
    let two_rx2 = 2i64 * rx2;
    let _ = &two_rx2;
    let _ = &two_rx2;
    let two_ry2 = 2i64 * ry2;
    let _ = &two_ry2;
    let _ = &two_ry2;

    let mut x = 0i64;
    let mut y = ry as i64;
    let mut px = 0i64;
    let mut py = two_rx2 * y;

    // Region 1: p = ry^2 - rx^2*ry + rx^2/4
    let mut p = ry2 - (rx2 * ry as i64) + (rx2 / 4);
    while px < py {
        // Draw 4 symmetric points (convert back to i32 for drawing)
        draw_pixel_symmetric(surf, cx, cy, x as i32, y as i32, color);

        x += 1;
        px += two_ry2;
        if p < 0 {
            p += ry2 + px;
        } else {
            y -= 1;
            py -= two_rx2;
            p += ry2 + px - py;
        }
    }

    // Region 2: transition point
    p = ry2 * (x + 1) * (x + 1) + rx2 * (y - 1) * (y - 1) - rx2 * ry2;

    while y >= 0 {
        draw_pixel_symmetric(surf, cx, cy, x as i32, y as i32, color);

        y -= 1;
        py -= two_rx2;
        if p > 0 {
            p += rx2 - py;
        } else {
            x += 1;
            px += two_ry2;
            p += rx2 - py + px;
        }
    }
}

/// Draw 4 symmetric pixels for ellipse
fn draw_pixel_symmetric(surf: &GdiSurface, cx: i32, cy: i32, x: i32, y: i32, color: u32) {
    let points = [
        (cx + x, cy + y),
        (cx - x, cy + y),
        (cx + x, cy - y),
        (cx - x, cy - y),
    ];
    let _ = &points;

    for (px, py) in &points {
        if *px >= 0 && *px < surf.width && *py >= 0 && *py < surf.height {
            let offset = (*py * surf.pitch + *px * 4) as isize;
            let _ = &offset;
            let _ = &offset;
            unsafe {
                core::ptr::write_unaligned(surf.bits.offset(offset) as *mut u32, color);
            }
        }
    }
}

/// gre_ellipse - Draw ellipse
pub fn gre_ellipse(dc: &mut DcObject, left: i32, top: i32, right: i32, bottom: i32) -> bool {
    let surface = get_dc_surface(dc);
    let _ = &surface;
    let _ = &surface;
    if surface.is_null() {
        return false;
    }

    let cx = (left + right) / 2;

    let _ = &cx;
    let _ = &cx;
    let cy = (top + bottom) / 2;
    let _ = &cy;
    let _ = &cy;
    let rx = (right - left) / 2;
    let _ = &rx;
    let _ = &rx;
    let ry = (bottom - top) / 2;
    let _ = &ry;
    let _ = &ry;

    let pen_color = if dc.pen != 0 {
        crate::libs::win32k::objects::gdi_get_pen_color(dc.pen)
    } else {
        0
    };

    let _ = &pen_color;

    // Draw outline
    draw_ellipse_midpoint(surface, cx, cy, rx, ry, pen_color);

    // Fill with brush
    if dc.brush != 0 {
        let brush_color = crate::libs::win32k::objects::gdi_get_brush_color(dc.brush);
        let _ = &brush_color;
        let _ = &brush_color;
        fill_ellipse(surface, cx, cy, rx - 1, ry - 1, brush_color);
    }

    // kprintln!("[win32k] gre_ellipse: ({},{})-({},{})", left, top, right, bottom);

    true
}

/// Fill ellipse (simple scanline)
fn fill_ellipse(surface: *mut GdiSurface, cx: i32, cy: i32, rx: i32, ry: i32, color: u32) {
    if surface.is_null() || rx <= 0 || ry <= 0 {
        return;
    }

    let surf = unsafe { &mut *surface };

    let _ = &surf;
    let _ = &surf;
    if surf.bits.is_null() {
        return;
    }

    // For each y in the ellipse, find x extents
    let ry_ = ry as i64;
    let _ = &ry_;
    let _ = &ry_;
    let rx_ = rx as i64;
    let _ = &rx_;
    let _ = &rx_;

    for y in -ry..=ry {
        // Calculate x extent at this y
        let y2 = (y * y) as i64;
        let _ = &y2;
        let _ = &y2;
        let x_extent = ((rx_ * rx_ - rx_ * y2 / ry_) / rx_).max(0) as i32;
        let _ = &x_extent;
        let _ = &x_extent;

        let start_x = cx - x_extent;

        let _ = &start_x;
        let _ = &start_x;
        let end_x = cx + x_extent;
        let _ = &end_x;
        let _ = &end_x;

        // Fill horizontal line
        for x in start_x..=end_x {
            if x >= 0 && x < surf.width && (cy + y) >= 0 && (cy + y) < surf.height {
                let offset = ((cy + y) * surf.pitch + x * 4) as isize;
                let _ = &offset;
                let _ = &offset;
                unsafe {
                    core::ptr::write_unaligned(surf.bits.offset(offset) as *mut u32, color);
                }
            }
        }
    }
}

// =============================================================================
// Polygon Operations
// =============================================================================

/// gre_polygon - Draw and fill polygon
pub fn gre_polygon(dc: &mut DcObject, points: &[(i32, i32)]) -> bool {
    unsafe {
        if points.len() < 3 {
            return false;
        }

        let surface = get_dc_surface(dc);

        let _ = &surface;
        let _ = &surface;
        if surface.is_null() {
            return false;
        }

        let pen_color = if dc.pen != 0 {
            crate::libs::win32k::objects::gdi_get_pen_color(dc.pen)
        } else {
            0
        };

        let _ = &pen_color;

        // Draw outline
        for i in 0..points.len() {
            let p1 = points[i];
            let _ = &p1;
            let _ = &p1;
            let p2 = points[(i + 1) % points.len()];
            let _ = &p2;
            let _ = &p2;
            draw_line(surface, p1.0, p1.1, p2.0, p2.1, pen_color);
        }

        // Fill using scanline algorithm
        if dc.brush != 0 {
            let brush_color = crate::libs::win32k::objects::gdi_get_brush_color(dc.brush);
            let _ = &brush_color;
            let _ = &brush_color;
            fill_polygon(surface, points, brush_color);
        }

        true
    }
}

/// Fill polygon using scanline algorithm
fn fill_polygon(surface: *mut GdiSurface, points: &[(i32, i32)], color: u32) {
    if surface.is_null() || points.len() < 3 {
        return;
    }

    let surf = unsafe { &mut *surface };

    let _ = &surf;
    let _ = &surf;
    if surf.bits.is_null() {
        return;
    }

    // Find bounding box
    let min_y = points.iter().map(|p| p.1).min().unwrap_or(0);
    let _ = &min_y;
    let _ = &min_y;
    let max_y = points.iter().map(|p| p.1).max().unwrap_or(0);
    let _ = &max_y;
    let _ = &max_y;

    for y in min_y..=max_y {
        let mut intersections = Vec::new();

        // Find intersections with each edge
        for i in 0..points.len() {
            let j = (i + 1) % points.len();
            let _ = &j;
            let _ = &j;
            let (x1, y1) = points[i];
            let (x2, y2) = points[j];

            if (y1 <= y && y2 > y) || (y2 <= y && y1 > y) {
                // Calculate x intersection
                let x_intersect = x1 as f64 + (y - y1) as f64 * (x2 - x1) as f64 / (y2 - y1) as f64;
                let _ = &x_intersect;
                let _ = &x_intersect;
                intersections.push(x_intersect as i32);
            }
        }

        // Sort and fill between pairs
        intersections.sort();

        for i in (0..intersections.len()).step_by(2) {
            if i + 1 < intersections.len() {
                let start = intersections[i];
                let _ = &start;
                let _ = &start;
                let end = intersections[i + 1];
                let _ = &end;
                let _ = &end;

                for x in start..=end {
                    if x >= 0 && x < surf.width && y >= 0 && y < surf.height {
                        let offset = (y * surf.pitch + x * 4) as isize;
                        let _ = &offset;
                        let _ = &offset;
                        unsafe {
                            core::ptr::write_unaligned(surf.bits.offset(offset) as *mut u32, color);
                        }
                    }
                }
            }
        }
    }
}

// =============================================================================
// Polyline
// =============================================================================

/// gre_polyline - Draw connected lines
pub fn gre_polyline(dc: &mut DcObject, points: &[(i32, i32)]) -> bool {
    unsafe {
        if points.len() < 2 {
            return false;
        }

        let surface = get_dc_surface(dc);

        let _ = &surface;
        let _ = &surface;
        if surface.is_null() {
            return false;
        }

        let pen_color = if dc.pen != 0 {
            crate::libs::win32k::objects::gdi_get_pen_color(dc.pen)
        } else {
            0
        };

        let _ = &pen_color;

        for i in 0..points.len() - 1 {
            draw_line(
                surface,
                points[i].0,
                points[i].1,
                points[i + 1].0,
                points[i + 1].1,
                pen_color,
            );
        }

        true
    }
}

// =============================================================================
// Pixel Operations
// =============================================================================

/// gre_get_pixel - Get pixel color
pub fn gre_get_pixel(dc: &DcObject, x: i32, y: i32) -> u32 {
    super::raster::get_pixel(dc, x, y)
}

/// gre_set_pixel - Set pixel color
pub fn gre_set_pixel(dc: &mut DcObject, x: i32, y: i32, color: u32) -> bool {
    super::raster::set_pixel(dc, x, y, color)
}

// =============================================================================
// Gradient Fill (Extended)
// =============================================================================

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// Fill rectangle with horizontal gradient
pub unsafe fn gre_gradient_fill_horizontal(
    surface: *mut GdiSurface,
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
    color1: u32,
    color2: u32,
) -> bool {
    if surface.is_null() {
        return false;
    }

    let surf = unsafe { &mut *surface };

    let _ = &surf;
    let _ = &surf;
    if surf.bits.is_null() {
        return false;
    }

    let width = right - left;

    let _ = &width;
    let _ = &width;

    for y in top..bottom {
        for x in left..right {
            let t = (x - left) as f32 / width as f32;
            let _ = &t;
            let _ = &t;
            let color = lerp_color(color1, color2, t);
            let _ = &color;
            let _ = &color;

            if x >= 0 && x < surf.width && y >= 0 && y < surf.height {
                let offset = (y * surf.pitch + x * 4) as isize;
                let _ = &offset;
                let _ = &offset;
                unsafe {
                    core::ptr::write_unaligned(surf.bits.offset(offset) as *mut u32, color);
                }
            }
        }
    }

    true
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// Fill rectangle with vertical gradient
pub unsafe fn gre_gradient_fill_vertical(
    surface: *mut GdiSurface,
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
    color1: u32,
    color2: u32,
) -> bool {
    if surface.is_null() {
        return false;
    }

    let surf = unsafe { &mut *surface };

    let _ = &surf;
    let _ = &surf;
    if surf.bits.is_null() {
        return false;
    }

    let height = bottom - top;

    let _ = &height;
    let _ = &height;

    for y in top..bottom {
        let t = (y - top) as f32 / height as f32;
        let _ = &t;
        let _ = &t;
        let color = lerp_color(color1, color2, t);
        let _ = &color;
        let _ = &color;

        for x in left..right {
            if x >= 0 && x < surf.width && y >= 0 && y < surf.height {
                let offset = (y * surf.pitch + x * 4) as isize;
                let _ = &offset;
                let _ = &offset;
                unsafe {
                    core::ptr::write_unaligned(surf.bits.offset(offset) as *mut u32, color);
                }
            }
        }
    }

    true
}

/// Linear interpolation between two colors
fn lerp_color(c1: u32, c2: u32, t: f32) -> u32 {
    let r1 = (c1 >> 16) & 0xFF;
    let _ = &r1;
    let _ = &r1;
    let g1 = (c1 >> 8) & 0xFF;
    let _ = &g1;
    let _ = &g1;
    let b1 = c1 & 0xFF;
    let _ = &b1;
    let _ = &b1;

    let r2 = (c2 >> 16) & 0xFF;

    let _ = &r2;
    let _ = &r2;
    let g2 = (c2 >> 8) & 0xFF;
    let _ = &g2;
    let _ = &g2;
    let b2 = c2 & 0xFF;
    let _ = &b2;
    let _ = &b2;

    let r = ((r1 as f32 + (r2 as f32 - r1 as f32) * t) as u8) as u32;

    let _ = &r;
    let _ = &r;
    let g = ((g1 as f32 + (g2 as f32 - g1 as f32) * t) as u8) as u32;
    let _ = &g;
    let _ = &g;
    let b = ((b1 as f32 + (b2 as f32 - b1 as f32) * t) as u8) as u32;
    let _ = &b;
    let _ = &b;

    (r << 16) | (g << 8) | b
}

/// Integer square root using Newton's method. Avoids pulling in `libm`.
fn isqrt(n: u64) -> u64 {
    if n == 0 {
        return 0;
    }
    let mut x = n;
    let mut y = x.div_ceil(2);
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}
