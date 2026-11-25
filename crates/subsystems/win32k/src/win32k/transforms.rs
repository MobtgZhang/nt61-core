//! GDI affine transforms and native coordinate services.
//!
//! XFORM uses row vectors: x' = x*m11 + y*m21 + dx. Compose(a,b)
//! therefore applies a first and b second, including their translations.
//! User arrays are captured before writing so overlapping buffers work.

extern crate alloc;

use super::dc::{self, DcObject, PointL, Xform};
use crate::arch::common::syscall_frame::SyscallFrame;
use crate::mm::user_copy;
use alloc::vec::Vec;

#[path = "transforms/metafile.rs"]
pub mod metafile;

const MAX_POINTS: usize = 65_536;

pub fn finite(transform: Xform) -> bool {
    [
        transform.m11,
        transform.m12,
        transform.m21,
        transform.m22,
        transform.dx,
        transform.dy,
    ]
    .into_iter()
    .all(f32::is_finite)
}

pub fn compose(first: Xform, second: Xform) -> Option<Xform> {
    if !finite(first) || !finite(second) {
        return None;
    }
    let result = Xform {
        m11: first.m11 * second.m11 + first.m12 * second.m21,
        m12: first.m11 * second.m12 + first.m12 * second.m22,
        m21: first.m21 * second.m11 + first.m22 * second.m21,
        m22: first.m21 * second.m12 + first.m22 * second.m22,
        dx: first.dx * second.m11 + first.dy * second.m21 + second.dx,
        dy: first.dx * second.m12 + first.dy * second.m22 + second.dy,
    };
    finite(result).then_some(result)
}

pub fn inverse(transform: Xform) -> Option<Xform> {
    if !finite(transform) {
        return None;
    }
    let determinant = f64::from(transform.m11) * f64::from(transform.m22)
        - f64::from(transform.m12) * f64::from(transform.m21);
    if determinant == 0.0 || !determinant.is_finite() {
        return None;
    }
    let result = Xform {
        m11: (f64::from(transform.m22) / determinant) as f32,
        m12: (-f64::from(transform.m12) / determinant) as f32,
        m21: (-f64::from(transform.m21) / determinant) as f32,
        m22: (f64::from(transform.m11) / determinant) as f32,
        dx: ((f64::from(transform.dy) * f64::from(transform.m21)
            - f64::from(transform.dx) * f64::from(transform.m22))
            / determinant) as f32,
        dy: ((f64::from(transform.dx) * f64::from(transform.m12)
            - f64::from(transform.dy) * f64::from(transform.m11))
            / determinant) as f32,
    };
    finite(result).then_some(result)
}

pub fn page_to_device(dc: &DcObject) -> Option<Xform> {
    if dc.window_ext.cx == 0 || dc.window_ext.cy == 0 {
        return None;
    }
    let viewport = dc::mapping::effective_viewport(dc)?;
    let sx = viewport.cx as f32 / dc.window_ext.cx as f32;
    let sy = viewport.cy as f32 / dc.window_ext.cy as f32;
    let origin = dc.effective_window_origin();
    let result = Xform {
        m11: sx,
        m12: 0.0,
        m21: 0.0,
        m22: sy,
        dx: dc.viewport_org.x as f32 - origin.x as f32 * sx,
        dy: dc.viewport_org.y as f32 - origin.y as f32 * sy,
    };
    finite(result).then_some(result)
}

pub fn world_to_device(dc: &DcObject) -> Option<Xform> {
    compose(dc.world_transform, page_to_device(dc)?)
}

pub fn nearest_integer(value: f64) -> Option<i32> {
    if !value.is_finite() || value < i32::MIN as f64 || value > i32::MAX as f64 {
        return None;
    }
    let truncated = value as i64;
    let floor = if truncated as f64 > value {
        truncated - 1
    } else {
        truncated
    };
    i32::try_from(if value - floor as f64 >= 0.5 {
        floor + 1
    } else {
        floor
    })
    .ok()
}

pub fn apply(transform: Xform, point: PointL) -> Option<PointL> {
    Some(PointL::new(
        nearest_integer(
            f64::from(point.x) * f64::from(transform.m11)
                + f64::from(point.y) * f64::from(transform.m21)
                + f64::from(transform.dx),
        )?,
        nearest_integer(
            f64::from(point.x) * f64::from(transform.m12)
                + f64::from(point.y) * f64::from(transform.m22)
                + f64::from(transform.dy),
        )?,
    ))
}

fn transform_point_buffer(transform: Xform, bytes: &mut [u8]) -> bool {
    for point in bytes.chunks_exact_mut(8) {
        let x = i32::from_le_bytes(point[..4].try_into().unwrap());
        let y = i32::from_le_bytes(point[4..].try_into().unwrap());
        let Some(result) = apply(transform, PointL::new(x, y)) else {
            return false;
        };
        point[..4].copy_from_slice(&result.x.to_le_bytes());
        point[4..].copy_from_slice(&result.y.to_le_bytes());
    }
    true
}

pub fn logical_to_device(handle: u64, points: &mut [u8]) -> bool {
    let Some(transform) = dc::get_dc(handle).and_then(|dc| world_to_device(&dc)) else {
        return false;
    };
    transform_point_buffer(transform, points)
}

