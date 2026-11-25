//! Text output over local SFNT coverage and the clipped DC surface. All user
//! buffers and glyph images are captured before the first pixel is changed.
//! Font shaping, font linking, and text paths still need separate backends;
//! requests requiring those operations fail rather than silently lose text.
use super::super::region_geometry::{Region, RegionCombineMode};
use super::super::{
    clipping, dc,
    objects::{FontQuality, Rect},
    path::PathState,
    polygon, raster, transforms,
};
use super::{capture_units, outline, FontSelection, MAX_CHARACTERS};
use crate::arch::common::syscall_frame::SyscallFrame;
use crate::mm::user_copy::{copy_from_user, read_user_object};
use alloc::vec::Vec;

const ETO_OPAQUE: u32 = 2;
const ETO_CLIPPED: u32 = 4;
const ETO_GLYPH_INDEX: u32 = 0x10;
const ETO_PDY: u32 = 0x2000;
const MAX_CAPTURE_BYTES: usize = 16 * 1024 * 1024;

struct TextRun {
    position: dc::PointL,
    flags: u32,
    rectangle: Option<Rect>,
    units: Vec<u16>,
    distances: Option<Vec<[i32; 2]>>,
}

impl TextRun {
    fn capture(
        position: dc::PointL,
        flags: u32,
        rectangle: Option<Rect>,
        string: u64,
        count: usize,
        distances: u64,
    ) -> Option<Self> {
        // ETO_IGNORELANGUAGE has no effect for this cmap-only backend. Bidi and
        // numeric substitution require shaping and are not accepted here.
        if flags & !(ETO_OPAQUE | ETO_CLIPPED | ETO_GLYPH_INDEX | ETO_PDY | 0x1000) != 0
            || count > MAX_CHARACTERS
        {
            return None;
        }
        let units = capture_units(string, count)?;
        let distances = if distances == 0 || count == 0 {
            None
        } else {
            let step = if flags & ETO_PDY != 0 { 8 } else { 4 };
            let mut bytes = Vec::new();
            bytes.try_reserve_exact(count.checked_mul(step)?).ok()?;
            bytes.resize(count * step, 0);
            copy_from_user(&mut bytes, distances).ok()?;
            let mut values = Vec::new();
            values.try_reserve_exact(count).ok()?;
            for value in bytes.chunks_exact(step) {
                let x = i32::from_le_bytes(value[..4].try_into().ok()?);
                let y = if step == 8 {
                    i32::from_le_bytes(value[4..8].try_into().ok()?)
                } else {
                    0
                };
                values.push([x, y]);
            }
            Some(values)
        };
        Some(Self {
            position,
            flags,
            rectangle,
            units,
            distances,
        })
    }
}

struct Image {
    origin: dc::PointL,
    width: usize,
    height: usize,
    stride: usize,
    coverage: Vec<u8>,
}

struct PreparedRun {
    images: Vec<Image>,
    backgrounds: Vec<Region>,
    clip: Region,
    current_position: Option<dc::PointL>,
}

fn device_point(dc: &dc::DcObject, [x, y]: [f64; 2]) -> Option<dc::PointL> {
    let transform = transforms::world_to_device(dc)?;
    Some(dc::PointL::new(
        transforms::nearest_integer(
            x * f64::from(transform.m11) + y * f64::from(transform.m21) + f64::from(transform.dx),
        )?,
        transforms::nearest_integer(
            x * f64::from(transform.m12) + y * f64::from(transform.m22) + f64::from(transform.dy),
        )?,
    ))
}

fn logical_rectangle(dc: &dc::DcObject, rectangle: Rect) -> Option<Region> {
    // Empty/reversed ETO rectangles paint nothing; they are not normalized.
    if rectangle.left >= rectangle.right || rectangle.top >= rectangle.bottom {
        return Some(Region::default());
    }
    let points = [
        [f64::from(rectangle.left), f64::from(rectangle.top)],
        [f64::from(rectangle.right), f64::from(rectangle.top)],
        [f64::from(rectangle.right), f64::from(rectangle.bottom)],
        [f64::from(rectangle.left), f64::from(rectangle.bottom)],
    ];
    let mut transformed = [dc::PointL::default(); 4];
    for (output, input) in transformed.iter_mut().zip(points) {
        *output = device_point(dc, input)?;
    }
    polygon::region_from_contours(&[&transformed], 2).ok()
}

