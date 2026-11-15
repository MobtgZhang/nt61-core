//! Common item dialog geometry, shared by rendering, pointer and keyboard paging.
use crate::graphics::Rect;

pub fn bounds(body: Rect) -> Rect {
    Rect::new(
        body.x + 6,
        body.y + 6,
        body.width.saturating_sub(12),
        body.height.saturating_sub(12),
    )
}
pub fn list_bounds(body: Rect) -> Rect {
    Layout::new(body).list
}

pub(super) struct Layout {
    pub bounds: Rect,
    pub back: Rect,
    pub forward: Rect,
    pub address: Rect,
    pub navigation: Rect,
    pub header: Rect,
    pub list: Rect,
    pub scroll: Rect,
    pub new_folder: Rect,
    pub name: Rect,
    pub filter: Rect,
    pub encoding: Rect,
    pub accept: Rect,
    pub cancel: Rect,
    pub status: Rect,
}
impl Layout {
    pub fn new(body: Rect) -> Self {
        let b = bounds(body);
        let nav = if b.width >= 480 { 148 } else { 0 };
        let footer = 122.min(b.height.saturating_sub(88));
        let bottom = b.height.saturating_sub(footer);
        let at = |x: u32, y: u32, w: u32, h: u32| {
            Rect::new(
                b.x + x as i32,
                b.y + y as i32,
                w.min(b.width.saturating_sub(x)),
                h.min(b.height.saturating_sub(y)),
            )
        };
        Self {
            bounds: b,
            back: at(6, 34, 27, 27),
            forward: at(36, 34, 27, 27),
            address: at(76, 35, b.width.saturating_sub(88), 25),
            navigation: at(0, 90, nav, bottom.saturating_sub(90)),
            header: at(nav + 1, 90, b.width.saturating_sub(nav + 17), 24),
            list: at(
                nav + 1,
                114,
                b.width.saturating_sub(nav + 17),
                bottom.saturating_sub(114),
            ),
            scroll: at(
                b.width.saturating_sub(16),
                114,
                16,
                bottom.saturating_sub(114),
            ),
            new_folder: at(100, 66, 110, 24),
            name: at(100, bottom + 10, b.width.saturating_sub(116), 25),
            filter: at(100, bottom + 42, b.width.saturating_sub(116), 25),
            encoding: at(
                12,
                b.height.saturating_sub(37),
                b.width.saturating_sub(244),
                25,
            ),
            accept: at(
                b.width.saturating_sub(224),
                b.height.saturating_sub(37),
                100,
                27,
            ),
            cancel: at(
                b.width.saturating_sub(112),
                b.height.saturating_sub(37),
                100,
                27,
            ),
            status: at(
                12,
                bottom.saturating_sub(22),
                b.width.saturating_sub(24),
                20,
            ),
        }
    }
}
