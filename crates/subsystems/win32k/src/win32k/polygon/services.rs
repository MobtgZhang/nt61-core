//! Native PolyDraw and PolyPolyDraw multiplexing, including polygon regions.

extern crate alloc;

use super::super::{clipping, dc, objects, path, raster};
use crate::arch::common::syscall_frame::SyscallFrame;
use crate::mm::user_copy;
use alloc::vec::Vec;
use dc::PointL;
use path::{
    GdiPath, PathState, MAX_PATH_POINTS, PT_BEZIERTO, PT_CLOSEFIGURE, PT_LINETO, PT_MOVETO,
};

fn capture_bytes(pointer: u64, count: usize) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(count).ok()?;
    bytes.resize(count, 0);
    if count != 0 {
        user_copy::copy_from_user(&mut bytes, pointer).ok()?;
    }
    Some(bytes)
}

fn capture_points(pointer: u64, count: usize) -> Option<Vec<PointL>> {
    if count > MAX_PATH_POINTS {
        return None;
    }
    let bytes = capture_bytes(pointer, count.checked_mul(8)?)?;
    let mut points = Vec::new();
    points.try_reserve_exact(count).ok()?;
    for point in bytes.chunks_exact(8) {
        points.push(PointL::new(
            i32::from_le_bytes(point[..4].try_into().ok()?),
            i32::from_le_bytes(point[4..].try_into().ok()?),
        ));
    }
    Some(points)
}

fn transformed(dc: &dc::DcObject, points: &[PointL]) -> Option<Vec<PointL>> {
    let mut result = Vec::new();
    result.try_reserve_exact(points.len()).ok()?;
    for point in points {
        result.push(clipping::logical_point(dc, *point)?);
    }
    Some(result)
}

fn recording(dc: &dc::DcObject) -> Option<GdiPath> {
    if dc.path.state == PathState::Open {
        return dc.path.try_clone();
    }
    let mut path = GdiPath::default();
    path.begin(clipping::logical_point(
        dc,
        PointL::new(dc.cur_x, dc.cur_y),
    )?);
    Some(path)
}

pub extern "C" fn nt_gdi_poly_draw(frame: &SyscallFrame) -> u64 {
    let count = frame.args[3] as u32 as usize;
    if count > MAX_PATH_POINTS {
        return 0;
    }
    let (Some(points), Some(types)) = (
        capture_points(frame.args[1], count),
        capture_bytes(frame.args[2], count),
    ) else {
        return 0;
    };
    // Validate the entire command stream before modifying a path or a pixel.
    let mut index = 0;
    while index < count {
        match types[index] {
            PT_MOVETO | PT_LINETO => index += 1,
            value if value == PT_LINETO | PT_CLOSEFIGURE => index += 1,
            PT_BEZIERTO
                if index + 2 < count
                    && types[index + 1] == PT_BEZIERTO
                    && types[index + 2] & !PT_CLOSEFIGURE == PT_BEZIERTO =>
            {
                index += 3
            }
            _ => return 0,
        }
    }
    let Some(mut dc) = dc::get_dc(frame.args[0]) else {
        return 0;
    };
    if count == 0 {
        return 1;
    }
    let (Some(device), Some(mut path)) = (transformed(&dc, &points), recording(&dc)) else {
        return 0;
    };
    let mut current = PointL::new(dc.cur_x, dc.cur_y);
    let mut figure_start = path.figure_start();
    index = 0;
    while index < count {
        let kind = types[index];
        if kind == PT_MOVETO {
            path.move_to(device[index]);
            current = points[index];
            figure_start = device[index];
            index += 1;
            continue;
        }
        let length = if kind == PT_BEZIERTO { 3 } else { 1 };
        let last = index + length - 1;
        if !path.append(&device[index..=last], kind & !PT_CLOSEFIGURE) {
            return 0;
        }
        current = points[last];
        if types[last] & PT_CLOSEFIGURE != 0 {
            path.close_figure();
            path.move_to(figure_start);
        }
        index += length;
    }
    if dc.path.state == PathState::Open {
        dc.path = path;
    } else if !raster::paint_path(&dc, &path, false, true) {
        return 0;
    }
    dc.cur_x = current.x;
    dc.cur_y = current.y;
    1
}

pub extern "C" fn nt_gdi_poly_poly_draw(frame: &SyscallFrame) -> u64 {
    let count = frame.args[3] as u32 as usize;
    let function = frame.args[4] as u32;
    if count == 0 || count > MAX_PATH_POINTS || !(1..=6).contains(&function) {
        return 0;
    }
    let Some(bytes) = capture_bytes(frame.args[2], count * 4) else {
        return 0;
    };
    let mut counts = Vec::new();
    if counts.try_reserve_exact(count).is_err() {
        return 0;
    }
    let mut total = 0usize;
    for bytes in bytes.chunks_exact(4) {
        let value = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
        let Some(sum) = total
            .checked_add(value)
            .filter(|sum| *sum <= MAX_PATH_POINTS)
        else {
            return 0;
        };
        total = sum;
        counts.push(value);
    }
    if matches!(function, 1 | 2 | 6) && counts.iter().any(|count| *count < 2) {
        return 0;
    }
    if function == 3 && (count != 1 || total < 4 || total % 3 != 1) {
        return 0;
    }
    if function == 5 && (count != 1 || total == 0 || total % 3 != 0) {
        return 0;
    }
    if function == 4 && count != 1 {
        return 0;
    }
    let Some(points) = capture_points(frame.args[1], total) else {
        return 0;
    };
    if function == 6 {
        let mut contours = Vec::new();
        if contours.try_reserve_exact(count).is_err() {
            return 0;
        }
        let mut offset = 0;
        for count in counts {
            contours.push(&points[offset..offset + count]);
            offset += count;
        }
        let Ok(region) = super::region_from_contours(&contours, frame.args[0] as i32) else {
            return 0;
        };
        return objects::gdi_create_region(region);
    }
    let Some(mut dc) = dc::get_dc(frame.args[0]) else {
        return 0;
    };
    if function == 4 && total == 0 {
        return 1;
    }
    let (Some(device), Some(mut path)) = (transformed(&dc, &points), recording(&dc)) else {
        return 0;
    };
    // Win7 selectors: 4 = PolylineTo, 5 = PolyBezierTo.
    match function {
        1 | 2 => {
            let mut offset = 0;
            for count in counts {
                if !path.add_figure(&device[offset..offset + count], function == 1) {
                    return 0;
                }
                offset += count;
            }
        }
        3 => {
            path.move_to(device[0]);
            if !path.append(&device[1..], PT_BEZIERTO) {
                return 0;
            }
            let Some(current) = clipping::logical_point(&dc, PointL::new(dc.cur_x, dc.cur_y))
            else {
                return 0;
            };
            path.move_to(current);
        }
        4 | 5 => {
            if !device.is_empty()
                && !path.append(
                    &device,
                    if function == 5 {
                        PT_BEZIERTO
                    } else {
                        PT_LINETO
                    },
                )
            {
                return 0;
            }
        }
        _ => return 0,
    }
    if dc.path.state == PathState::Open {
        dc.path = path;
    } else if !raster::paint_path(&dc, &path, function == 1, true) {
        return 0;
    }
    if matches!(function, 4 | 5) {
        if let Some(point) = points.last() {
            dc.cur_x = point.x;
            dc.cur_y = point.y;
        }
    }
    1
}
