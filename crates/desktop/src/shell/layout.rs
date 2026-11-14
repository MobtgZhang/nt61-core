//! Shared geometry for shell painting, pointer routing and popup dismissal.
use super::{Flyout, START_BUTTON_WIDTH};
use crate::graphics::Rect;

include!(concat!(env!("OUT_DIR"), "/shell_metrics.rs"));
pub const NOTIFICATION_AREA_WIDTH: u32 = TRAY_WIDTH + CLOCK_WIDTH + SHOW_DESKTOP_WIDTH;
pub const TRAY_SLOT_WIDTH: u32 = 24;
pub const PINNED_WIDTH: u32 = 56;

#[derive(Clone, Copy, Debug)]
pub struct ShellLayout {
    pub width: u32,
    pub height: u32,
}

impl ShellLayout {
    pub fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }
    pub fn taskbar_top(self) -> i32 {
        self.height.saturating_sub(TASKBAR_HEIGHT) as i32
    }
    pub fn start_orb(self) -> Rect {
        // The asset includes transparent glow around the disc. Center the whole
        // canvas so the solid disc fits the taskbar without clipping its bottom.
        let size = START_ORB_SIZE + 2 * START_ORB_GLOW_RADIUS;
        Rect::new(
            (START_BUTTON_WIDTH as i32 - size as i32) / 2,
            self.taskbar_top() + (TASKBAR_HEIGHT as i32 - size as i32) / 2,
            size,
            size,
        )
    }
    pub fn tray_left(self) -> i32 {
        self.width.saturating_sub(NOTIFICATION_AREA_WIDTH) as i32
    }
    // Overflow, Action Center, network, volume, then the separate clock/peek strip.
    pub fn tray(self, index: u32) -> Rect {
        Rect::new(
            self.tray_left() + 10 + index as i32 * TRAY_SLOT_WIDTH as i32,
            self.taskbar_top() + 2,
            TRAY_SLOT_WIDTH,
            TASKBAR_HEIGHT - 3,
        )
    }
    pub fn clock(self) -> Rect {
        Rect::new(
            self.width.saturating_sub(SHOW_DESKTOP_WIDTH + CLOCK_WIDTH) as i32,
            self.taskbar_top() + 1,
            CLOCK_WIDTH,
            TASKBAR_HEIGHT - 1,
        )
    }
    pub fn peek(self) -> Rect {
        Rect::new(
            self.width.saturating_sub(SHOW_DESKTOP_WIDTH) as i32,
            self.taskbar_top(),
            SHOW_DESKTOP_WIDTH,
            TASKBAR_HEIGHT,
        )
    }
    pub fn pinned(self) -> Rect {
        Rect::new(
            START_BUTTON_WIDTH as i32 + 4,
            self.taskbar_top() + 1,
            PINNED_WIDTH - 4,
            TASKBAR_HEIGHT - 2,
        )
    }
    pub fn window_button(self, index: usize, count: usize) -> Rect {
        let left = super::taskbar_program_x(0);
        let available = self.width.saturating_sub(NOTIFICATION_AREA_WIDTH + left);
        let step =
            (available / count.max(1) as u32).min(PROGRAM_BUTTON_WIDTH + super::PROGRAM_BUTTON_GAP);
        Rect::new(
            (left + index as u32 * step) as i32,
            self.taskbar_top() + 1,
            step.saturating_sub(super::PROGRAM_BUTTON_GAP).max(1),
            TASKBAR_HEIGHT - 2,
        )
    }
    pub fn start(self) -> Rect {
        let width = START_MENU_WIDTH.min(self.width.saturating_sub(4));
        let height = START_MENU_HEIGHT.min(self.height.saturating_sub(TASKBAR_HEIGHT + 12));
        Rect::new(2, self.taskbar_top() - height as i32, width, height)
    }
    pub fn left_width(self) -> u32 {
        START_LEFT_WIDTH.min(self.start().width * START_LEFT_WIDTH / START_MENU_WIDTH)
    }
    pub fn program(self, index: usize) -> Rect {
        let s = self.start();
        let row = 38.min(s.height.saturating_sub(90) / 4);
        Rect::new(
            s.x + 8,
            s.y + 12 + index as i32 * row as i32,
            self.left_width().saturating_sub(16),
            row,
        )
    }
    pub fn link(self, index: usize) -> Rect {
        let s = self.start();
        let row = 38.min(s.height.saturating_sub(122) / 7);
        Rect::new(
            s.x + self.left_width() as i32 + 6,
            s.y + 80 + index as i32 * row as i32,
            s.width.saturating_sub(self.left_width() + 16),
            row,
        )
    }
    pub fn program_page(self,next:bool)->Rect {let s=self.start();let w=(self.left_width().saturating_sub(16))/2;Rect::new(s.x+8+if next {w as i32}else{0},s.y+s.height as i32-116,w,28)}
    pub fn all_programs(self) -> Rect {
        let s = self.start();
        Rect::new(
            s.x + 8,
            s.y + s.height as i32 - 78,
            self.left_width().saturating_sub(16),
            30,
        )
    }
    pub fn search(self) -> Rect {
        let s = self.start();
        Rect::new(
            s.x + 12,
            s.y + s.height as i32 - 36,
            self.left_width().saturating_sub(24),
            SEARCH_HEIGHT,
        )
    }
    pub fn shutdown(self) -> Rect {
        let s = self.start();
        Rect::new(
            s.x + self.left_width() as i32 + 12,
            s.y + s.height as i32 - 36,
            s.width.saturating_sub(self.left_width() + 24 + 22),
            SEARCH_HEIGHT,
        )
    }
    pub fn power_arrow(self) -> Rect {
        let b = self.shutdown();
        Rect::new(b.x + b.width as i32, b.y, 22, b.height)
    }
    pub fn flyout(self, kind: Flyout) -> Rect {
        let (width, height, anchor) = match kind {
            Flyout::Overflow => (220, 148, self.tray(0)),
            Flyout::ActionCenter => (320, 208, self.tray(1)),
            Flyout::Network => (300, 208, self.tray(2)),
            Flyout::Volume => (160, 264, self.tray(3)),
            Flyout::Clock => (310, 316, self.clock()),
            Flyout::Power => (180, 148, self.power_arrow()),
        };
        let width = width.min(self.width);
        let height = height.min(self.height.saturating_sub(TASKBAR_HEIGHT + 4));
        let x = if kind == Flyout::Power {
            anchor.x + anchor.width as i32
        } else {
            anchor.x + anchor.width as i32 / 2 - width as i32 / 2
        };
        let y = if kind == Flyout::Power {
            anchor.y + anchor.height as i32 - height as i32
        } else {
            self.taskbar_top() - height as i32 - 4
        };
        Rect::new(
            x.clamp(0, self.width.saturating_sub(width) as i32),
            y.clamp(
                0,
                self.height.saturating_sub(height + TASKBAR_HEIGHT) as i32,
            ),
            width,
            height,
        )
    }
    pub fn volume_track(self) -> Rect {
        let b = self.flyout(Flyout::Volume);
        Rect::new(
            b.x + b.width as i32 / 2 - 10,
            b.y + 50,
            20,
            b.height.saturating_sub(124),
        )
    }
    pub fn flyout_footer(self, kind: Flyout) -> Rect {
        let b = self.flyout(kind);
        Rect::new(
            b.x + 8,
            b.y + b.height as i32 - 34,
            b.width.saturating_sub(16),
            26,
        )
    }
}
