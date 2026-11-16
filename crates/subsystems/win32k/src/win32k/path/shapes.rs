//! Curved primitives share path recording, affine transforms and the raster
//! engine. Angles in the native ABI are DWORDs containing IEEE-754 FLOAT bits.

use super::{Entry, GdiPath, PathState, PT_BEZIERTO, PT_LINETO, PT_MOVETO};
use super::super::{clipping, dc, objects::Rect, raster};
use crate::arch::common::syscall_frame::SyscallFrame;
use core::f64::consts::{FRAC_PI_2, TAU};

#[derive(Clone, Copy)]
struct Ellipse {
    center: [f64; 2],
    radius: [f64; 2],
}

impl Ellipse {
    fn point(self, angle: f64) -> [f64; 2] {
        [self.center[0] + self.radius[0] * libm::cos(angle),
            self.center[1] - self.radius[1] * libm::sin(angle)]
    }

    fn tangent(self, angle: f64) -> [f64; 2] {
        [-self.radius[0] * libm::sin(angle), -self.radius[1] * libm::cos(angle)]
    }
}

fn logical_point(point: [f64; 2]) -> Option<dc::PointL> {
    let [x, y] = point.map(|value| libm::floor(value + 0.5));
    if !x.is_finite() || !y.is_finite()
        || x < f64::from(i32::MIN) || x > f64::from(i32::MAX)
        || y < f64::from(i32::MIN) || y > f64::from(i32::MAX)
    {
        return None;
    }
    Some(dc::PointL::new(x as i32, y as i32))
}

fn entry(dc: &dc::DcObject, point: [f64; 2], kind: u8) -> Option<Entry> {
    Some(Entry { point: clipping::logical_point(dc, logical_point(point)?)?, kind })
}

/// Split each sweep into at most quarter-circle cubic segments. Both size and
/// allocation are checked before appending, including multi-turn AngleArc.
fn arc(path: &mut GdiPath, dc: &dc::DcObject, ellipse: Ellipse,
    start: f64, sweep: f64) -> Option<()>
{
    if !start.is_finite() || !sweep.is_finite() { return None; }
    let count = libm::ceil(sweep.abs() / FRAC_PI_2).max(1.0);
    if count > 4096.0 { return None; }
    let count = count as usize;
    if !path.reserve(count.checked_mul(3)?) { return None; }
    let step = sweep / count as f64;
    let factor = 4.0 / 3.0 * libm::tan(step / 4.0);
    for index in 0..count {
        let first = start + index as f64 * step;
        let last = start + (index + 1) as f64 * step;
        let a = ellipse.point(first);
        let b = ellipse.point(last);
        let ta = ellipse.tangent(first);
        let tb = ellipse.tangent(last);
        for point in [[a[0] + factor * ta[0], a[1] + factor * ta[1]],
            [b[0] - factor * tb[0], b[1] - factor * tb[1]], b]
        {
            path.entries.push(entry(dc, point, PT_BEZIERTO)?);
        }
    }
    Some(())
}

fn bounds(frame: &SyscallFrame, offset: usize) -> Rect {
    let mut rect = Rect::new(frame.args[offset] as i32, frame.args[offset + 1] as i32,
        frame.args[offset + 2] as i32, frame.args[offset + 3] as i32);
    rect.normalize();
    rect
}

fn ellipse(rect: Rect, dc: &dc::DcObject) -> Option<Ellipse> {
    let mut right = f64::from(rect.right);
    let mut bottom = f64::from(rect.bottom);
    if dc.graphics_mode == 1 {
        right -= 1.0;
        bottom -= 1.0;
    }
    let radius = [(right - f64::from(rect.left)) / 2.0,
        (bottom - f64::from(rect.top)) / 2.0];
    if radius.iter().any(|&value| value <= 0.0) { return None; }
    Some(Ellipse { center: [f64::from(rect.left) + radius[0],
        f64::from(rect.top) + radius[1]], radius })
}