fn prepare(dc: &dc::DcObject, run: &TextRun, capture_bytes: &mut usize) -> Option<PreparedRun> {
    if dc.path.state == PathState::Open || dc.text_align & 0x100 != 0 {
        return None;
    }
    let selection = FontSelection::from_dc(dc)?;
    let font = selection.realize()?;
    // Decorations require outline-derived thickness and placement; accepting
    // them here would silently omit part of the selected font's appearance.
    if font.requested.underline != 0 || font.requested.strike_out != 0 {
        return None;
    }
    let ascent = font.vertical(i64::from(font.description.ascent))?;
    let descent = font.vertical(i64::from(font.description.descent))?;
    let angle = f64::from(font.requested.escapement % 3600) * core::f64::consts::PI / 1800.0;
    let (cosine, sine) = (libm::cos(angle), libm::sin(angle));
    let rotate = |[x, y]: [f64; 2]| [x * cosine + y * sine, -x * sine + y * cosine];
    let position = if dc.text_align & dc::TA_UPDATECP != 0 {
        dc::PointL::new(dc.cur_x, dc.cur_y)
    } else {
        run.position
    };
    let mut offsets = Vec::new();
    offsets.try_reserve_exact(run.units.len()).ok()?;
    let mut advance = [0i64; 2];
    let mut design_advance = 0i64;
    let mut breaks_used = 0i32;
    let breaks = dc.attributes.text_justification_breaks;
    for (index, unit) in run.units.iter().copied().enumerate() {
        offsets.push(advance);
        let delta = if let Some(distances) = &run.distances {
            distances[index]
        } else {
            let glyph = font.glyph(unit, run.flags & ETO_GLYPH_INDEX != 0);
            let before = font.horizontal(design_advance)?;
            design_advance =
                design_advance.checked_add(i64::from(font.font.glyph_metrics(glyph)?.advance))?;
            let mut width = font.horizontal(design_advance)?.checked_sub(before)?;
            let is_break = if run.flags & ETO_GLYPH_INDEX != 0 {
                glyph == font.glyph(u16::from(b' '), false)
            } else {
                unit == u16::from(b' ')
            };
            if is_break && breaks_used < breaks {
                let extra = dc.attributes.text_justification_extra;
                let remainder = extra % breaks;
                let adjustment = extra / breaks
                    + if breaks_used < remainder.abs() {
                        remainder.signum()
                    } else {
                        0
                    };
                width = width.checked_add(adjustment)?;
                breaks_used += 1;
            }
            [width, 0]
        };
        for axis in 0..2 {
            advance[axis] = advance[axis].checked_add(i64::from(delta[axis]))?;
        }
    }
    let total = rotate(advance.map(|value| value as f64));
    let horizontal = dc.text_align & 6;
    let factor = match horizontal {
        dc::TA_LEFT => 0.0,
        dc::TA_RIGHT => 1.0,
        dc::TA_CENTER => 0.5,
        _ => return None,
    };
    let mut baseline = [
        f64::from(position.x) - total[0] * factor,
        f64::from(position.y) - total[1] * factor,
    ];
    let vertical = match dc.text_align & 24 {
        dc::TA_TOP => ascent,
        dc::TA_BOTTOM => -descent,
        dc::TA_BASELINE => 0,
        _ => return None,
    };
    let shift = rotate([0.0, f64::from(vertical)]);
    baseline[0] += shift[0];
    baseline[1] += shift[1];
    let current_position = if dc.text_align & dc::TA_UPDATECP != 0 && horizontal != dc::TA_CENTER {
        let factor = if horizontal == dc::TA_RIGHT {
            -1.0
        } else {
            1.0
        };
        Some(dc::PointL::new(
            transforms::nearest_integer(f64::from(position.x) + factor * total[0])?,
            transforms::nearest_integer(f64::from(position.y) + factor * total[1])?,
        ))
    } else {
        None
    };
    let mut clip = clipping::effective_region(dc).ok()?;
    let mut backgrounds = Vec::new();
    backgrounds.try_reserve_exact(2).ok()?;
    if let Some(rectangle) = run.rectangle {
        let rectangle = logical_rectangle(dc, rectangle)?;
        if run.flags & ETO_CLIPPED != 0 {
            clip = clip
                .combine(&rectangle, RegionCombineMode::Intersection)
                .ok()?;
        }
        if run.flags & ETO_OPAQUE != 0 {
            backgrounds.push(
                rectangle
                    .combine(&clip, RegionCombineMode::Intersection)
                    .ok()?,
            );
        }
    }
    if dc.bk_mode == dc::OPAQUE && !run.units.is_empty() {
        let corners = [
            [0.0, -f64::from(ascent)],
            [advance[0] as f64, advance[1] as f64 - f64::from(ascent)],
            [advance[0] as f64, advance[1] as f64 + f64::from(descent)],
            [0.0, f64::from(descent)],
        ];
        let mut points = [dc::PointL::default(); 4];
        for (output, input) in points.iter_mut().zip(corners) {
            let point = rotate(input);
            *output = device_point(dc, [baseline[0] + point[0], baseline[1] + point[1]])?;
        }
        let region = polygon::region_from_contours(&[&points], 2).ok()?;
        backgrounds.push(
            region
                .combine(&clip, RegionCombineMode::Intersection)
                .ok()?,
        );
    }
    let mut images = Vec::new();
    images.try_reserve_exact(run.units.len()).ok()?;
    // Text quality chooses the drawing bitmap only. GetGlyphOutline keeps its
    // caller-selected bitmap format, independently of the logical font quality.
    let bitmap_format = if font.requested.quality == FontQuality::NonAntialiased as u8 { 1 } else { 6 };
    for (unit, offset) in run.units.iter().copied().zip(offsets) {
        let format = if run.flags & ETO_GLYPH_INDEX != 0 {
            bitmap_format | 0x80
        } else {
            bitmap_format
        };
        let glyph = outline::realize_font(&font, dc, unit, format, [65536, 0, 0, 65536], false)?;
        let (stride, size) = glyph.bitmap_layout(bitmap_format)?;
        *capture_bytes = capture_bytes.checked_add(size)?;
        if *capture_bytes > MAX_CAPTURE_BYTES {
            return None;
        }
        let coverage = glyph.bitmap(bitmap_format, stride, size)?;
        let offset = rotate(offset.map(|value| value as f64));
        let origin = device_point(dc, [baseline[0] + offset[0], baseline[1] + offset[1]])?;
        let [left, bottom, right, top] = glyph.bounds;
        let width = usize::try_from(right.checked_sub(left)?).ok()?;
        let height = usize::try_from(top.checked_sub(bottom)?).ok()?;
        let (stride, coverage) = if bitmap_format == 1 {
            // The monochrome backend emits DWORD-aligned rows, high bit first.
            // Expand only the glyph width, excluding its packed row padding.
            let expanded_size = width.checked_mul(height)?;
            *capture_bytes = capture_bytes.checked_add(expanded_size)?;
            if *capture_bytes > MAX_CAPTURE_BYTES {
                return None;
            }
            let mut expanded = Vec::new();
            expanded.try_reserve_exact(expanded_size).ok()?;
            expanded.resize(expanded_size, 0);
            for row in 0..height {
                for column in 0..width {
                    if coverage[row * stride + column / 8] & (0x80 >> (column % 8)) != 0 {
                        expanded[row * width + column] = 64;
                    }
                }
            }
            (width, expanded)
        } else {
            (stride, coverage)
        };
        images.push(Image {
            origin: dc::PointL::new(origin.x.checked_add(left)?, origin.y.checked_sub(top)?),
            width,
            height,
            stride,
            coverage,
        });
    }
    Some(PreparedRun {
        images,
        backgrounds,
        clip,
        current_position,
    })
}

