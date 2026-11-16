//! Source-over blending on bound 32-bit DC surfaces.
//!
//! BLENDFUNCTION is passed by value in a DWORD. Per-pixel sources contain
//! premultiplied channels; multiplying those channels by their alpha again
//! would produce dark edges. Scaling uses COLORONCOLOR regardless of the DC's
//! stretch mode. Source clipping does not affect the copy.

use super::*;
use crate::arch::common::syscall_frame::SyscallFrame;

#[derive(Clone, Copy)]
struct Blend {
    constant: u32,
    per_pixel: bool,
}

impl Blend {
    fn decode(value: u32) -> Option<Self> {
        // AC_SRC_OVER = 0, BlendFlags = 0, AlphaFormat = 0 or AC_SRC_ALPHA.
        if value & 0xfe00_ffff != 0 {
            return None;
        }
        Some(Self {
            constant: (value >> 16) & 255,
            per_pixel: value >> 24 == 1,
        })
    }

    fn pixel(self, destination: u32, source: u32, argb: bool) -> u32 {
        let source_alpha = if self.per_pixel { source >> 24 } else { 255 };
        let alpha = (source_alpha * self.constant + 127) / 255;
        let mut result = 0;
        for shift in [0, 8, 16] {
            let foreground = (source >> shift) & 255;
            let background = (destination >> shift) & 255;
            let channel = if self.per_pixel {
                (foreground * self.constant + 127) / 255 + (background * (255 - alpha) + 127) / 255
            } else {
                (foreground * self.constant + background * (255 - alpha) + 127) / 255
            };
            result |= channel.min(255) << shift;
        }
        if argb {
            let background = destination >> 24;
            result |= (alpha + (background * (255 - alpha) + 127) / 255).min(255) << 24;
        }
        result
    }
}

fn intersects(first: Rect, second: Rect) -> bool {
    first.left < second.right
        && second.left < first.right
        && first.top < second.bottom
        && second.top < first.bottom
}

pub extern "C" fn nt_gdi_alpha_blend(frame: &SyscallFrame) -> u64 {
    let Some(blend) = Blend::decode(frame.args[10] as u32) else {
        return 0;
    };
    // A non-default color transform requires the ICM engine.
    if frame.args[11] != 0 {
        return 0;
    }
    let width = frame.args[3] as i32;
    let height = frame.args[4] as i32;
    let source_width = frame.args[8] as i32;
    let source_height = frame.args[9] as i32;
    if [width, height, source_width, source_height]
        .iter()
        .any(|&value| value < 0)
    {
        return 0;
    }
    let mut table = super::super::dc::get_dc_table();
    let Some(source_dc) = table.get(frame.args[5]) else {
        return 0;
    };
    let Some(source_surface) = bound_surface(source_dc) else {
        return 0;
    };
    let source_pointer = source_surface as *const GdiSurface;
    if width == 0 || height == 0 || source_width == 0 || source_height == 0 {
        return u64::from(table.get(frame.args[0]).is_some());
    }
    let Some(source_rect) = super::extended::positive_rect(
        source_dc,
        frame.args[6] as i32,
        frame.args[7] as i32,
        source_width,
        source_height,
    ) else {
        return 0;
    };
    if source_rect.left < 0
        || source_rect.top < 0
        || source_rect.right > source_surface.width
        || source_rect.bottom > source_surface.height
    {
        return 0;
    }
    let Some(destination) = table.get(frame.args[0]) else {
        return 0;
    };
    let result = (|| {
        let destination_rect = super::extended::positive_rect(
            destination,
            frame.args[1] as i32,
            frame.args[2] as i32,
            width,
            height,
        )?;
        let destination_surface = bound_surface(destination)?;
        // SAFETY: the DC table guard pins both bindings through the entire
        // operation. The source is only read, and overlapping copies fail.
        let source_surface = unsafe { &*source_pointer };
        if source_surface.bits == destination_surface.bits
            && intersects(source_rect, destination_rect)
        {
            return None;
        }
        let clip = clipping::effective_region(destination).ok()?;
        let draw = Region::rectangle(destination_rect)
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
                    let sx = i64::from(source_rect.left)
                        + i64::from(x - destination_rect.left) * i64::from(source_rect.width())
                            / i64::from(destination_rect.width());
                    let sy = i64::from(source_rect.top)
                        + i64::from(y - destination_rect.top) * i64::from(source_rect.height())
                            / i64::from(destination_rect.height());
                    let source = read_pixel(
                        source_surface,
                        i32::try_from(sx).ok()?,
                        i32::try_from(sy).ok()?,
                    )?;
                    let old = read_pixel(destination_surface, x, y)?;
                    pixels.push(blend.pixel(
                        old,
                        source,
                        destination_surface.format == PIXEL_FORMAT_32BPP_ARGB,
                    ));
                }
            }
        }
        let mut pixels = pixels.into_iter();
        for span in draw.rectangles() {
            for y in span.top..span.bottom {
                for x in span.left..span.right {
                    write_pixel(destination, destination_surface, x, y, pixels.next()?);
                }
            }
        }
        Some(())
    })();
    u64::from(result.is_some())
}
