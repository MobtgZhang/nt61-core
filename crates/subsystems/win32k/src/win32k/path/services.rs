//! Native path ABI. The DC guard covers state changes, rendering and output
//! capture. User buffers are probed/copied; none are borrowed as kernel slices.

extern crate alloc;

use super::super::{clipping, dc, objects, raster, region_geometry::RegionCombineMode, transforms};
use super::{GdiPath, PathState};
use crate::arch::common::syscall_frame::SyscallFrame;
use crate::mm::user_copy;
use alloc::vec::Vec;

const GDI_ERROR: u64 = u32::MAX as u64;

pub extern "C" fn nt_gdi_begin_path(frame: &SyscallFrame) -> u64 {
    let Some(mut dc) = dc::get_dc(frame.args[0]) else {
        return 0;
    };
    let Some(position) = clipping::logical_point(&dc, dc::PointL::new(dc.cur_x, dc.cur_y)) else {
        return 0;
    };
    dc.path.begin(position);
    1
}

pub extern "C" fn nt_gdi_end_path(frame: &SyscallFrame) -> u64 {
    let Some(mut dc) = dc::get_dc(frame.args[0]) else {
        return 0;
    };
    if dc.path.state != PathState::Open {
        return 0;
    }
    dc.path.state = PathState::Closed;
    1
}

pub extern "C" fn nt_gdi_abort_path(frame: &SyscallFrame) -> u64 {
    let Some(mut dc) = dc::get_dc(frame.args[0]) else {
        return 0;
    };
    dc.path = GdiPath::default();
    1
}

pub extern "C" fn nt_gdi_close_figure(frame: &SyscallFrame) -> u64 {
    let Some(mut dc) = dc::get_dc(frame.args[0]) else {
        return 0;
    };
    if dc.path.state != PathState::Open {
        return 0;
    }
    dc.path.close_figure();
    1
}

pub extern "C" fn nt_gdi_flatten_path(frame: &SyscallFrame) -> u64 {
    let Some(mut dc) = dc::get_dc(frame.args[0]) else {
        return 0;
    };
    if dc.path.state != PathState::Closed {
        return 0;
    }
    let Some(path) = dc.path.flattened() else {
        return 0;
    };
    dc.path = path;
    1
}

pub extern "C" fn nt_gdi_get_path(frame: &SyscallFrame) -> u64 {
    let Some(dc) = dc::get_dc(frame.args[0]) else {
        return GDI_ERROR;
    };
    if dc.path.state != PathState::Closed {
        return GDI_ERROR;
    }
    let count = dc.path.entries.len();
    let size = frame.args[3] as i32;
    if size == 0 {
        return count as u64;
    }
    if size < 0 || size > 0x1fff_ffff || (size as usize) < count {
        return GDI_ERROR;
    }
    // Probe both arrays before writing either one. Changes to mappings during
    // the actual copies can still cause a later user_copy failure.
    // Win7 probes the caller's full capacity with DWORD alignment for both
    // buffers (even the BYTE type array), rather than only the used entries.
    if frame.args[1] & 3 != 0
        || frame.args[2] & 3 != 0
        || user_copy::probe_user_write(frame.args[1], size as usize * 8).is_err()
        || user_copy::probe_user_write(frame.args[2], size as usize).is_err()
    {
        return GDI_ERROR;
    }
    let Some(inverse) = transforms::world_to_device(&dc).and_then(transforms::inverse) else {
        return GDI_ERROR;
    };
    let mut points = Vec::new();
    let mut types = Vec::new();
    if points.try_reserve_exact(count * 8).is_err() || types.try_reserve_exact(count).is_err() {
        return GDI_ERROR;
    }
    for entry in &dc.path.entries {
        let Some(point) = transforms::apply(inverse, entry.point) else {
            return GDI_ERROR;
        };
        points.extend_from_slice(&point.x.to_le_bytes());
        points.extend_from_slice(&point.y.to_le_bytes());
        types.push(entry.kind);
    }
    if user_copy::copy_to_user(frame.args[1], &points).is_err()
        || user_copy::copy_to_user(frame.args[2], &types).is_err()
    {
        return GDI_ERROR;
    }
    count as u64
}

pub extern "C" fn nt_gdi_path_to_region(frame: &SyscallFrame) -> u64 {
    let Some(mut dc) = dc::get_dc(frame.args[0]) else {
        return 0;
    };
    if dc.path.state != PathState::Closed {
        return 0;
    }
    let path = core::mem::take(&mut dc.path);
    if path.entries.is_empty() {
        return 0;
    }
    let Some(region) = path.region(dc.poly_fill_mode) else {
        return 0;
    };
    objects::gdi_create_region(region)
}

pub extern "C" fn nt_gdi_select_clip_path(frame: &SyscallFrame) -> u64 {
    let Some(mode) = RegionCombineMode::from_raw(frame.args[1] as i32) else {
        return 0;
    };
    let Some(mut dc) = dc::get_dc(frame.args[0]) else {
        return 0;
    };
    if dc.path.state != PathState::Closed {
        return 0;
    }
    let path = core::mem::take(&mut dc.path);
    if path.entries.is_empty() {
        return 0;
    }
    let Some(region) = path.region(dc.poly_fill_mode) else {
        return 0;
    };
    (clipping::clip_with(&mut dc, Some(region), mode) != 0) as u64
}

fn draw(frame: &SyscallFrame, fill: bool, stroke: bool) -> u64 {
    let Some(mut dc) = dc::get_dc(frame.args[0]) else {
        return 0;
    };
    if dc.path.state != PathState::Closed {
        return 0;
    }
    let path = core::mem::take(&mut dc.path);
    (path.entries.is_empty() || raster::paint_path(&dc, &path, fill, stroke)) as u64
}

pub extern "C" fn nt_gdi_fill_path(frame: &SyscallFrame) -> u64 {
    draw(frame, true, false)
}
pub extern "C" fn nt_gdi_stroke_path(frame: &SyscallFrame) -> u64 {
    draw(frame, false, true)
}
pub extern "C" fn nt_gdi_stroke_and_fill_path(frame: &SyscallFrame) -> u64 {
    draw(frame, true, true)
}

pub extern "C" fn nt_gdi_get_miter_limit(frame: &SyscallFrame) -> u64 {
    let Some(dc) = dc::get_dc(frame.args[0]) else {
        return 0;
    };
    if frame.args[1] & 3 != 0 {
        return 0;
    }
    user_copy::write_user_object(frame.args[1], &dc.miter_limit.to_bits()).is_ok() as u64
}

pub extern "C" fn nt_gdi_set_miter_limit(frame: &SyscallFrame) -> u64 {
    let Some(mut dc) = dc::get_dc(frame.args[0]) else {
        return 0;
    };
    // Win7 passes FLOAT bits in a DWORD. Its COMISS check rejects values below
    // one, while accepting unordered values (NaN), as well as positive infinity.
    let limit = f32::from_bits(frame.args[1] as u32);
    if limit < 1.0 {
        return 0;
    }
    let old = dc.miter_limit.to_bits();
    dc.miter_limit = limit;
    // The native service commits the new limit before probing this optional
    // output. A failed copy therefore reports failure without undoing the set.
    if frame.args[2] != 0
        && (frame.args[2] & 3 != 0 || user_copy::write_user_object(frame.args[2], &old).is_err())
    {
        return 0;
    }
    1
}
