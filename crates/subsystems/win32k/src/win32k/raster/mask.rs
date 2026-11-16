//! ROP4 copies with a monochrome mask. Capture the mask and source samples
//! before writing, so shared surfaces and concurrent object deletion are safe.
//! The current device path supports translated 32-bit DCs.

use super::*;
use crate::arch::common::syscall_frame::SyscallFrame;

pub(super) struct Mask {
    bits: Vec<u8>,
    stride: usize,
    x: usize,
    width: usize,
    height: usize,
}

impl Mask {
    fn capture(handle: u64, x: i32, y: i32, width: i32, height: i32) -> Option<Self> {
        if x < 0 || y < 0 {
            return None;
        }
        objects::with_handle_table(|table| {
            let entry = table.get(handle)?;
            let process = super::super::window::input_context::caller_identity().0 as u32;
            if entry.get_type()? != GdiObjectType::Bitmap
                || entry.obj_ptr == 0
                || entry.process_id != 0 && entry.process_id != process
            {
                return None;
            }
            // SAFETY: the object guard pins the live bitmap through the copy.
            let bitmap = unsafe { &*(entry.obj_ptr as *const objects::GdiBitmap) };
            Self::capture_bitmap(bitmap, x, y, width, height)
        })
    }

    /// PlgBlt repeats a mask that is smaller than its source rectangle. Capture
    /// the tile under one object guard, including its storage dimensions.
    pub(super) fn capture_tile(handle: u64) -> Option<Self> {
        objects::with_handle_table(|table| {
            let entry = table.get(handle)?;
            let process = super::super::window::input_context::caller_identity().0 as u32;
            if entry.get_type()? != GdiObjectType::Bitmap
                || entry.obj_ptr == 0
                || entry.process_id != 0 && entry.process_id != process
            {
                return None;
            }
            // SAFETY: the live bitmap remains pinned by the object guard.
            let bitmap = unsafe { &*(entry.obj_ptr as *const objects::GdiBitmap) };
            Self::capture_bitmap(bitmap, 0, 0, bitmap.width, bitmap.height.checked_abs()?)
        })
    }

    fn capture_bitmap(
        bitmap: &objects::GdiBitmap,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    ) -> Option<Self> {
        if width <= 0
            || height <= 0
            || bitmap.planes != 1
            || bitmap.bit_count != 1
            || bitmap.bits.is_null()
            || x.checked_add(width)? > bitmap.width
            || y.checked_add(height)? > bitmap.height.checked_abs()?
        {
            return None;
        }
        let pitch = usize::try_from(bitmap.width_bytes).ok()?;
        let start = x as usize / 8;
        let end = (x as usize).checked_add(width as usize)?.checked_add(7)? / 8;
        if end > pitch {
            return None;
        }
        let stride = end - start;
        let count = stride.checked_mul(height as usize)?;
        if count > MAX_CAPTURE_PIXELS {
            return None;
        }
        let mut bits = Vec::new();
        bits.try_reserve_exact(count).ok()?;
        bits.resize(count, 0);
        for row in 0..height as usize {
            let offset = (y as usize + row).checked_mul(pitch)?.checked_add(start)?;
            if offset.checked_add(stride)? > usize::try_from(bitmap.size).ok()? {
                return None;
            }
            // SAFETY: bounds are checked against bitmap.size, and the
            // independent Vec owns count initialized, disjoint bytes.
            unsafe {
                core::ptr::copy_nonoverlapping(
                    bitmap.bits.add(offset),
                    bits.as_mut_ptr().add(row * stride),
                    stride,
                );
            }
        }
        Some(Self {
            bits,
            stride,
            x: x as usize % 8,
            width: width as usize,
            height: height as usize,
        })
    }

    fn foreground(&self, x: usize, y: usize) -> bool {
        let x = self.x + x;
        self.bits[y * self.stride + x / 8] & (0x80 >> (x % 8)) != 0
    }

    pub(super) fn tiled_foreground(&self, x: i64, y: i64) -> bool {
        self.foreground(
            x.rem_euclid(self.width as i64) as usize,
            y.rem_euclid(self.height as i64) as usize,
        )
    }
}

