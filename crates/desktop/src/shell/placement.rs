//! Single-monitor window placement, checked against Windows 7 SP1 user32.
use super::{ShellModel, SnapSide, WindowState, TASKBAR_HEIGHT};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowShortcut {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DragSnap {
    Side(SnapSide),
    Maximize,
}

impl DragSnap {
    pub(super) fn at(x: i32, y: i32, viewport: (u32, u32)) -> Option<Self> {
        if y <= 0 {
            Some(Self::Maximize)
        } else if y >= viewport.1.saturating_sub(TASKBAR_HEIGHT) as i32 {
            None
        } else if x <= 0 {
            Some(Self::Side(SnapSide::Left))
        } else if x >= viewport.0.saturating_sub(1) as i32 {
            Some(Self::Side(SnapSide::Right))
        } else {
            None
        }
    }
}

impl WindowState {
    pub(super) fn remember_normal_placement(&mut self) {
        if self.restore_geometry.is_none() {
            self.restore_geometry = Some((self.x, self.y, self.width, self.height));
        }
    }

    pub(crate) fn restore_normal_placement(&mut self) {
        if let Some(geometry) = self.restore_geometry.take() {
            (self.x, self.y, self.width, self.height) = geometry;
        }
        self.maximized = false;
        self.snapped = None;
    }

    pub(crate) fn maximize(&mut self, width: u32, height: u32) {
        self.remember_normal_placement();
        self.snapped = None;
        self.maximized = true;
        // SP1 GetWindowRect includes the off-screen sizing frame. The
        // reference capture at 96 DPI reports (-8,-8,816,576) for 800x560.
        let frame_x = super::layout::MAXIMIZED_FRAME_X as i32;
        let frame_y = super::layout::MAXIMIZED_FRAME_Y as i32;
        (self.x, self.y, self.width, self.height) = (
            -frame_x, -frame_y,
            (width as i32).saturating_add(2 * frame_x),
            (height as i32).saturating_add(2 * frame_y),
        );
    }
}

impl ShellModel {
    pub(super) fn toggle_window_maximize(&mut self, id: u32, width: u32, height: u32) {
        if let Some(window) = self.taskbar.windows.iter_mut().find(|w| w.id == id) {
            if window.maximized {
                window.restore_normal_placement();
            } else {
                window.maximize(width, height);
            }
        }
    }

    pub fn window_shortcut(&mut self, shortcut: WindowShortcut) {
        let Some(id) = self.selected_window else { return };
        let Some(window) = self.taskbar.windows.iter_mut().find(|w| w.id == id && !w.minimized) else {
            return;
        };
        match shortcut {
            WindowShortcut::Left | WindowShortcut::Right => {
                let side = if shortcut == WindowShortcut::Left { SnapSide::Left } else { SnapSide::Right };
                match window.snapped {
                    Some(previous) if previous != side => window.restore_normal_placement(),
                    Some(_) => self.snap_window(id, self.viewport.0, match side {
                        SnapSide::Left => SnapSide::Right,
                        SnapSide::Right => SnapSide::Left,
                    }),
                    None => self.snap_window(id, self.viewport.0, side),
                }
            }
            WindowShortcut::Up => {
                if !window.maximized {
                    window.maximize(self.viewport.0, self.viewport.1.saturating_sub(TASKBAR_HEIGHT));
                }
            }
            WindowShortcut::Down => {
                if window.maximized || window.snapped.is_some() {
                    window.restore_normal_placement();
                } else {
                    window.minimized = true;
                    self.selected_window = self.taskbar.windows.iter()
                        .filter(|w| !w.minimized && w.id != id)
                        .max_by_key(|w| self.window_z_order(w.id))
                        .map(|w| w.id);
                }
            }
        }
    }
}
