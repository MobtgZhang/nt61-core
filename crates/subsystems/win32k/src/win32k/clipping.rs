//! Owned DC clipping state and native clipping services.
//!
//! Application and meta regions use device coordinates. Logical coordinates
//! enter through the DC transform, while the system-visible region comes from
//! the bound surface. Window occlusion is a separate USER responsibility.

use super::dc::{self, DcObject, PointL};
use super::objects::Rect;
use super::region;
use super::region_geometry::{Region, RegionCombineMode as Mode, RegionError};
use crate::arch::common::syscall_frame::SyscallFrame;
use crate::mm::user_copy;

pub fn visible_region(dc: &DcObject) -> Result<Region, RegionError> {
    if dc.is_information_dc() {
        return Region::rectangle(dc.bounds);
    }
    let surface = dc.drawing_surface();
    if surface.is_null() {
        return Ok(Region::default());
    }
    // The DC guard owns access to the binding. Display surfaces are borrowed;
    // a selected bitmap's ObjectReference pins its view and pixel allocation.
    let surface = unsafe { &*surface };
    Region::rectangle(Rect::new(0, 0, surface.width, surface.height))
}

pub fn effective_region(dc: &DcObject) -> Result<Region, RegionError> {
    let mut result = visible_region(dc)?;
    for clip in [&dc.meta_clip, &dc.application_clip].into_iter().flatten() {
        result = result.combine(clip, Mode::Intersection)?;
    }
    Ok(result)
}

fn api_region(dc: &DcObject) -> Result<Option<Region>, RegionError> {
    match (&dc.application_clip, &dc.meta_clip) {
        (Some(application), Some(meta)) => Ok(Some(application.combine(meta, Mode::Intersection)?)),
        (Some(clip), None) | (None, Some(clip)) => Ok(Some(clip.try_clone()?)),
        (None, None) => Ok(None),
    }
}

pub fn logical_point(dc: &DcObject, point: PointL) -> Option<PointL> {
    super::transforms::apply(super::transforms::world_to_device(dc)?, point)
}

pub(super) fn logical_rect(dc: &DcObject, mut rect: Rect) -> Option<Rect> {
    // Axis-aligned region clipping is exact. Rotated/sheared rectangle
    // rasterization requires a polygon region and must not use its bounding box.
    if dc.world_transform.m12 != 0.0 || dc.world_transform.m21 != 0.0 {
        return None;
    }
    rect.normalize();
    let first = logical_point(dc, PointL::new(rect.left, rect.top))?;
    let second = logical_point(dc, PointL::new(rect.right, rect.bottom))?;
    let mut result = Rect::new(first.x, first.y, second.x, second.y);
    result.normalize();
    Some(result)
}

pub(super) fn clip_with(dc: &mut DcObject, input: Option<Region>, mode: Mode) -> i32 {
    let new_clip = match input {
        None if mode == Mode::Copy => None,
        None => return 0,
        Some(region) if mode == Mode::Copy => Some(region),
        Some(region) => {
            let current = match &dc.application_clip {
                Some(clip) => clip.try_clone(),
                None => visible_region(dc),
            };
            let Ok(current) = current else { return 0 };
            let Ok(combined) = current.combine(&region, mode) else {
                return 0;
            };
            Some(combined)
        }
    };
    // Compute the result before commit, so allocation failures preserve state.
    let mut effective = match visible_region(dc) {
        Ok(region) => region,
        Err(_) => return 0,
    };
    for clip in [&dc.meta_clip, &new_clip].into_iter().flatten() {
        let Ok(combined) = effective.combine(clip, Mode::Intersection) else {
            return 0;
        };
        effective = combined;
    }
    let complexity = effective.complexity();
    dc.application_clip = new_clip;
    complexity
}

pub fn select_clip(handle: u64, source: u64, mode: Mode) -> i32 {
    let input = if source == 0 {
        None
    } else {
        let Some(region) = region::snapshot(source) else {
            return 0;
        };
        Some(region)
    };
    let Some(mut dc) = dc::get_dc(handle) else {
        return 0;
    };
    clip_with(&mut dc, input, mode)
}

pub extern "C" fn nt_gdi_ext_select_clip_rgn(frame: &SyscallFrame) -> u64 {
    let Some(mode) = Mode::from_raw(frame.args[2] as i32) else {
        return 0;
    };
    select_clip(frame.args[0], frame.args[1], mode) as u64
}

pub fn clip_rectangle(handle: u64, rect: Rect, mode: Mode) -> i32 {
    let Some(mut dc) = dc::get_dc(handle) else {
        return 0;
    };
    let Some(rect) = logical_rect(&dc, rect) else {
        return 0;
    };
    let Ok(region) = Region::rectangle(rect) else {
        return 0;
    };
    clip_with(&mut dc, Some(region), mode)
}

fn rectangle_arguments(frame: &SyscallFrame) -> Rect {
    Rect::new(
        frame.args[1] as i32,
        frame.args[2] as i32,
        frame.args[3] as i32,
        frame.args[4] as i32,
    )
}

pub extern "C" fn nt_gdi_intersect_clip_rect(frame: &SyscallFrame) -> u64 {
    let result = clip_rectangle(
        frame.args[0],
        rectangle_arguments(frame),
        Mode::Intersection,
    );
    // Native rectangle clipping reports COMPLEXREGION even for a single box.
    if result == 2 {
        3
    } else {
        result as u64
    }
}

