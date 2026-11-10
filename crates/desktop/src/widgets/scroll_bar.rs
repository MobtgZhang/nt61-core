#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScrollBar {
    pub id: u32,
    pub visible: bool,
    pub enabled: bool,
    pub minimum: i32,
    pub maximum: i32,
    pub value: i32,
    pub page: i32,
}

impl ScrollBar {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            visible: true,
            enabled: true,
            minimum: 0,
            maximum: 100,
            value: 0,
            page: 10,
        }
    }

    pub fn set_range(&mut self, minimum: i32, maximum: i32) {
        self.minimum = minimum;
        self.maximum = maximum.max(minimum);
        self.value = self.value.clamp(self.minimum, self.maximum);
    }

    pub fn set_value(&mut self, value: i32) -> bool {
        if !self.enabled || value < self.minimum || value > self.maximum {
            return false;
        }
        self.value = value;
        true
    }

    pub fn scroll_by(&mut self, delta: i32) {
        self.value = self
            .value
            .saturating_add(delta)
            .clamp(self.minimum, self.maximum);
    }

    pub fn page_down(&mut self) {
        self.scroll_by(self.page);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scroll_bar_clamps_to_range_and_pages_down() {
        let mut bar = ScrollBar::new(1);
        bar.set_range(0, 20);
        assert!(!bar.set_value(25));
        assert_eq!(bar.value, 0);
        assert!(bar.set_value(14));
        assert_eq!(bar.value, 14);
        bar.value = 14;
        bar.page_down();
        assert_eq!(bar.value, 20);
    }
}