/// Commit an independently built figure only after capture and geometry work
/// succeed. Standalone shapes preserve the DC's current logical position.
fn finish(dc: &mut dc::DcObject, mut shape: GdiPath, fill: bool,
    position: Option<dc::PointL>) -> bool
{
    if dc.path.state == PathState::Open {
        let device_position = match position {
            Some(point) => match clipping::logical_point(dc, point) {
                Some(point) => Some(point),
                None => return false,
            },
            None => None,
        };
        if !dc.path.reserve(shape.entries.len()) { return false; }
        if let Some(device) = device_position {
            let skip = usize::from(!dc.path.new_figure);
            dc.path.entries.extend_from_slice(&shape.entries[skip..]);
            dc.path.position = device;
            dc.path.new_figure = false;
        } else {
            dc.path.entries.append(&mut shape.entries);
            dc.path.new_figure = true;
        }
    } else if !raster::paint_path(dc, &shape, fill, true) {
        return false;
    }
    if let Some(point) = position {
        dc.cur_x = point.x;
        dc.cur_y = point.y;
    }
    true
}

pub extern "C" fn nt_gdi_ellipse(frame: &SyscallFrame) -> u64 {
    let Some(mut dc) = dc::get_dc(frame.args[0]) else { return 0; };
    let rect = bounds(frame, 1);
    if rect.is_empty() { return 1; }
    let Some(ellipse) = ellipse(rect, &dc) else { return 1; };
    let result = (|| {
        let mut shape = GdiPath::default();
        if !shape.reserve(13) { return None; }
        shape.entries.push(entry(&dc, ellipse.point(0.0), PT_MOVETO)?);
        let sweep = if dc.attributes.arc_direction == 1 { TAU } else { -TAU };
        arc(&mut shape, &dc, ellipse, 0.0, sweep)?;
        shape.close_figure();
        Some(finish(&mut dc, shape, true, None))
    })();
    (result == Some(true)) as u64
}

pub extern "C" fn nt_gdi_round_rect(frame: &SyscallFrame) -> u64 {
    let Some(mut dc) = dc::get_dc(frame.args[0]) else { return 0; };
    let rect = bounds(frame, 1);
    if rect.is_empty() { return 1; }
    let width = (i64::from(frame.args[5] as i32)).abs().min(i64::from(rect.right) - i64::from(rect.left));
    let height = (i64::from(frame.args[6] as i32)).abs().min(i64::from(rect.bottom) - i64::from(rect.top));
    if width == 0 || height == 0 {
        return if dc.path.state == PathState::Open { super::rectangle(&mut dc, rect) }
            else { raster::rectangle(&dc, rect) } as u64;
    }
    let result = (|| {
        let outer = ellipse(rect, &dc)?;
        let radius = [((width as f64 - 1.0) / 2.0).max(0.0).min(outer.radius[0]),
            ((height as f64 - 1.0) / 2.0).max(0.0).min(outer.radius[1])];
        let [cx, cy] = outer.center;
        let [rx, ry] = outer.radius;
        let centers = [[cx + rx - radius[0], cy - ry + radius[1]],
            [cx - rx + radius[0], cy - ry + radius[1]],
            [cx - rx + radius[0], cy + ry - radius[1]],
            [cx + rx - radius[0], cy + ry - radius[1]]];
        let mut shape = GdiPath::default();
        if !shape.reserve(17) { return None; }
        for (index, center) in centers.into_iter().enumerate() {
            let corner = Ellipse { center, radius };
            let angle = index as f64 * FRAC_PI_2;
            shape.entries.push(entry(&dc, corner.point(angle),
                if index == 0 { PT_MOVETO } else { PT_LINETO })?);
            arc(&mut shape, &dc, corner, angle, FRAC_PI_2)?;
        }
        shape.close_figure();
        Some(finish(&mut dc, shape, true, None))
    })();
    (result == Some(true)) as u64
}

