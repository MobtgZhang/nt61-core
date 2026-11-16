use alloc::string::String;
use alloc::vec::Vec;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuKind {
    Start,
    Context,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuItem {
    pub id: u32,
    pub text: String,
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Menu {
    pub id: u32,
    pub kind: MenuKind,
    pub items: Vec<MenuItem>,
    pub selected: Option<u32>,
    pub visible: bool,
}

impl Menu {
    pub fn new(id: u32, kind: MenuKind) -> Self {
        Self {
            id,
            kind,
            items: Vec::new(),
            selected: None,
            visible: false,
        }
    }

    pub fn add_item(&mut self, id: u32, text: &str) {
        self.items.push(MenuItem {
            id,
            text: String::from(text),
            enabled: true,
        });
    }

    pub fn add_disabled_item(&mut self, id: u32, text: &str) {
        self.items.push(MenuItem {
            id,
            text: String::from(text),
            enabled: false,
        });
    }

    pub fn select(&mut self, id: u32) -> bool {
        if self.items.iter().any(|item| item.id == id && item.enabled) {
            self.selected = Some(id);
            return true;
        }
        false
    }

    pub fn select_next(&mut self) -> Option<u32> {
        if self.items.is_empty() {
            return None;
        }
        let start = self
            .selected
            .and_then(|id| self.items.iter().position(|item| item.id == id))
            .map_or(0, |index| (index + 1) % self.items.len());
        for offset in 0..self.items.len() {
            let item = &self.items[(start + offset) % self.items.len()];
            if item.enabled {
                self.selected = Some(item.id);
                return self.selected;
            }
        }
        self.selected = None;
        None
    }

    pub fn activate(&mut self) -> Option<u32> {
        self.selected
            .filter(|id| self.items.iter().any(|item| item.id == *id && item.enabled))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_selects_enabled_items_and_skips_disabled_items() {
        let mut menu = Menu::new(1, MenuKind::Context);
        menu.add_item(10, "Open");
        menu.add_disabled_item(11, "Delete");
        menu.add_item(12, "Properties");

        assert!(menu.select(10));
        assert!(!menu.select(11));
        assert_eq!(menu.select_next(), Some(12));
        assert_eq!(menu.activate(), Some(12));
    }
    #[test]
    fn unselected_menu_starts_at_first_enabled_item() {
        let mut menu = Menu::new(1, MenuKind::Context);
        menu.add_item(10, "Open");
        menu.add_item(11, "Save");
        assert_eq!(menu.select_next(), Some(10));
    }

    #[test]
    fn disabled_selection_cannot_activate() {
        let mut menu = Menu::new(1, MenuKind::Context);
        menu.add_item(10, "Open");
        menu.select(10);
        menu.items[0].enabled = false;
        assert_eq!(menu.activate(), None);
    }
}
