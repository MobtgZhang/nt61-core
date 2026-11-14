//! Client geometry shared by painting and input. Narrow windows collapse the tree.
use crate::graphics::Rect;

pub const ROW_HEIGHT: u32 = 24;
#[derive(Clone, Copy, Debug)]
pub struct Layout {
    pub body: Rect,
    pub back: Rect,
    pub forward: Rect,
    pub address: Rect,
    pub refresh: Rect,
    pub search: Rect,
    pub commands: Rect,
    pub navigation: Rect,
    pub header: Rect,
    pub list: Rect,
    pub scroll: Rect,
    pub details: Rect,
    pub status: Rect,
}
impl Layout {
    pub fn new(b: Rect) -> Self {
        let search_width = if b.width >= 480 {
            180.min(b.width / 3)
        } else {
            0
        };
        let nav_width = if b.width >= 440 {
            156.min(b.width / 3)
        } else {
            0
        };
        let top = 36.min(b.height);
        let command_h = 30.min(b.height.saturating_sub(top));
        let footer = if b.height >= 220 {
            74
        } else {
            22.min(b.height.saturating_sub(top + command_h))
        };
        let content_h = b.height.saturating_sub(top + command_h + footer);
        let header_h = 24.min(content_h);
        let right_width = b.width.saturating_sub(nav_width);
        let scrollbar = 16.min(right_width);
        let at = |x: u32, y: u32, w: u32, h: u32| Rect::new(b.x + x as i32, b.y + y as i32, w, h);
        let address_width = b
            .width
            .saturating_sub(76 + search_width + if search_width > 0 { 8 } else { 0 });
        let status_h = 22.min(footer);
        Self {
            body: b,
            back: at(4, 5, 26.min(b.width), 26.min(top)),
            forward: at(34, 5, 26.min(b.width.saturating_sub(34)), 26.min(top)),
            address: at(68, 5, address_width.saturating_sub(24), 26.min(top)),
            refresh: at(
                68 + address_width.saturating_sub(24),
                5,
                24.min(address_width),
                26.min(top),
            ),
            search: at(
                b.width.saturating_sub(search_width + 4),
                5,
                search_width,
                26.min(top),
            ),
            commands: at(0, top, b.width, command_h),
            navigation: at(0, top + command_h, nav_width, content_h),
            header: at(
                nav_width,
                top + command_h,
                right_width.saturating_sub(scrollbar),
                header_h,
            ),
            list: at(
                nav_width,
                top + command_h + header_h,
                right_width.saturating_sub(scrollbar),
                content_h.saturating_sub(header_h),
            ),
            scroll: at(
                b.width.saturating_sub(scrollbar),
                top + command_h + header_h,
                scrollbar,
                content_h.saturating_sub(header_h),
            ),
            details: at(
                0,
                b.height.saturating_sub(footer),
                b.width,
                footer.saturating_sub(status_h),
            ),
            status: at(0, b.height.saturating_sub(status_h), b.width, status_h),
        }
    }
    pub fn command(&self, index: usize) -> Rect {
        const WIDTHS: [u32; 4] = [90, 70, 100, 76];
        let offset: u32 = WIDTHS[..index.min(4)].iter().sum();
        Rect::new(
            self.commands.x + offset as i32,
            self.commands.y + 2,
            WIDTHS
                .get(index)
                .copied()
                .unwrap_or(0)
                .min(self.commands.width.saturating_sub(offset)),
            self.commands.height.saturating_sub(4),
        )
    }
    pub fn row(&self, index: usize) -> Rect {
        Rect::new(
            self.list.x + 4,
            self.list.y + index as i32 * ROW_HEIGHT as i32,
            self.list.width.saturating_sub(8),
            ROW_HEIGHT,
        )
    }
    pub fn capacity(&self) -> usize {
        (self.list.height / ROW_HEIGHT) as usize
    }
    pub fn column(&self, index: usize) -> Rect {
        let w = self.header.width;
        let widths = if w >= 460 {
            [w * 42 / 100, w * 23 / 100, w * 22 / 100]
        } else {
            [w * 64 / 100, 0, 0]
        };
        let offset: u32 = widths[..index.min(3)].iter().sum();
        let width = widths
            .get(index)
            .copied()
            .unwrap_or(w.saturating_sub(offset));
        Rect::new(
            self.header.x + offset as i32,
            self.header.y,
            width,
            self.header.height,
        )
    }
}

/// Geometry shared by the deletion prompt painter and pointer hit testing.
pub fn delete_dialog(b:Rect)->(Rect,Rect,Rect){
    let width=440.min(b.width);let height=158.min(b.height);
    let dialog=Rect::new(b.x+(b.width-width)as i32/2,b.y+(b.height-height)as i32/2,width,height);
    let button_width=88.min(width/3);let y=dialog.y+height.saturating_sub(34)as i32;
    let no=Rect::new(dialog.x+width.saturating_sub(button_width+12)as i32,y,button_width,24);
    let yes=Rect::new(no.x-button_width as i32-10,y,button_width,24);
    (dialog,yes,no)
}
