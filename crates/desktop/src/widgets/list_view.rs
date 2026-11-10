use alloc::string::String;
use alloc::vec::Vec;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListView {
    pub id: u32,
    pub items: Vec<String>,
    pub selected: Option<usize>,
    pub visible: bool,
    pub enabled: bool,
}

impl ListView {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            items: Vec::new(),
            selected: None,
            visible: true,
            enabled: true,
        }
    }

    pub fn with_items<'a, I>(id: u32, items: I) -> Self
    where
        I: IntoIterator<Item = &'a str>,
    {
        let mut list = Self::new(id);
        for item in items {
            list.items.push(String::from(item));
        }
        list
    }

    pub fn select(&mut self, index: usize) -> bool {
        if !self.enabled || index >= self.items.len() {
            return false;
        }
        self.selected = Some(index);
        true
    }

    pub fn selected_item(&self) -> Option<&str> {
        self.selected
            .and_then(|index| self.items.get(index).map(String::as_str))
    }

    pub fn clear_selection(&mut self) {
        self.selected = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_view_selects_valid_items_and_rejects_invalid_items() {
        let mut list = ListView::with_items(1, ["Details", "Icons"]);
        assert!(list.select(1));
        assert_eq!(list.selected_item(), Some("Icons"));
        assert!(!list.select(2));
        list.clear_selection();
        assert_eq!(list.selected_item(), None);
    }
}
