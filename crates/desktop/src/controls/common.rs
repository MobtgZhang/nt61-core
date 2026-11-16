//! Native common-control input and notifications. These carry control IDs and
//! stable item handles, so sorting or inserting rows does not retarget actions.
use crate::graphics::{Rect, Surface};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommonKey {
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Space,
    Enter,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub control: bool,
    pub shift: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommonNotification {
    SelectionChanged {
        control: u32,
        item: u64,
    },
    Activated {
        control: u32,
        item: u64,
    },
    Expanded {
        control: u32,
        item: u64,
        expanded: bool,
    },
    Checked {
        control: u32,
        item: u64,
        checked: bool,
    },
    Command {
        control: u32,
        command: u32,
    },
    DropDown {
        control: u32,
        command: u32,
    },
    ColumnClick {
        control: u32,
        column: usize,
    },
}

pub(super) const INK: u32 = 0xff000000;
pub(super) const DISABLED: u32 = 0xff6d6d6d;
pub(super) const BORDER: u32 = 0xffa0a0a0;
pub(super) const SELECTED: u32 = 0xff3399ff;
pub(super) const INACTIVE: u32 = 0xffe4e4e4;
pub(super) const ROW_HEIGHT: u32 = 24;

pub(super) fn fill(frame: &mut Surface, rect: Rect, color: u32) {
    frame.fill_rect(rect.x, rect.y, rect.width, rect.height, color);
}

pub(super) fn border(frame: &mut Surface, rect: Rect, color: u32) {
    if rect.width == 0 || rect.height == 0 {
        return;
    }
    frame.fill_rect(rect.x, rect.y, rect.width, 1, color);
    frame.fill_rect(
        rect.x,
        rect.y + rect.height as i32 - 1,
        rect.width,
        1,
        color,
    );
    frame.fill_rect(rect.x, rect.y, 1, rect.height, color);
    frame.fill_rect(
        rect.x + rect.width as i32 - 1,
        rect.y,
        1,
        rect.height,
        color,
    );
}

pub(super) fn label<F: Fn(&mut Surface, &str, Rect, u32)>(
    frame: &mut Surface,
    draw: &F,
    text: &str,
    rect: Rect,
    bounds: Rect,
    color: u32,
) {
    if let Some(clipped) = rect.intersection(&bounds) {
        draw(frame, text, clipped, color);
    }
}

pub(super) fn check_box(frame: &mut Surface, rect: Rect, checked: bool, enabled: bool) {
    fill(frame, rect, 0xffffffff);
    border(frame, rect, if enabled { BORDER } else { INACTIVE });
    if checked {
        let color = if enabled { INK } else { DISABLED };
        for n in 0..4 {
            frame.fill_rect(rect.x + 2 + n, rect.y + 5 + n, 2, 2, color);
        }
        for n in 0..6 {
            frame.fill_rect(rect.x + 5 + n, rect.y + 8 - n, 2, 2, color);
        }
    }
}

/// Preserve a caller's damage clip, and restore the unclipped fast path when
/// the caller was drawing the complete surface.
pub(super) fn restore_clip(frame: &mut Surface, previous: Rect) {
    let full = Rect::new(0, 0, frame.width(), frame.height());
    frame.set_draw_clip(if previous == full {
        None
    } else {
        Some(previous)
    });
}