pub extern "C" fn nt_gdi_exclude_clip_rect(frame: &SyscallFrame) -> u64 {
    let result = clip_rectangle(frame.args[0], rectangle_arguments(frame), Mode::Difference);
    if result == 2 {
        3
    } else {
        result as u64
    }
}

pub extern "C" fn nt_gdi_offset_clip_rgn(frame: &SyscallFrame) -> u64 {
    let Some(mut dc) = dc::get_dc(frame.args[0]) else {
        return 0;
    };
    let Some(zero) = logical_point(&dc, PointL::new(0, 0)) else {
        return 0;
    };
    let Some(offset) = logical_point(&dc, PointL::new(frame.args[1] as i32, frame.args[2] as i32))
    else {
        return 0;
    };
    let (Some(dx), Some(dy)) = (offset.x.checked_sub(zero.x), offset.y.checked_sub(zero.y)) else {
        return 0;
    };
    let Some(clip) = dc.application_clip.as_mut() else {
        return 2;
    };
    if clip.offset(dx, dy).is_err() {
        return 0;
    }
    clip.complexity() as u64
}

pub extern "C" fn nt_gdi_set_meta_rgn(frame: &SyscallFrame) -> u64 {
    let Some(mut dc) = dc::get_dc(frame.args[0]) else {
        return 0;
    };
    let Ok(combined) = api_region(&dc) else {
        return 0;
    };
    let complexity = combined.as_ref().map_or(2, Region::complexity);
    dc.meta_clip = combined;
    dc.application_clip = None;
    complexity as u64
}

pub extern "C" fn nt_gdi_get_random_rgn(frame: &SyscallFrame) -> u64 {
    let Some(dc) = dc::get_dc(frame.args[0]) else {
        return u32::MAX as u64;
    };
    let result = match frame.args[2] as u32 {
        1 => dc
            .application_clip
            .as_ref()
            .map(Region::try_clone)
            .transpose(),
        2 => dc.meta_clip.as_ref().map(Region::try_clone).transpose(),
        3 => api_region(&dc),
        4 => visible_region(&dc).map(Some),
        _ => return u32::MAX as u64,
    };
    match result {
        Ok(Some(region)) => {
            if region::replace_geometry(frame.args[1], region) {
                1
            } else {
                u32::MAX as u64
            }
        }
        Ok(None) => 0,
        Err(_) => u32::MAX as u64,
    }
}

pub extern "C" fn nt_gdi_get_app_clip_box(frame: &SyscallFrame) -> u64 {
    let Some(dc) = dc::get_dc(frame.args[0]) else {
        return 0;
    };
    let Ok(effective) = effective_region(&dc) else {
        return 0;
    };
    let mut rect = effective.bounds();
    // GetClipBox returns logical coordinates. Invert the axis-aligned mapping.
    let transform = dc.world_transform;
    if transform.m12 != 0.0
        || transform.m21 != 0.0
        || transform.m11 == 0.0
        || transform.m22 == 0.0
        || dc.viewport_ext.cx == 0
        || dc.viewport_ext.cy == 0
    {
        return 0;
    }
    if effective.complexity() != 1 {
        let origin = dc.effective_window_origin();
        let inverse = |x: i32, y: i32| -> Option<PointL> {
            Some(PointL::new(
                super::transforms::nearest_integer(
                    ((f64::from(x) - f64::from(dc.viewport_org.x)) * f64::from(dc.window_ext.cx)
                        / f64::from(dc.viewport_ext.cx)
                        + f64::from(origin.x)
                        - f64::from(transform.dx))
                        / f64::from(transform.m11),
                )?,
                super::transforms::nearest_integer(
                    ((f64::from(y) - f64::from(dc.viewport_org.y)) * f64::from(dc.window_ext.cy)
                        / f64::from(dc.viewport_ext.cy)
                        + f64::from(origin.y)
                        - f64::from(transform.dy))
                        / f64::from(transform.m22),
                )?,
            ))
        };
        let (Some(first), Some(second)) = (
            inverse(rect.left, rect.top),
            inverse(rect.right, rect.bottom),
        ) else {
            return 0;
        };
        rect = Rect::new(first.x, first.y, second.x, second.y);
        rect.normalize();
    }
    if user_copy::write_user_object(frame.args[1], &rect).is_err() {
        return 0;
    }
    effective.complexity() as u64
}

pub extern "C" fn nt_gdi_pt_visible(frame: &SyscallFrame) -> u64 {
    let Some(dc) = dc::get_dc(frame.args[0]) else {
        return 0;
    };
    let Some(point) = logical_point(&dc, PointL::new(frame.args[1] as i32, frame.args[2] as i32))
    else {
        return 0;
    };
    effective_region(&dc).is_ok_and(|region| region.contains_point(point.x, point.y)) as u64
}

pub extern "C" fn nt_gdi_rect_visible(frame: &SyscallFrame) -> u64 {
    let Ok(rect) = user_copy::read_user_object::<Rect>(frame.args[1]) else {
        return 0;
    };
    let Some(dc) = dc::get_dc(frame.args[0]) else {
        return 0;
    };
    let Some(rect) = logical_rect(&dc, rect) else {
        return 0;
    };
    effective_region(&dc).is_ok_and(|region| region.intersects_rect(&rect)) as u64
}
