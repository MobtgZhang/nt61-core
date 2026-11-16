//! Native region ABI. User buffers are copied through user_copy, never cast to
//! references. RGNDATA is a 32-byte header followed by 16-byte RECT records.

extern crate alloc;

use super::{objects, Rect, Region, RegionCombineMode};
use crate::arch::common::syscall_frame::SyscallFrame;
use crate::mm::user_copy;
use alloc::vec::Vec;

const HEADER_SIZE: usize = 32;
const RECT_SIZE: usize = 16;

fn rectangle_arguments(frame: &SyscallFrame, start: usize) -> Rect {
    Rect::new(
        frame.args[start] as i32,
        frame.args[start + 1] as i32,
        frame.args[start + 2] as i32,
        frame.args[start + 3] as i32,
    )
}

pub extern "C" fn nt_gdi_create_rect_rgn(frame: &SyscallFrame) -> u64 {
    let rect = rectangle_arguments(frame, 0);
    super::gre_create_rect_rgn_indirect(&rect)
}

pub extern "C" fn nt_gdi_create_round_rect_rgn(frame: &SyscallFrame) -> u64 {
    let bounds = rectangle_arguments(frame, 0);
    let diameter = [i64::from(frame.args[4] as i32), i64::from(frame.args[5] as i32)];
    super::curves::rounded(bounds, diameter).map(objects::gdi_create_region).unwrap_or(0)
}

pub extern "C" fn nt_gdi_create_elliptic_rgn(frame: &SyscallFrame) -> u64 {
    let bounds = rectangle_arguments(frame, 0);
    let diameter = [
        (i64::from(bounds.right) - i64::from(bounds.left)).abs(),
        (i64::from(bounds.bottom) - i64::from(bounds.top)).abs(),
    ];
    super::curves::rounded(bounds, diameter).map(objects::gdi_create_region).unwrap_or(0)
}

pub extern "C" fn nt_gdi_combine_rgn(frame: &SyscallFrame) -> u64 {
    let Some(mode) = RegionCombineMode::from_raw(frame.args[3] as i32) else {
        return 0;
    };
    super::gre_combine_rgn(frame.args[0], frame.args[1], frame.args[2], mode) as u64
}

pub extern "C" fn nt_gdi_equal_rgn(frame: &SyscallFrame) -> u64 {
    super::gre_equal_rgn(frame.args[0], frame.args[1]) as u64
}

pub extern "C" fn nt_gdi_pt_in_region(frame: &SyscallFrame) -> u64 {
    super::gre_pt_in_region(frame.args[0], frame.args[1] as i32, frame.args[2] as i32) as u64
}

pub extern "C" fn nt_gdi_rect_in_region(frame: &SyscallFrame) -> u64 {
    let Ok(rect) = user_copy::read_user_object::<Rect>(frame.args[1]) else {
        return 0;
    };
    super::gre_rect_in_region(frame.args[0], &rect) as u64
}

pub extern "C" fn nt_gdi_get_rgn_box(frame: &SyscallFrame) -> u64 {
    let mut rect = Rect::default();
    let complexity = super::gre_get_rgn_box(frame.args[0], &mut rect);
    if complexity == 0 || user_copy::write_user_object(frame.args[1], &rect).is_err() {
        return 0;
    }
    complexity as u64
}

pub extern "C" fn nt_gdi_offset_rgn(frame: &SyscallFrame) -> u64 {
    super::gre_offset_rgn(frame.args[0], frame.args[1] as i32, frame.args[2] as i32) as u64
}

pub extern "C" fn nt_gdi_set_rect_rgn(frame: &SyscallFrame) -> u64 {
    super::gre_set_rect_rgn(frame.args[0], rectangle_arguments(frame, 1)) as u64
}

fn append_rect(bytes: &mut Vec<u8>, rect: Rect) {
    for coordinate in rect.to_array() {
        bytes.extend_from_slice(&coordinate.to_le_bytes());
    }
}

pub extern "C" fn nt_gdi_get_region_data(frame: &SyscallFrame) -> u64 {
    let Some(region) = super::snapshot(frame.args[0]) else {
        return 0;
    };
    let data_size = region.rectangles().len() * RECT_SIZE;
    let required = HEADER_SIZE + data_size;
    if frame.args[2] == 0 {
        return required as u64;
    }
    if (frame.args[1] as u32 as usize) < required {
        return 0;
    }
    let mut bytes = Vec::new();
    if bytes.try_reserve_exact(required).is_err() {
        return 0;
    }
    for value in [
        HEADER_SIZE as u32,
        1,
        region.rectangles().len() as u32,
        data_size as u32,
    ] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    append_rect(&mut bytes, region.bounds());
    for rect in region.rectangles() {
        append_rect(&mut bytes, *rect);
    }
    if user_copy::copy_to_user(frame.args[2], &bytes).is_err() {
        return 0;
    }
    required as u64
}

fn word(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

pub extern "C" fn nt_gdi_ext_create_region(frame: &SyscallFrame) -> u64 {
    // Capture the bounded header first, then capture the complete buffer once.
    // Revalidate its header to avoid trusting values changed during capture.
    let size = frame.args[1] as u32 as usize;
    if size < HEADER_SIZE {
        return 0;
    }
    let mut header = [0; HEADER_SIZE];
    if user_copy::copy_from_user(&mut header, frame.args[2]).is_err() {
        return 0;
    }
    let count = word(&header, 8) as usize;
    if count > super::super::region_geometry::MAX_REGION_RECTS {
        return 0;
    }
    let required = HEADER_SIZE + count * RECT_SIZE;
    if word(&header, 0) != HEADER_SIZE as u32 || word(&header, 4) != 1 || required > size {
        return 0;
    }
    let mut bytes = Vec::new();
    if bytes.try_reserve_exact(required).is_err() {
        return 0;
    }
    bytes.resize(required, 0);
    if user_copy::copy_from_user(&mut bytes, frame.args[2]).is_err()
        || bytes[..HEADER_SIZE] != header
    {
        return 0;
    }
    let mut rectangles = Vec::new();
    if rectangles.try_reserve_exact(count).is_err() {
        return 0;
    }
    for index in 0..count {
        let offset = HEADER_SIZE + index * RECT_SIZE;
        let rect = Rect::new(
            word(&bytes, offset) as i32,
            word(&bytes, offset + 4) as i32,
            word(&bytes, offset + 8) as i32,
            word(&bytes, offset + 12) as i32,
        );
        if !super::super::region_geometry::valid_rect(&rect)
            || rect.left > rect.right
            || rect.top > rect.bottom
        {
            return 0;
        }
        rectangles.push(rect);
    }
    if frame.args[0] != 0 {
        use super::super::{
            dc::{PointL, Xform},
            transforms,
        };
        let Ok(transform) = user_copy::read_user_object::<Xform>(frame.args[0]) else {
            return 0;
        };
        // Axis-aligned scaling, translation and reflections preserve rectangles.
        // General rotation/shear needs polygon scan conversion, not a bounds box.
        if !transforms::finite(transform) || transform.m12 != 0.0 || transform.m21 != 0.0 {
            return 0;
        }
        for rect in &mut rectangles {
            let Some(first) = transforms::apply(transform, PointL::new(rect.left, rect.top)) else {
                return 0;
            };
            let Some(second) = transforms::apply(transform, PointL::new(rect.right, rect.bottom))
            else {
                return 0;
            };
            *rect = Rect::new(first.x, first.y, second.x, second.y);
            rect.normalize();
        }
    }
    let Ok(region) = Region::from_rectangles(&rectangles) else {
        return 0;
    };
    objects::gdi_create_region(region)
}
