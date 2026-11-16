//! GDI region object operations. Handle validation and geometry access stay
//! inside the GDI table lock, including when the destination aliases a source.

use super::objects::{self, GdiHandleTable, GdiObjectType, GdiRegion, Rect};
use super::region_geometry::Region;
pub use super::region_geometry::{RegionCombineMode, RegionType};

#[path = "region/services.rs"]
pub mod services;
#[path = "region/curves.rs"]
mod curves;

pub(super) fn replace_geometry(handle: u64, geometry: Region) -> bool {
    objects::with_handle_table(|table| {
        let Some(pointer) = region_pointer(table, handle) else {
            return false;
        };
        unsafe {
            (*pointer).replace(geometry);
        }
        true
    })
}

fn region_pointer(table: &GdiHandleTable, handle: u64) -> Option<*mut GdiRegion> {
    let entry = table.get(handle)?;
    if entry.get_type()? != GdiObjectType::Region || entry.obj_ptr == 0 {
        return None;
    }
    Some(entry.obj_ptr as *mut GdiRegion)
}

pub fn gre_create_rect_rgn(left: i32, top: i32, right: i32, bottom: i32) -> u64 {
    objects::gdi_create_rect_rgn(left, top, right, bottom)
}

pub fn gre_create_rect_rgn_indirect(rect: &Rect) -> u64 {
    gre_create_rect_rgn(rect.left, rect.top, rect.right, rect.bottom)
}

pub fn gre_delete_region(handle: u64) -> bool {
    objects::delete_object_checked(handle, Some(GdiObjectType::Region))
}

pub fn rect_contains(outer: &Rect, inner: &Rect) -> bool {
    inner.left >= outer.left
        && inner.right <= outer.right
        && inner.top >= outer.top
        && inner.bottom <= outer.bottom
}

pub fn rect_intersection(first: &Rect, second: &Rect) -> Option<Rect> {
    first.intersect(second)
}

pub fn rects_overlap(first: &Rect, second: &Rect) -> bool {
    first.intersect(second).is_some()
}

pub fn snapshot(handle: u64) -> Option<Region> {
    objects::with_handle_table(|table| {
        let pointer = region_pointer(table, handle)?;
        // SAFETY: the table guard prevents deletion for this entire closure.
        unsafe { (*pointer).geometry.try_clone().ok() }
    })
}

pub fn gre_combine_rgn(destination: u64, first: u64, second: u64, mode: RegionCombineMode) -> i32 {
    objects::with_handle_table(|table| {
        let Some(destination) = region_pointer(table, destination) else {
            return 0;
        };
        let Some(first) = region_pointer(table, first) else {
            return 0;
        };
        let result = unsafe {
            if mode == RegionCombineMode::Copy {
                (*first).geometry.try_clone()
            } else {
                let Some(second) = region_pointer(table, second) else {
                    return 0;
                };
                (*first).geometry.combine(&(*second).geometry, mode)
            }
        };
        let Ok(result) = result else { return 0 };
        let complexity = result.complexity();
        // All source borrows ended before this write, even for CombineRgn(r,r,r).
        unsafe {
            (*destination).replace(result);
        }
        complexity
    })
}

pub fn gre_equal_rgn(first: u64, second: u64) -> bool {
    objects::with_handle_table(|table| {
        let (Some(first), Some(second)) =
            (region_pointer(table, first), region_pointer(table, second))
        else {
            return false;
        };
        unsafe { (*first).geometry == (*second).geometry }
    })
}

pub fn gre_pt_in_region(handle: u64, x: i32, y: i32) -> bool {
    objects::with_handle_table(|table| {
        region_pointer(table, handle)
            .is_some_and(|pointer| unsafe { (*pointer).geometry.contains_point(x, y) })
    })
}

pub fn gre_rect_in_region(handle: u64, rect: &Rect) -> bool {
    objects::with_handle_table(|table| {
        region_pointer(table, handle)
            .is_some_and(|pointer| unsafe { (*pointer).geometry.intersects_rect(rect) })
    })
}

pub fn gre_get_rgn_box(handle: u64, rect: &mut Rect) -> i32 {
    objects::with_handle_table(|table| {
        let Some(pointer) = region_pointer(table, handle) else {
            return 0;
        };
        unsafe {
            *rect = (*pointer).geometry.bounds();
            (*pointer).geometry.complexity()
        }
    })
}

pub fn gre_offset_rgn(handle: u64, dx: i32, dy: i32) -> i32 {
    objects::with_handle_table(|table| {
        let Some(pointer) = region_pointer(table, handle) else {
            return 0;
        };
        unsafe {
            if (*pointer).geometry.offset(dx, dy).is_err() {
                return 0;
            }
            (*pointer).extents = (*pointer).geometry.bounds();
            (*pointer).geometry.complexity()
        }
    })
}

pub fn gre_set_rect_rgn(handle: u64, rect: Rect) -> bool {
    let Ok(geometry) = Region::rectangle(rect) else {
        return false;
    };
    objects::with_handle_table(|table| {
        let Some(pointer) = region_pointer(table, handle) else {
            return false;
        };
        unsafe {
            (*pointer).replace(geometry);
        }
        true
    })
}

/// Copy an application's clipping geometry into a DC.
pub fn gre_select_clip_rgn(dc: u64, region: u64) -> i32 {
    super::clipping::select_clip(dc, region, RegionCombineMode::Copy)
}

pub fn gre_offset_dcorg(dc: u64, x: i32, y: i32) -> bool {
    let Some(mut dc) = super::dc::get_dc(dc) else {
        return false;
    };
    let (Some(x), Some(y)) = (
        dc.viewport_org.x.checked_add(x),
        dc.viewport_org.y.checked_add(y),
    ) else {
        return false;
    };
    dc.viewport_org = super::dc::PointL::new(x, y);
    true
}

pub fn gre_set_dcclipping(dc: u64, rect: &Rect) -> bool {
    let Ok(region) = Region::rectangle(*rect) else {
        return false;
    };
    let Some(mut dc) = super::dc::get_dc(dc) else {
        return false;
    };
    super::clipping::clip_with(&mut dc, Some(region), RegionCombineMode::Copy) != 0
}

pub fn gre_ext_exclude_clip_rect(dc: u64, left: i32, top: i32, right: i32, bottom: i32) -> i32 {
    super::clipping::clip_rectangle(
        dc,
        Rect::new(left, top, right, bottom),
        RegionCombineMode::Difference,
    )
}

pub fn dump_region_info(_handle: u64) {}
