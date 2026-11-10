use crate::graphics::Rect;
use alloc::vec::Vec;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Panel {
    pub id: u32,
    pub bounds: Rect,
    pub visible: bool,
    pub enabled: bool,
    pub children: Vec<(u32, Rect)>,
}

impl Panel {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            bounds: Rect::new(0, 0, 0, 0),
            visible: true,
            enabled: true,
            children: Vec::new(),
        }
    }

    pub fn add_child(&mut self, id: u32, bounds: Rect) -> bool {
        if bounds.width == 0 || bounds.height == 0 {
            return false;
        }
        self.children.push((id, bounds));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_tracks_child_geometry() {
        let mut panel = Panel::new(1);
        panel.bounds = Rect::new(10, 10, 200, 100);
        panel.add_child(2, Rect::new(20, 20, 50, 20));
        assert_eq!(panel.children.len(), 1);
        assert!(panel.bounds.contains(30, 30));
    }
}
