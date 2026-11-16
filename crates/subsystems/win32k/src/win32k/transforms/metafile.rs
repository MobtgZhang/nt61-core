//! Conversion of exclusive metafile bounds to inclusive device bounds.
//!
//! Win7 transforms the two rectangle corners to 28.4 device coordinates,
//! orders the axes, subtracts one device pixel from the lower-right corner,
//! then transforms back. The result is 1 for an empty device rectangle and 2
//! otherwise; neither result is a BOOL failure.

use super::{dc, finite, inverse, world_to_device, Xform};
use crate::arch::common::syscall_frame::SyscallFrame;
use crate::mm::user_copy::{read_user_object, write_user_object};

fn failure() -> u64 {
    super::super::input_services::set_last_error(87);
    0
}

// Win7's FLOAT-to-LONG helper rounds the magnitude before restoring its sign.
// It is different from the identity POINTFIX path's floor(value + 0.5).
fn rounded_magnitude(value: f32) -> Option<i32> {
    if !value.is_finite() {
        return None;
    }
    let rounded = (f64::from(value).abs() + 0.5) as i64;
    i32::try_from(if value < 0.0 { -rounded } else { rounded }).ok()
}

fn to_device_fixed(transform: Xform, point: [i32; 2]) -> Option<[i32; 2]> {
    if transform.m11 == 1.0 && transform.m12 == 0.0 && transform.m21 == 0.0 && transform.m22 == 1.0
    {
        return Some([
            point[0]
                .wrapping_shl(4)
                .wrapping_add(rounded_magnitude(transform.dx * 16.0)?),
            point[1]
                .wrapping_shl(4)
                .wrapping_add(rounded_magnitude(transform.dy * 16.0)?),
        ]);
    }
    let [x, y] = point.map(|coordinate| coordinate as f32);
    Some([
        rounded_magnitude(x * (transform.m11 * 16.0) + y * (transform.m21 * 16.0))?
            .wrapping_add(rounded_magnitude(transform.dx * 16.0)?),
        rounded_magnitude(x * (transform.m12 * 16.0) + y * (transform.m22 * 16.0))?
            .wrapping_add(rounded_magnitude(transform.dy * 16.0)?),
    ])
}

fn from_device_fixed(transform: Xform, point: [i32; 2]) -> Option<[i32; 2]> {
    if transform.m11 == 1.0
        && transform.m12 == 0.0
        && transform.m21 == 0.0
        && transform.m22 == 1.0
        && transform.dx == transform.dx as i32 as f32
        && transform.dy == transform.dy as i32 as f32
    {
        // The native identity/translation optimization rounds fixed-point
        // values with SAR 3; INC; SAR 1, including negative half-pixels.
        return Some([
            ((point[0] >> 3).wrapping_add(1) >> 1).wrapping_add(transform.dx as i32),
            ((point[1] >> 3).wrapping_add(1) >> 1).wrapping_add(transform.dy as i32),
        ]);
    }
    let [x, y] = point.map(|coordinate| coordinate as f32);
    Some([
        rounded_magnitude(x * (transform.m11 / 16.0) + y * (transform.m21 / 16.0))?
            .wrapping_add(rounded_magnitude(transform.dx)?),
        rounded_magnitude(x * (transform.m12 / 16.0) + y * (transform.m22 / 16.0))?
            .wrapping_add(rounded_magnitude(transform.dy)?),
    ])
}

pub extern "C" fn nt_gdi_convert_metafile_rect(frame: &SyscallFrame) -> u64 {
    let Ok(input) = read_user_object::<[i32; 4]>(frame.args[1]) else {
        return failure();
    };
    let transform = {
        let Some(object) = dc::get_dc(frame.args[0]) else {
            return failure();
        };
        let Some(transform) = world_to_device(&object) else {
            return failure();
        };
        transform
    };
    if !finite(transform) {
        return failure();
    }
    let Some(inverted) = inverse(transform) else {
        return failure();
    };
    let Some(first) = to_device_fixed(transform, [input[0], input[1]]) else {
        return failure();
    };
    let Some(second) = to_device_fixed(transform, [input[2], input[3]]) else {
        return failure();
    };
    let minimum = [first[0].min(second[0]), first[1].min(second[1])];
    let maximum = [
        first[0].max(second[0]).wrapping_sub(16),
        first[1].max(second[1]).wrapping_sub(16),
    ];
    let empty = maximum[0] < minimum[0] || maximum[1] < minimum[1];
    let Some(first) = from_device_fixed(inverted, minimum) else {
        return failure();
    };
    let Some(second) = from_device_fixed(inverted, maximum) else {
        return failure();
    };
    let output = [first[0], first[1], second[0], second[1]];
    if write_user_object(frame.args[1], &output).is_err() {
        return failure();
    }
    if empty {
        1
    } else {
        2
    }
}
