//! Stretch copies on the existing 32-bit DC surfaces.
//!
//! Signed extents retain their direction through the DC mapping. Capture all
//! source samples before writing, so scaled copies also work on the same DC.
//! HALFTONE and capture of layered windows require additional engine support.

use super::*;
use crate::arch::common::syscall_frame::SyscallFrame;

pub const BLACKONWHITE: i32 = 1;
pub const WHITEONBLACK: i32 = 2;
pub const COLORONCOLOR: i32 = 3;

#[derive(Clone, Copy)]
struct Axis {
    start: i32,
    end: i32,
}

impl Axis {
    fn length(self) -> i64 {
        (i64::from(self.end) - i64::from(self.start)).abs()
    }

    fn index(self, coordinate: i32) -> i64 {
        if self.end >= self.start {
            i64::from(coordinate) - i64::from(self.start)
        } else {
            i64::from(self.start) - 1 - i64::from(coordinate)
        }
    }

    fn coordinate(self, index: i64) -> Option<i32> {
        let value = if self.end >= self.start {
            i64::from(self.start) + index
        } else {
            i64::from(self.start) - 1 - index
        };
        i32::try_from(value).ok()
    }

    fn samples(self, destination: Self, coordinate: i32, mode: i32) -> (i64, i64) {
        let index = destination.index(coordinate);
        // i128 also covers transformed endpoints spanning the entire i32 range.
        let scale = |index: i64| {
            (i128::from(index) * i128::from(self.length()) / i128::from(destination.length()))
                as i64
        };
        let first = scale(index);
        let last = if mode != COLORONCOLOR && self.length() > destination.length() {
            scale(index + 1).max(first + 1)
        } else {
            first + 1
        };
        (first, last)
    }
}

#[derive(Clone, Copy)]
struct MappedRect {
    x: Axis,
    y: Axis,
}

impl MappedRect {
    fn new(dc: &DcObject, x: i32, y: i32, width: i32, height: i32) -> Option<Self> {
        if dc.world_transform.m12 != 0.0 || dc.world_transform.m21 != 0.0 {
            return None;
        }
        let first = clipping::logical_point(dc, PointL::new(x, y))?;
        let last = clipping::logical_point(
            dc,
            PointL::new(x.checked_add(width)?, y.checked_add(height)?),
        )?;
        Some(Self {
            x: Axis {
                start: first.x,
                end: last.x,
            },
            y: Axis {
                start: first.y,
                end: last.y,
            },
        })
    }

    fn empty(self) -> bool {
        self.x.length() == 0 || self.y.length() == 0
    }

    fn bounds(self) -> Rect {
        Rect::new(
            self.x.start.min(self.x.end),
            self.y.start.min(self.y.end),
            self.x.start.max(self.x.end),
            self.y.start.max(self.y.end),
        )
    }
}

fn sample(
    surface: &GdiSurface,
    source: MappedRect,
    destination: MappedRect,
    point: PointL,
    mode: i32,
    work: &mut usize,
) -> Option<Option<u32>> {
    let (left, right) = source.x.samples(destination.x, point.x, mode);
    let (top, bottom) = source.y.samples(destination.y, point.y, mode);
    let count = usize::try_from((right - left).checked_mul(bottom - top)?).ok()?;
    *work = work.checked_add(count)?;
    if *work > MAX_CAPTURE_PIXELS {
        return None;
    }
    let mut result = if mode == BLACKONWHITE { 0xffffff } else { 0 };
    for row in top..bottom {
        for column in left..right {
            let x = source.x.coordinate(column)?;
            let y = source.y.coordinate(row)?;
            let Some(pixel) = read_pixel(surface, x, y) else {
                return Some(None);
            };
            if mode == BLACKONWHITE {
                result &= pixel;
            } else {
                result |= pixel;
            }
        }
    }
    Some(Some(result))
}