pub extern "C" fn nt_gdi_mask_blt(frame: &SyscallFrame) -> u64 {
    let foreground = (frame.args[11] as u32) & 0x00ff_ffff;
    let background = ((frame.args[11] as u32) >> 24) << 16;
    let masked = frame.args[8] != 0;
    let needs_source = uses_source(foreground) || masked && uses_source(background);
    let mut table = super::super::dc::get_dc_table();
    let mut source = DcObject::new();
    if needs_source {
        let Some(dc) = table.get(frame.args[5]) else {
            return 0;
        };
        source.surface = dc.surface;
        source.world_transform = dc.world_transform;
        source.viewport_ext = dc.viewport_ext;
        source.window_ext = dc.window_ext;
        source.viewport_org = dc.viewport_org;
        source.window_org = dc.window_org;
    }
    let Some(destination) = table.get(frame.args[0]) else {
        return 0;
    };
    let width = frame.args[3] as i32;
    let height = frame.args[4] as i32;
    if !masked {
        return u64::from(bit_blt(
            destination,
            frame.args[1] as i32,
            frame.args[2] as i32,
            width,
            height,
            needs_source.then_some(&source),
            frame.args[6] as i32,
            frame.args[7] as i32,
            foreground,
        ));
    }
    if width < 0 || height < 0 {
        return 0;
    }
    if width == 0 || height == 0 {
        return 1;
    }
    let result = (|| {
        let offset = translation(destination)?;
        let x = (frame.args[1] as i32).checked_add(offset.x)?;
        let y = (frame.args[2] as i32).checked_add(offset.y)?;
        let requested = Rect::new(x, y, x.checked_add(width)?, y.checked_add(height)?);
        let surface = bound_surface(destination)?;
        let mask = Mask::capture(
            frame.args[8],
            frame.args[9] as i32,
            frame.args[10] as i32,
            width,
            height,
        )?;
        let source_data = if needs_source {
            let offset = translation(&source)?;
            Some((
                bound_surface(&source)?,
                (frame.args[6] as i32).checked_add(offset.x)?,
                (frame.args[7] as i32).checked_add(offset.y)?,
            ))
        } else {
            None
        };
        let pattern = if uses_pattern(foreground) || uses_pattern(background) {
            brush_sample(destination, destination.brush)?
        } else {
            Some(BrushSample::solid(0))
        };
        let clip = clipping::effective_region(destination).ok()?;
        let draw = Region::rectangle(requested)
            .ok()?
            .combine(&clip, RegionCombineMode::Intersection)
            .ok()?;
        let mut pixels = Vec::new();
        pixels
            .try_reserve_exact(super::extended::pixel_count(&draw)?)
            .ok()?;
        for span in draw.rectangles() {
            for dy in span.top..span.bottom {
                for dx in span.left..span.right {
                    let column = i64::from(dx) - i64::from(x);
                    let row = i64::from(dy) - i64::from(y);
                    let operation = if mask.foreground(column as usize, row as usize) {
                        foreground
                    } else {
                        background
                    };
                    let sample = if uses_source(operation) {
                        let (source, sx, sy) = source_data?;
                        read_pixel(
                            source,
                            i32::try_from(i64::from(sx) + column).ok()?,
                            i32::try_from(i64::from(sy) + row).ok()?,
                        )
                    } else {
                        Some(0)
                    };
                    let brush = if uses_pattern(operation) {
                        pattern.and_then(|brush| brush.at(dx, dy))
                    } else {
                        Some(0)
                    };
                    pixels.push(sample.zip(brush).map(|(sample, brush)| {
                        rop3(
                            read_pixel(surface, dx, dy).unwrap_or(0),
                            sample,
                            brush,
                            operation,
                        )
                    }));
                }
            }
        }
        let mut pixels = pixels.into_iter();
        for span in draw.rectangles() {
            for dy in span.top..span.bottom {
                for dx in span.left..span.right {
                    if let Some(pixel) = pixels.next()? {
                        write_pixel(destination, surface, dx, dy, pixel);
                    }
                }
            }
        }
        Some(())
    })();
    u64::from(result.is_some())
}