fn draw(hdc: u64, runs: &[TextRun]) -> Option<()> {
    let mut dc = dc::get_dc(hdc)?;
    let mut prepared = Vec::new();
    prepared.try_reserve_exact(runs.len()).ok()?;
    let mut bytes = 0;
    // Stage current-position changes locally: failed capture/layout must leave
    // both pixels and the caller's current position untouched.
    let original_position = (dc.cur_x, dc.cur_y);
    let result = (|| {
        for run in runs {
            let run = prepare(&dc, run, &mut bytes)?;
            if let Some(position) = run.current_position {
                dc.cur_x = position.x;
                dc.cur_y = position.y;
            }
            prepared.push(run);
        }
        Some(())
    })();
    let final_position = (dc.cur_x, dc.cur_y);
    (dc.cur_x, dc.cur_y) = original_position;
    result?;
    let mut painter = raster::CoveragePainter::new(&dc)?;
    for run in prepared {
        for background in run.backgrounds {
            painter.fill(&background, dc.bg_color);
        }
        for image in run.images {
            painter.paint(
                &run.clip,
                image.origin,
                image.width,
                image.height,
                image.stride,
                &image.coverage,
                dc.text_color,
            );
        }
    }
    drop(painter);
    (dc.cur_x, dc.cur_y) = final_position;
    Some(())
}

