use crate::resources::ImageResource;
use crate::theme::Theme;
use alloc::string::String;
use alloc::vec::Vec;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DesktopItem {
    pub name: String,
    pub icon: ImageResource,
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortMode {
    Name,
    Type,
    Size,
    Date,
}

#[derive(Clone, Debug)]
pub struct DesktopState {
    pub items: Vec<DesktopItem>,
    pub selected: Vec<usize>,
    pub sort_mode: SortMode,
}

impl DesktopState {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            selected: Vec::new(),
            sort_mode: SortMode::Name,
        }
    }

    pub fn add(&mut self, name: &str, icon: ImageResource, x: i32, y: i32) {
        self.items.push(DesktopItem {
            name: String::from(name),
            icon,
            x,
            y,
        });
    }

    pub fn sort(&mut self) {
        self.items.sort_by(|left, right| match self.sort_mode {
            SortMode::Name => left.name.cmp(&right.name),
            SortMode::Type => left.icon.cmp(&right.icon).then(left.name.cmp(&right.name)),
            SortMode::Size | SortMode::Date => left.name.cmp(&right.name),
        });
    }

    pub fn hit_test(&self, x: i32, y: i32) -> Option<usize> {
        self.items.iter().position(|item| {
            (0..=96).contains(&x.saturating_sub(item.x))
                && (0..=96).contains(&y.saturating_sub(item.y))
        })
    }

    pub fn select(&mut self, index: usize) {
        self.selected.clear();
        self.selected.push(index);
    }

    pub fn toggle_select(&mut self, index: usize) {
        if let Some(position) = self.selected.iter().position(|value| *value == index) {
            self.selected.remove(position);
        } else {
            self.selected.push(index);
        }
    }
}

impl Default for DesktopState {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowState {
    pub title: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub minimized: bool,
    pub maximized: bool,
}

#[derive(Clone, Debug)]
pub struct TaskbarState {
    pub windows: Vec<WindowState>,
    pub theme: Theme,
}

impl TaskbarState {
    pub fn new(theme: Theme) -> Self {
        Self {
            windows: Vec::new(),
            theme,
        }
    }

    pub fn add_window(&mut self, title: &str, x: i32, y: i32, width: i32, height: i32) {
        self.windows.push(WindowState {
            title: String::from(title),
            x,
            y,
            width,
            height,
            minimized: false,
            maximized: false,
        });
    }

    pub fn minimize_all(&mut self) {
        for window in &mut self.windows {
            window.minimized = true;
        }
    }

    pub fn restore_all(&mut self) {
        for window in &mut self.windows {
            window.minimized = false;
        }
    }

    pub fn show_desktop(&mut self) {
        self.minimize_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Theme;

    #[test]
    fn desktop_items_support_name_sorting_and_hit_testing() {
        let mut desktop = DesktopState::new();
        desktop.add("Computer", ImageResource::Computer, 8, 8);
        desktop.add("Recycle Bin", ImageResource::RecycleBin, 8, 112);
        desktop.sort();
        assert_eq!(desktop.items[0].name, "Computer");
        assert_eq!(desktop.hit_test(10, 12), Some(0));
        assert_eq!(desktop.hit_test(10, 116), Some(1));
        assert_eq!(desktop.hit_test(500, 500), None);
    }

    #[test]
    fn selection_can_be_toggled() {
        let mut desktop = DesktopState::new();
        desktop.add("Folder", ImageResource::Folder, 0, 0);
        desktop.add("Document", ImageResource::Document, 0, 112);
        desktop.select(0);
        desktop.toggle_select(1);
        assert_eq!(desktop.selected, vec![0, 1]);
        desktop.toggle_select(1);
        assert_eq!(desktop.selected, vec![0]);
    }

    #[test]
    fn taskbar_show_desktop_minimizes_then_restores() {
        let mut taskbar = TaskbarState::new(Theme::windows7_basic());
        taskbar.add_window("Notepad", 20, 20, 400, 300);
        taskbar.add_window("CMD", 40, 40, 500, 300);
        taskbar.show_desktop();
        assert!(taskbar.windows.iter().all(|window| window.minimized));
        taskbar.restore_all();
        assert!(taskbar.windows.iter().all(|window| !window.minimized));
    }
}