pub extern "C" fn nt_gdi_combine_transform(frame: &SyscallFrame) -> u64 {
    let Ok(first) = user_copy::read_user_object::<Xform>(frame.args[1]) else {
        return 0;
    };
    let Ok(second) = user_copy::read_user_object::<Xform>(frame.args[2]) else {
        return 0;
    };
    let Some(result) = compose(first, second) else {
        return 0;
    };
    user_copy::write_user_object(frame.args[0], &result).is_ok() as u64
}

pub extern "C" fn nt_gdi_modify_world_transform(frame: &SyscallFrame) -> u64 {
    let mode = frame.args[2] as u32;
    if !(1..=4).contains(&mode) || (mode != 1 && frame.args[1] == 0) {
        return 0;
    }
    // IDENTITY permits NULL; a non-NULL argument must still be readable.
    let input = if frame.args[1] == 0 {
        Xform::IDENTITY
    } else {
        let Ok(transform) = user_copy::read_user_object::<Xform>(frame.args[1]) else {
            return 0;
        };
        transform
    };
    let Some(mut dc) = dc::get_dc(frame.args[0]) else {
        return 0;
    };
    let result = match mode {
        1 => Some(Xform::IDENTITY),
        2 => compose(input, dc.world_transform),
        3 => compose(dc.world_transform, input),
        4 => finite(input).then_some(input),
        _ => None,
    };
    let Some(result) = result else { return 0 };
    dc.world_transform = result;
    dc.world_transform_valid = mode != 1;
    1
}

pub extern "C" fn nt_gdi_get_transform(frame: &SyscallFrame) -> u64 {
    let Some(dc) = dc::get_dc(frame.args[0]) else {
        return 0;
    };
    let result = match frame.args[1] as u32 {
        0x203 => Some(dc.world_transform),
        0x204 => world_to_device(&dc),
        0x304 => page_to_device(&dc),
        0x402 => world_to_device(&dc).and_then(inverse),
        _ => None,
    };
    let Some(result) = result else { return 0 };
    user_copy::write_user_object(frame.args[2], &result).is_ok() as u64
}

pub extern "C" fn nt_gdi_transform_points(frame: &SyscallFrame) -> u64 {
    let count = frame.args[3] as i32;
    if count <= 0 {
        return 1;
    }
    let count = count as usize;
    if count > MAX_POINTS || frame.args[4] as u32 > 1 {
        return 0;
    }
    let transform = {
        let Some(dc) = dc::get_dc(frame.args[0]) else {
            return 0;
        };
        let Some(transform) = world_to_device(&dc) else {
            return 0;
        };
        if frame.args[4] as u32 == 0 {
            let Some(inverted) = inverse(transform) else {
                return 0;
            };
            inverted
        } else {
            transform
        }
    };
    let mut bytes = Vec::new();
    if bytes.try_reserve_exact(count * 8).is_err() {
        return 0;
    }
    bytes.resize(count * 8, 0);
    if user_copy::copy_from_user(&mut bytes, frame.args[1]).is_err() {
        return 0;
    }
    if !transform_point_buffer(transform, &mut bytes) {
        return 0;
    }
    user_copy::copy_to_user(frame.args[2], &bytes).is_ok() as u64
}

pub extern "C" fn nt_gdi_get_dc_point(frame: &SyscallFrame) -> u64 {
    let Some(mut dc) = dc::get_dc(frame.args[0]) else {
        return 0;
    };
    let selector = frame.args[1] as u32;
    let point = match selector & !0x40 {
        1 => {
            let Some(viewport) = dc::mapping::effective_viewport(&dc) else {
                return 0;
            };
            // Win7 realizes pending isotropic scaling on a viewport query.
            dc.viewport_ext = viewport;
            PointL::new(viewport.cx, viewport.cy)
        }
        2 => PointL::new(
            if dc.attributes.layout & dc::mirror::LAYOUT_RTL != 0 {
                dc.window_ext.cx.wrapping_neg()
            } else {
                dc.window_ext.cx
            },
            dc.window_ext.cy,
        ),
        4 => PointL::new(
            if dc.attributes.layout & dc::mirror::LAYOUT_RTL != 0 {
                dc.viewport_org.x.wrapping_neg()
            } else {
                dc.viewport_org.x
            },
            dc.viewport_org.y,
        ),
        8 => dc.window_org,
        16 => PointL::new(dc.cur_x, dc.cur_y),
        // Current bindings are surface-local. The client-origin variant
        // requires window/DC origin tracking that is not yet available.
        32 if selector & 0x40 == 0 => PointL::new(0, 0),
        _ => return 0,
    };
    user_copy::write_user_object(frame.args[2], &point).is_ok() as u64
}

pub extern "C" fn nt_gdi_move_to(frame: &SyscallFrame) -> u64 {
    let Some(mut dc) = dc::get_dc(frame.args[0]) else {
        return 0;
    };
    let old = PointL::new(dc.cur_x, dc.cur_y);
    if !super::path::move_to(&mut dc, PointL::new(frame.args[1] as i32, frame.args[2] as i32)) {
        return 0;
    }
    if frame.args[3] != 0 && user_copy::write_user_object(frame.args[3], &old).is_err() {
        return 0;
    }
    1
}