pub fn draw_text_layout(hdc: u64, rectangle: Option<[i32; 4]>, lines: &[(i32, i32, &[u16])]) -> bool {
    let rectangle = rectangle.map(|[left, top, right, bottom]| Rect { left, top, right, bottom });
    let mut runs = Vec::new();
    if runs.try_reserve_exact(lines.len()).is_err() {
        return false;
    }
    for &(x, y, text) in lines {
        if text.is_empty() {
            continue;
        }
        let mut units = Vec::new();
        if text.len() > MAX_CHARACTERS || units.try_reserve_exact(text.len()).is_err() {
            return false;
        }
        units.extend_from_slice(text);
        runs.push(TextRun {
            position: dc::PointL::new(x, y),
            flags: if rectangle.is_some() { ETO_CLIPPED } else { 0 },
            rectangle,
            units,
            distances: None,
        });
    }
    draw(hdc, &runs).is_some()
}

pub extern "C" fn nt_gdi_ext_text_out(frame: &SyscallFrame) -> u64 {
    let result = (|| {
        let flags = frame.args[3] as u32;
        let rectangle = if frame.args[4] != 0 && flags & (ETO_CLIPPED | ETO_OPAQUE) != 0 {
            Some(read_user_object(frame.args[4]).ok()?)
        } else {
            None
        };
        let run = TextRun::capture(
            dc::PointL::new(frame.args[1] as i32, frame.args[2] as i32),
            flags,
            rectangle,
            frame.args[5],
            frame.args[6] as u32 as usize,
            frame.args[7],
        )?;
        draw(frame.args[0], &[run])
    })();
    result.is_some() as u64
}

pub extern "C" fn nt_gdi_set_text_justification(frame: &SyscallFrame) -> u64 {
    let breaks = frame.args[2] as i32;
    if breaks < 0 {
        return 0;
    }
    let Some(mut dc) = dc::get_dc(frame.args[0]) else {
        return 0;
    };
    dc.attributes.text_justification_extra = if breaks == 0 { 0 } else { frame.args[1] as i32 };
    dc.attributes.text_justification_breaks = breaks;
    1
}

/// Native POLYTEXTW has four-byte alignment gaps before its pointer fields on
/// the 64-bit ABI. Decode captured bytes instead of reading Rust struct padding.
pub extern "C" fn nt_gdi_poly_text_out(frame: &SyscallFrame) -> u64 {
    let result = (|| {
        let count = frame.args[2] as u32 as usize;
        if count > MAX_CHARACTERS {
            return None;
        }
        let mut descriptors = Vec::new();
        descriptors.try_reserve_exact(count.checked_mul(56)?).ok()?;
        descriptors.resize(count * 56, 0);
        if count != 0 {
            copy_from_user(&mut descriptors, frame.args[1]).ok()?;
        }
        let mut runs = Vec::new();
        runs.try_reserve_exact(count).ok()?;
        let mut total = 0usize;
        for item in descriptors.chunks_exact(56) {
            let int = |offset| {
                Some(i32::from_le_bytes(
                    item.get(offset..offset + 4)?.try_into().ok()?,
                ))
            };
            let pointer = |offset| {
                Some(u64::from_le_bytes(
                    item.get(offset..offset + 8)?.try_into().ok()?,
                ))
            };
            let count = int(8)? as u32 as usize;
            total = total.checked_add(count)?;
            if total > MAX_CHARACTERS {
                return None;
            }
            let flags = int(24)? as u32;
            let rectangle = if flags & (ETO_OPAQUE | ETO_CLIPPED) != 0 {
                Some(Rect::new(int(28)?, int(32)?, int(36)?, int(40)?))
            } else {
                None
            };
            runs.push(TextRun::capture(
                dc::PointL::new(int(0)?, int(4)?),
                flags,
                rectangle,
                pointer(16)?,
                count,
                pointer(48)?,
            )?);
        }
        draw(frame.args[0], &runs)
    })();
    u64::from(result.is_some())
}
