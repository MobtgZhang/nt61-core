//! Non-client geometry shared by the desktop renderer and pointer routing.
use super::WindowState;
use crate::graphics::Rect;

pub const FRAME_BORDER: i32 = 6;
pub const CAPTION_HEIGHT: i32 = 30;
pub const MIN_WINDOW_WIDTH: i32 = 240;
pub const MIN_WINDOW_HEIGHT: i32 = 120;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResizeEdge {
    Left,
    Right,
    Top,
    Bottom,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowCursor {
    Arrow,
    Horizontal,
    Vertical,
    NorthWestSouthEast,
    NorthEastSouthWest,
}

impl ResizeEdge {
    pub fn cursor(self) -> WindowCursor {
        match self {
            Self::Left | Self::Right => WindowCursor::Horizontal,
            Self::Top | Self::Bottom => WindowCursor::Vertical,
            Self::TopLeft | Self::BottomRight => WindowCursor::NorthWestSouthEast,
            Self::TopRight | Self::BottomLeft => WindowCursor::NorthEastSouthWest,
        }
    }

    pub(super) fn resize(
        self,
        start: (i32, i32, i32, i32),
        dx: i32,
        dy: i32,
    ) -> (i32, i32, i32, i32) {
        let (mut x, mut y, mut width, mut height) = start;
        if matches!(self, Self::Left | Self::TopLeft | Self::BottomLeft) {
            width = width.saturating_sub(dx).max(MIN_WINDOW_WIDTH);
            x = x.saturating_add(start.2.saturating_sub(width));
        } else if matches!(self, Self::Right | Self::TopRight | Self::BottomRight) {
            width = width.saturating_add(dx).max(MIN_WINDOW_WIDTH);
        }
        if matches!(self, Self::Top | Self::TopLeft | Self::TopRight) {
            height = height.saturating_sub(dy).max(MIN_WINDOW_HEIGHT);
            y = y.saturating_add(start.3.saturating_sub(height));
        } else if matches!(self, Self::Bottom | Self::BottomLeft | Self::BottomRight) {
            height = height.saturating_add(dy).max(MIN_WINDOW_HEIGHT);
        }
        (x, y, width, height)
    }
}

impl WindowState {
    pub fn bounds(&self) -> Rect {
        Rect::new(
            self.x,
            self.y,
            self.width.max(0) as u32,
            self.height.max(0) as u32,
        )
    }

    pub fn caption_control_group(&self) -> Rect {
        // The close button ends at the inner right frame. The group attaches
        // to the top frame, rather than floating at the title text baseline.
        Rect::new(
            self.x + self.width - FRAME_BORDER - 102,
            self.y + 1,
            102,
            20,
        )
    }

    pub fn caption_controls(&self) -> [Rect; 3] {
        let b = self.caption_control_group();
        [
            Rect::new(b.x, b.y, 29, b.height),
            Rect::new(b.x + 29, b.y, 29, b.height),
            Rect::new(b.x + 58, b.y, 44, b.height),
        ]
    }

    pub fn caption_control_at(&self, x: i32, y: i32) -> Option<u8> {
        if self.minimized {
            return None;
        }
        self.caption_controls()
            .iter()
            .position(|b| b.contains(x, y))
            .map(|i| i as u8)
    }

    pub fn resize_edge_at(&self, x: i32, y: i32) -> Option<ResizeEdge> {
        if self.minimized
            || self.maximized
            || !self.bounds().contains(x, y)
            || self.caption_control_at(x, y).is_some()
        {
            return None;
        }
        let local_x = x - self.x;
        let local_y = y - self.y;
        let left = local_x < FRAME_BORDER;
        let right = local_x >= self.width - FRAME_BORDER;
        let top = local_y < FRAME_BORDER;
        let bottom = local_y >= self.height - FRAME_BORDER;
        // Make corner grips a little longer along each adjacent frame edge.
        let corner = 16;
        if (left && local_y < corner) || (top && local_x < corner) {
            Some(ResizeEdge::TopLeft)
        } else if (right && local_y < corner) || (top && local_x >= self.width - corner) {
            Some(ResizeEdge::TopRight)
        } else if (left && local_y >= self.height - corner) || (bottom && local_x < corner) {
            Some(ResizeEdge::BottomLeft)
        } else if (right && local_y >= self.height - corner)
            || (bottom && local_x >= self.width - corner)
        {
            Some(ResizeEdge::BottomRight)
        } else if left {
            Some(ResizeEdge::Left)
        } else if right {
            Some(ResizeEdge::Right)
        } else if top {
            Some(ResizeEdge::Top)
        } else if bottom {
            Some(ResizeEdge::Bottom)
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ResizeDrag {
    pub edge: ResizeEdge,
    pub origin: (i32, i32),
    pub geometry: (i32, i32, i32, i32),
}