pub extern "C" fn nt_gdi_arc_internal(frame: &SyscallFrame) -> u64 {
    let kind = frame.args[0] as u32;
    if kind > 3 { return 0; }
    let Some(mut dc) = dc::get_dc(frame.args[1]) else { return 0; };
    let rect = bounds(frame, 2);
    if rect.is_empty() { return 1; }
    let Some(oval) = ellipse(rect, &dc) else { return 1; };
    let angle = |x, y| libm::atan2((oval.center[1] - f64::from(y)) / oval.radius[1],
        (f64::from(x) - oval.center[0]) / oval.radius[0]);
    let start = angle(frame.args[6] as i32, frame.args[7] as i32);
    let end = angle(frame.args[8] as i32, frame.args[9] as i32);
    let mut sweep = end - start;
    if dc.attributes.arc_direction == 1 {
        if sweep <= 0.0 { sweep += TAU; }
    } else if sweep >= 0.0 { sweep -= TAU; }
    let result = (|| {
        let mut shape = GdiPath::default();
        if !shape.reserve(17) { return None; }
        if kind == 1 {
            shape.entries.push(entry(&dc, [f64::from(dc.cur_x), f64::from(dc.cur_y)], PT_MOVETO)?);
        }
        shape.entries.push(entry(&dc, oval.point(start),
            if kind == 1 { PT_LINETO } else { PT_MOVETO })?);
        arc(&mut shape, &dc, oval, start, sweep)?;
        if kind == 3 { shape.entries.push(entry(&dc, oval.center, PT_LINETO)?); }
        if kind >= 2 { shape.close_figure(); }
        // ArcTo's logical current position is projected on the original
        // bounding ellipse, before GM_COMPATIBLE's device-edge exclusion.
        let position = if kind == 1 {
            let radius = [
                (f64::from(rect.right) - f64::from(rect.left)) / 2.0,
                (f64::from(rect.bottom) - f64::from(rect.top)) / 2.0,
            ];
            let logical = Ellipse {
                center: [f64::from(rect.left) + radius[0], f64::from(rect.top) + radius[1]],
                radius,
            };
            let end = libm::atan2(
                (logical.center[1] - f64::from(frame.args[9] as i32)) / radius[1],
                (f64::from(frame.args[8] as i32) - logical.center[0]) / radius[0],
            );
            Some(logical_point(logical.point(end))?)
        } else {
            None
        };
        Some(finish(&mut dc, shape, kind >= 2, position))
    })();
    (result == Some(true)) as u64
}

pub extern "C" fn nt_gdi_angle_arc(frame: &SyscallFrame) -> u64 {
    let radius = frame.args[3] as i32;
    let start = f32::from_bits(frame.args[4] as u32);
    let sweep = f32::from_bits(frame.args[5] as u32);
    if radius < 0 || !start.is_finite() || !sweep.is_finite() { return 0; }
    let Some(mut dc) = dc::get_dc(frame.args[0]) else { return 0; };
    let oval = Ellipse { center: [f64::from(frame.args[1] as i32), f64::from(frame.args[2] as i32)],
        radius: [f64::from(radius); 2] };
    let start = f64::from(start).to_radians();
    let sweep = f64::from(sweep).to_radians();
    let result = (|| {
        let mut shape = GdiPath::default();
        if !shape.reserve(2) { return None; }
        shape.entries.push(entry(&dc, [f64::from(dc.cur_x), f64::from(dc.cur_y)], PT_MOVETO)?);
        shape.entries.push(entry(&dc, oval.point(start), PT_LINETO)?);
        arc(&mut shape, &dc, oval, start, sweep)?;
        let position = logical_point(oval.point(start + sweep))?;
        Some(finish(&mut dc, shape, false, Some(position)))
    })();
    (result == Some(true)) as u64
}
