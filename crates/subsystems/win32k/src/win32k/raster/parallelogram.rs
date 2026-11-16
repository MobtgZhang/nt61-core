//! Rectangle-to-parallelogram copies on 32-bit DC surfaces. Pixel centers are
//! tested with exact integer determinants; capture precedes destination writes.
//! COLORONCOLOR is supported. Other stretch modes need a coverage filter.

use super::*;
use crate::arch::common::syscall_frame::SyscallFrame;
use crate::mm::user_copy;

struct Parallelogram {
    origin: PointL,
    horizontal: (i64, i64),
    vertical: (i64, i64),
    determinant: i128,
    bounds: Rect,
}

fn cross(first: (i64, i64), second: (i64, i64)) -> i128 {
    i128::from(first.0) * i128::from(second.1) - i128::from(first.1) * i128::from(second.0)
}

impl Parallelogram {
    fn new(dc: &DcObject, points: [PointL; 3]) -> Option<Self> {
        let a = clipping::logical_point(dc, points[0])?;
        let b = clipping::logical_point(dc, points[1])?;
        let c = clipping::logical_point(dc, points[2])?;
        let horizontal = (
            i64::from(b.x) - i64::from(a.x),
            i64::from(b.y) - i64::from(a.y),
        );
        let vertical = (
            i64::from(c.x) - i64::from(a.x),
            i64::from(c.y) - i64::from(a.y),
        );
        let determinant = cross(horizontal, vertical);
        if determinant == 0 {
            return None;
        }
        let d = PointL::new(
            i32::try_from(i64::from(b.x) + vertical.0).ok()?,
            i32::try_from(i64::from(b.y) + vertical.1).ok()?,
        );
        let points = [a, b, c, d];
        let bounds = Rect::new(
            points.iter().map(|point| point.x).min()?,
            points.iter().map(|point| point.y).min()?,
            points.iter().map(|point| point.x).max()?,
            points.iter().map(|point| point.y).max()?,
        );
        super::super::region_geometry::valid_rect(&bounds).then_some(Self {
            origin: a,
            horizontal,
            vertical,
            determinant,
            bounds,
        })
    }

    /// Return normalized coordinates as numerators over 2*abs(determinant).
    /// Doubled coordinates retain the half-pixel center without floating point.
    fn coordinates(&self, x: i32, y: i32) -> Option<(i128, i128, i128)> {
        let offset = (
            2 * (i64::from(x) - i64::from(self.origin.x)) + 1,
            2 * (i64::from(y) - i64::from(self.origin.y)) + 1,
        );
        let direction = self.determinant.signum();
        let horizontal = cross(offset, self.vertical) * direction;
        let vertical = cross(self.horizontal, offset) * direction;
        let denominator = 2 * self.determinant.abs();
        (horizontal >= 0 && horizontal < denominator && vertical >= 0 && vertical < denominator)
            .then_some((horizontal, vertical, denominator))
    }
}

struct SourceRect {
    first: PointL,
    last: PointL,
}

impl SourceRect {
    fn new(dc: &DcObject, x: i32, y: i32, width: i32, height: i32) -> Option<Self> {
        // The source DC may reflect/scale/translate, but PlgBlt does not accept
        // a rotated or sheared source DC.
        if dc.world_transform.m12 != 0.0 || dc.world_transform.m21 != 0.0 {
            return None;
        }
        let first = clipping::logical_point(dc, PointL::new(x, y))?;
        let last = clipping::logical_point(
            dc,
            PointL::new(x.checked_add(width)?, y.checked_add(height)?),
        )?;
        (first.x != last.x && first.y != last.y).then_some(Self { first, last })
    }

    fn sample(&self, u: i128, v: i128, denominator: i128) -> PointL {
        fn axis(first: i32, last: i32, numerator: i128, denominator: i128) -> i32 {
            let distance = i128::from(last) - i128::from(first);
            let index = numerator * distance.abs() / denominator;
            // 0 <= index < abs(distance), so the result lies between the
            // validated i32 endpoints even when the source is reflected.
            if distance > 0 {
                (i128::from(first) + index) as i32
            } else {
                (i128::from(first) - 1 - index) as i32
            }
        }
        PointL::new(
            axis(self.first.x, self.last.x, u, denominator),
            axis(self.first.y, self.last.y, v, denominator),
        )
    }
}

/// NtGdiPlgBlt(HDC, POINT[3], HDC, x, y, width, height, mask, mx, my, color).
/// The background COLORREF only affects monochrome-to-color conversion; both
/// bound source and destination surfaces in this path are already 32-bit.
pub extern "C" fn nt_gdi_plg_blt(frame: &SyscallFrame) -> u64 {
    let Ok(points) = user_copy::read_user_object::<[PointL; 3]>(frame.args[1]) else {
        return 0;
    };
    let width = frame.args[5] as i32;
    let height = frame.args[6] as i32;
    let Some(mask_width) = width.checked_abs().filter(|&value| value != 0) else {
        return 0;
    };
    let Some(mask_height) = height.checked_abs().filter(|&value| value != 0) else {
        return 0;
    };
    let mut table = super::super::dc::get_dc_table();
    let Some(source) = table.get(frame.args[2]) else {
        return 0;
    };
    let Some(source_rect) = SourceRect::new(
        source,
        frame.args[3] as i32,
        frame.args[4] as i32,
        width,
        height,
    ) else {
        return 0;
    };
    let Some(source_surface) = bound_surface(source) else {
        return 0;
    };
    let source_pointer = source_surface as *const GdiSurface;
    let Some(destination) = table.get(frame.args[0]) else {
        return 0;
    };
    if destination.stretch_mode != super::stretch::COLORONCOLOR {
        return 0;
    }
    let result = (|| {
        let parallelogram = Parallelogram::new(destination, points)?;
        let surface = bound_surface(destination)?;
        let mask = if frame.args[7] == 0 {
            None
        } else {
            Some(super::mask::Mask::capture_tile(frame.args[7])?)
        };
        let clip = clipping::effective_region(destination).ok()?;
        let draw = Region::rectangle(parallelogram.bounds)
            .ok()?
            .combine(&clip, RegionCombineMode::Intersection)
            .ok()?;
        let mut pixels = Vec::new();
        pixels
            .try_reserve_exact(super::extended::pixel_count(&draw)?)
            .ok()?;
        for span in draw.rectangles() {
            for y in span.top..span.bottom {
                for x in span.left..span.right {
                    let sample = parallelogram
                        .coordinates(x, y)
                        .and_then(|(u, v, denominator)| {
                            if let Some(mask) = &mask {
                                let mx = i64::from(frame.args[8] as i32)
                                    + (u * i128::from(mask_width) / denominator) as i64;
                                let my = i64::from(frame.args[9] as i32)
                                    + (v * i128::from(mask_height) / denominator) as i64;
                                if !mask.tiled_foreground(mx, my) {
                                    return None;
                                }
                            }
                            let source = source_rect.sample(u, v, denominator);
                            // SAFETY: the DC table guard retains both bindings
                            // throughout capture and commit, even for the same DC.
                            read_pixel(unsafe { &*source_pointer }, source.x, source.y)
                                .map(|color| color & 0x00ff_ffff)
                        });
                    pixels.push(sample);
                }
            }
        }
        let mut pixels = pixels.into_iter();
        for span in draw.rectangles() {
            for y in span.top..span.bottom {
                for x in span.left..span.right {
                    if let Some(color) = pixels.next()? {
                        write_pixel(destination, surface, x, y, color);
                    }
                }
            }
        }
        Some(())
    })();
    u64::from(result.is_some())
}
