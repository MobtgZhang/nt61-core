//! Drawing bounds are device state, independent of SaveDC's drawing attributes.
//! The DC table guard serializes access, including the interior Cell used by
//! raster operations which otherwise only borrow the DC immutably.

use super::PointL;
use super::super::{clipping, objects::Rect, transforms};
use crate::{arch::common::syscall_frame::SyscallFrame, mm::user_copy};
use core::cell::Cell;

const RESET: u32 = 1;
const ACCUMULATE: u32 = 2;
const SET: u32 = 3;
const ENABLE: u32 = 4;
const DISABLE: u32 = 8;
const WINDOW_MANAGER: u32 = 0x8000;

#[derive(Clone, Copy)]
struct State {
    rectangles: [Rect; 2],
    enabled: [bool; 2],
}

pub struct DrawingBounds(Cell<State>);

impl Default for DrawingBounds {
    fn default() -> Self {
        Self(Cell::new(State {
            rectangles: [Rect::new(0, 0, 0, 0); 2],
            enabled: [false; 2],
        }))
    }
}

fn union(destination: &mut Rect, input: Rect) {
    if input.is_empty() {
        return;
    }
    if destination.is_empty() {
        *destination = input;
    } else {
        destination.left = destination.left.min(input.left);
        destination.top = destination.top.min(input.top);
        destination.right = destination.right.max(input.right);
        destination.bottom = destination.bottom.max(input.bottom);
    }
}

impl DrawingBounds {
    pub fn record_pixel(&self, x: i32, y: i32) {
        let mut state = self.0.get();
        if !state.enabled.contains(&true) {
            return;
        }
        let (Some(right), Some(bottom)) = (x.checked_add(1), y.checked_add(1)) else {
            return;
        };
        for index in 0..2 {
            if state.enabled[index] {
                union(&mut state.rectangles[index], Rect::new(x, y, right, bottom));
            }
        }
        self.0.set(state);
    }
}

fn transform_rect(rect: Rect, transform: super::Xform) -> Option<Rect> {
    let first = transforms::apply(transform, PointL::new(rect.left, rect.top))?;
    let last = transforms::apply(transform, PointL::new(rect.right, rect.bottom))?;
    Some(Rect::new(first.x, first.y, last.x, last.y))
}

pub extern "C" fn nt_gdi_set_bounds_rect(frame: &SyscallFrame) -> u64 {
    let flags = frame.args[2] as u32;
    if flags & (ENABLE | DISABLE) == ENABLE | DISABLE {
        return 0;
    }
    let Some(dc) = super::get_dc(frame.args[0]) else {
        return 0;
    };
    let index = usize::from(flags & WINDOW_MANAGER != 0);
    let mut state = dc.drawing_bounds.0.get();
    let previous = if state.enabled.contains(&true) { ENABLE } else { DISABLE }
        | if state.rectangles[0].is_empty() { RESET } else { SET }
        | (flags & WINDOW_MANAGER);
    if flags & RESET != 0 {
        state.rectangles[index] = Rect::new(0, 0, 0, 0);
        dc.drawing_bounds.0.set(state);
    }
    if flags & ACCUMULATE != 0 && frame.args[1] != 0 {
        let Ok(mut rect) = user_copy::read_user_object::<Rect>(frame.args[1]) else {
            return 0;
        };
        rect.normalize();
        if index == 0 {
            let Some(transform) = transforms::world_to_device(&dc) else {
                return 0;
            };
            let Some(transformed) = transform_rect(rect, transform) else {
                return 0;
            };
            rect = transformed;
        }
        union(&mut state.rectangles[index], rect);
    }
    if flags & ENABLE != 0 {
        state.enabled[index] = true;
    }
    if flags & DISABLE != 0 {
        state.enabled[index] = false;
    }
    dc.drawing_bounds.0.set(state);
    u64::from(previous)
}

pub extern "C" fn nt_gdi_get_bounds_rect(frame: &SyscallFrame) -> u64 {
    let Some(dc) = super::get_dc(frame.args[0]) else {
        return 0;
    };
    let flags = frame.args[2] as u32;
    let index = usize::from(flags & WINDOW_MANAGER != 0);
    let mut state = dc.drawing_bounds.0.get();
    let mut rect = state.rectangles[index];
    let result = (|| {
        let status = if index == 1 {
            SET
        } else {
            let status = if rect.is_empty() {
                rect = Rect::new(0, 0, 0, 0);
                RESET
            } else {
                let visible = clipping::visible_region(&dc).ok()?.bounds();
                rect.left = rect.left.max(0);
                rect.top = rect.top.max(0);
                rect.right = rect.right.min(visible.right);
                rect.bottom = rect.bottom.min(visible.bottom);
                SET
            };
            rect = transform_rect(rect, transforms::inverse(transforms::world_to_device(&dc)?)?)?;
            status
        };
        user_copy::write_user_object(frame.args[1], &rect).ok()?;
        Some(status)
    })();
    // Reset is honored even when the caller's output buffer faults.
    if flags & RESET != 0 {
        state.rectangles[index] = Rect::new(0, 0, 0, 0);
        dc.drawing_bounds.0.set(state);
    }
    u64::from(result.unwrap_or(0))
}