pub extern "C" fn nt_gdi_stretch_blt(frame: &SyscallFrame) -> u64 {
    let operation = frame.args[10] as u32;
    if operation & 0x4000_0000 != 0 {
        return 0;
    }
    let mut table = super::super::dc::get_dc_table();
    // Snapshot only the source mapping; the table guard pins both bindings.
    let source = if uses_source(operation) {
        let Some(dc) = table.get(frame.args[5]) else {
            return 0;
        };
        let Some(rect) = MappedRect::new(
            dc,
            frame.args[6] as i32,
            frame.args[7] as i32,
            frame.args[8] as i32,
            frame.args[9] as i32,
        ) else {
            return 0;
        };
        let Some(surface) = bound_surface(dc) else {
            return 0;
        };
        Some((rect, surface as *const GdiSurface))
    } else {
        None
    };
    let Some(destination) = table.get(frame.args[0]) else {
        return 0;
    };
    let Some(rect) = MappedRect::new(
        destination,
        frame.args[1] as i32,
        frame.args[2] as i32,
        frame.args[3] as i32,
        frame.args[4] as i32,
    ) else {
        return 0;
    };
    // SAFETY: the table guard pins both bindings through capture and commit.
    let source = source.map(|(rect, pointer)| (rect, unsafe { &*pointer }));
    let result = render(
        destination,
        rect,
        source,
        operation,
        destination.stretch_mode,
    );
    u64::from(result.is_some())
}

/// Draw a captured DIB through the same filtering, clipping and ROP pipeline.
pub(super) fn draw_dib(
    destination: &DcObject,
    surface: &GdiSurface,
    target: [i32; 4],
    input: [i32; 4],
    operation: u32,
) -> Option<()> {
    let rect = MappedRect::new(destination, target[0], target[1], target[2], target[3])?;
    let source = MappedRect {
        x: Axis {
            start: input[0],
            end: input[0].checked_add(input[2])?,
        },
        y: Axis {
            start: input[1],
            end: input[1].checked_add(input[3])?,
        },
    };
    render(
        destination,
        rect,
        uses_source(operation).then_some((source, surface)),
        operation,
        destination.stretch_mode,
    )
}

/// SetDIBitsToDevice maps the destination origin, but its extents remain device
/// pixels. It does not apply the DC's stretching filter or scale the source.
pub(super) fn draw_dib_device(
    destination: &DcObject,
    surface: &GdiSurface,
    target: [i32; 4],
    input: PointL,
    transform_origin: bool,
) -> Option<()> {
    let point = PointL::new(target[0], target[1]);
    let origin = if transform_origin {
        clipping::logical_point(destination, point)?
    } else {
        point
    };
    let rectangle = |origin: PointL| {
        Some(MappedRect {
            x: Axis {
                start: origin.x,
                end: origin.x.checked_add(target[2])?,
            },
            y: Axis {
                start: origin.y,
                end: origin.y.checked_add(target[3])?,
            },
        })
    };
    render(
        destination,
        rectangle(origin)?,
        Some((rectangle(input)?, surface)),
        0x00cc_0020,
        COLORONCOLOR,
    )
}

fn render(
    destination: &DcObject,
    rect: MappedRect,
    source: Option<(MappedRect, &GdiSurface)>,
    operation: u32,
    mode: i32,
) -> Option<()> {
    if source.is_some() && !matches!(mode, BLACKONWHITE | WHITEONBLACK | COLORONCOLOR) {
        return None;
    }
    let surface = bound_surface(destination)?;
    if rect.empty() || source.is_some_and(|(rect, _)| rect.empty()) {
        return Some(());
    }
    let clip = clipping::effective_region(destination).ok()?;
    let draw = Region::rectangle(rect.bounds())
        .ok()?
        .combine(&clip, RegionCombineMode::Intersection)
        .ok()?;
    let brush = if uses_pattern(operation) {
        brush_sample(destination, destination.brush)?
    } else {
        Some(BrushSample::solid(0))
    };
    let count = super::extended::pixel_count(&draw)?;
    let mut pixels = Vec::new();
    pixels.try_reserve_exact(count).ok()?;
    let mut work = 0;
    for span in draw.rectangles() {
        for y in span.top..span.bottom {
            for x in span.left..span.right {
                let pixel = if let Some((source_rect, pointer)) = source {
                    sample(
                        pointer,
                        source_rect,
                        rect,
                        PointL::new(x, y),
                        mode,
                        &mut work,
                    )?
                } else {
                    Some(0)
                };
                pixels.push(pixel);
            }
        }
    }
    let mut pixels = pixels.into_iter();
    for span in draw.rectangles() {
        for y in span.top..span.bottom {
            for x in span.left..span.right {
                if let (Some(Some(source)), Some(old), Some(pattern)) = (
                    pixels.next(),
                    read_pixel(surface, x, y),
                    brush.and_then(|b| b.at(x, y)),
                ) {
                    write_pixel(
                        destination,
                        surface,
                        x,
                        y,
                        rop3(old, source, pattern, operation),
                    );
                }
            }
        }
    }
    Some(())
}
