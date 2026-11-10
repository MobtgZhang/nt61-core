use crate::controls::{ControlEvent, ControlKind, ControlTree};
use crate::graphics::Rect;
use crate::resources::ImageResource;
use crate::theme::Theme;
use crate::widgets::textbox::TextBox;

pub const TASKBAR_HEIGHT: u32 = 40;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowKind {
    Generic,
    Notepad,
    CommandPrompt,
    Computer,
    Explorer,
}
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DesktopState {
    pub items: Vec<DesktopItem>,
    pub selected: Vec<usize>,
    pub sort_mode: SortMode,
    pub context_menu_open: bool,
}

impl DesktopState {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            selected: Vec::new(),
            sort_mode: SortMode::Name,
            context_menu_open: false,
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

    pub fn begin_drag(&mut self, index: usize, x: i32, y: i32) -> bool {
        if self.hit_test(x, y) == Some(index) {
            if let Some(item) = self.items.get_mut(index) {
                item.x = x;
                item.y = y;
                return true;
            }
        }
        false
    }

    pub fn auto_arrange(&mut self, width: u32, height: u32) {
        self.sort();
        const CELL_WIDTH: u32 = 96;
        const CELL_HEIGHT: u32 = 112;
        let columns = (width / CELL_WIDTH).max(1) as usize;
        let rows = (height.saturating_sub(40) / CELL_HEIGHT).max(1) as usize;
        for (index, item) in self.items.iter_mut().enumerate() {
            let row = (index / columns).min(rows.saturating_sub(1));
            let column = index % columns;
            item.x = 8 + row as i32 * CELL_WIDTH as i32;
            item.y = 8 + column as i32 * CELL_HEIGHT as i32;
        }
    }

    pub fn open_context_menu(&mut self) {
        self.context_menu_open = true;
    }

    pub fn close_context_menu(&mut self) {
        self.context_menu_open = false;
    }
}

impl Default for DesktopState {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowState {
    pub id: u32,
    pub title: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub minimized: bool,
    pub maximized: bool,
    pub pinned: bool,
    pub group_id: u32,
    pub kind: WindowKind,
    pub restore_geometry: Option<(i32, i32, i32, i32)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskbarState {
    pub windows: Vec<WindowState>,
    pub theme: Theme,
    pub notifications: Vec<NotificationIcon>,
    pub clock: String,
}

impl TaskbarState {
    pub fn new(theme: Theme) -> Self {
        Self {
            windows: Vec::new(),
            theme,
            notifications: Vec::new(),
            clock: String::from("12:00"),
        }
    }

    pub fn add_window(&mut self, id: u32, title: &str, x: i32, y: i32, width: i32, height: i32) {
        self.add_window_with_kind(id, title, x, y, width, height, WindowKind::Generic);
    }

    pub fn add_window_with_kind(
        &mut self,
        id: u32,
        title: &str,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        kind: WindowKind,
    ) {
        self.windows.push(WindowState {
            id,
            title: String::from(title),
            x,
            y,
            width,
            height,
            minimized: false,
            maximized: false,
            pinned: false,
            group_id: id,
            kind,
            restore_geometry: None,
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

    pub fn pin(&mut self, id: u32, title: &str) {
        if let Some(window) = self.windows.iter_mut().find(|window| window.id == id) {
            window.pinned = true;
        } else {
            self.windows.push(WindowState {
                id,
                title: String::from(title),
                x: 0,
                y: 0,
                width: 0,
                height: 0,
                minimized: false,
                maximized: false,
                pinned: true,
                group_id: id,
                kind: WindowKind::Generic,
                restore_geometry: None,
            });
        }
    }

    pub fn unpin(&mut self, id: u32) {
        if let Some(window) = self.windows.iter_mut().find(|window| window.id == id) {
            window.pinned = false;
        }
    }

    pub fn move_button(&mut self, id: u32, offset: i32) -> bool {
        let Some(position) = self.windows.iter().position(|window| window.id == id) else {
            return false;
        };
        let target = (position as i32 + offset).clamp(0, self.windows.len() as i32 - 1) as usize;
        if position == target {
            return true;
        }
        let window = self.windows.remove(position);
        self.windows.insert(target, window);
        true
    }

    pub fn group_windows(&mut self, primary: u32, other: &[u32]) {
        let Some(group) = self
            .windows
            .iter()
            .find(|window| window.id == primary)
            .map(|window| window.group_id)
        else {
            return;
        };
        for window in self.windows.iter_mut() {
            if other.contains(&window.id) {
                window.group_id = group;
            }
        }
    }

    pub fn group_members(&self, primary: u32) -> Vec<u32> {
        let Some(primary) = self.windows.iter().find(|window| window.id == primary) else {
            return Vec::new();
        };
        self.windows
            .iter()
            .filter(|window| window.group_id == primary.group_id)
            .map(|window| window.id)
            .collect()
    }

    pub fn button_hit_test(
        &self,
        x: i32,
        pointer_y: i32,
        taskbar_y: i32,
        taskbar_height: u32,
        width: u32,
    ) -> Option<u32> {
        let y = taskbar_y + 10;
        for (index, window) in self.windows.iter().enumerate() {
            let left = 52 + index as i32 * 88;
            if left + 80 <= (width - 220).max(0) as i32
                && (left..left + 80).contains(&x)
                && (y..y + taskbar_height.saturating_sub(20) as i32).contains(&pointer_y)
            {
                return Some(window.id);
            }
        }
        None
    }

    pub fn add_notification(&mut self, id: u32, text: &str) {
        self.notifications.push(NotificationIcon {
            id,
            text: String::from(text),
        });
    }

    pub fn update_notification(&mut self, id: u32, text: &str) {
        if let Some(icon) = self.notifications.iter_mut().find(|icon| icon.id == id) {
            icon.text = String::from(text);
        }
    }

    pub fn remove_notification(&mut self, id: u32) {
        self.notifications.retain(|icon| icon.id != id);
    }

    pub fn show_desktop_peek(&self) -> bool {
        self.windows.iter().any(|window| !window.minimized)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotificationIcon {
    pub id: u32,
    pub text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartProgram {
    Notepad,
    CommandPrompt,
    Computer,
    Explorer,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartMenu {
    pub open: bool,
    pub selected: Option<StartProgram>,
    pub query: String,
    pub highlighted: usize,
}

impl StartMenu {
    pub fn new() -> Self {
        Self {
            open: false,
            selected: None,
            query: String::new(),
            highlighted: 0,
        }
    }

    pub fn toggle(&mut self) {
        self.open = !self.open;
        if !self.open {
            self.selected = None;
        }
    }

    pub fn close(&mut self) {
        self.open = false;
        self.selected = None;
        self.query.clear();
        self.highlighted = 0;
    }

    pub fn all_programs(&self) -> [StartProgram; 4] {
        [
            StartProgram::Notepad,
            StartProgram::CommandPrompt,
            StartProgram::Computer,
            StartProgram::Explorer,
        ]
    }

    pub fn search(&self) -> Vec<StartProgram> {
        let query = self.query.to_ascii_lowercase();
        if query.is_empty() {
            return Vec::new();
        }
        self.all_programs()
            .into_iter()
            .filter(|program| program.title().to_ascii_lowercase().contains(&query))
            .collect()
    }

    pub fn set_query(&mut self, query: &str) {
        self.query = String::from(query);
        self.highlighted = 0;
        self.selected = self.search().first().copied();
    }

    pub fn move_highlight(&mut self, offset: i32) {
        let count = if self.query.is_empty() {
            self.all_programs().len()
        } else {
            self.search().len()
        };
        if count == 0 {
            return;
        }
        let current = self.highlighted as i32 + offset;
        self.highlighted = current.rem_euclid(count as i32) as usize;
    }

    pub fn confirm(&mut self) -> Option<StartProgram> {
        let results = if self.query.is_empty() {
            self.all_programs().to_vec()
        } else {
            self.search()
        };
        let selected = results.get(self.highlighted).copied();
        if let Some(selected) = selected {
            self.selected = Some(selected);
        }
        selected
    }
}

impl StartProgram {
    pub fn title(self) -> &'static str {
        match self {
            StartProgram::Notepad => "Notepad",
            StartProgram::CommandPrompt => "Command Prompt",
            StartProgram::Computer => "Computer",
            StartProgram::Explorer => "Windows Explorer",
        }
    }
}

impl Default for StartMenu {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellModel {
    pub desktop: DesktopState,
    pub taskbar: TaskbarState,
    pub start_menu: StartMenu,
    pub files: crate::file::FileSystem,
    pub next_window_id: u32,
    pub selected_window: Option<u32>,
    pub desktop_visible: bool,
    pub controls: ControlTree,
    pub textboxes: Vec<TextBox>,
    pub drag_window: Option<u32>,
    pub drag_offset: (i32, i32),
}

impl ShellModel {
    pub fn new(theme: Theme) -> Self {
        Self {
            desktop: DesktopState::new(),
            taskbar: TaskbarState::new(theme),
            start_menu: StartMenu::new(),
            files: crate::file::FileSystem::new(),
            next_window_id: 1,
            selected_window: None,
            desktop_visible: true,
            controls: ControlTree::new(),
            textboxes: Vec::new(),
            drag_window: None,
            drag_offset: (0, 0),
        }
    }

    pub fn set_theme(&mut self, kind: crate::theme::ThemeKind, width: u32, height: u32) {
        self.taskbar.theme = Theme::for_theme(kind);
        self.refresh_controls(width, height);
    }

    pub fn refresh_controls(&mut self, width: u32, height: u32) {
        self.controls = self.build_controls(width, height);
    }

    fn build_controls(&self, width: u32, height: u32) -> ControlTree {
        let mut controls = ControlTree::new();
        let top = height.saturating_sub(TASKBAR_HEIGHT);
        controls.add(
            CONTROL_START,
            ControlKind::Button,
            Rect::new(2, top as i32, 38, TASKBAR_HEIGHT),
        );
        for (index, window) in self.taskbar.windows.iter().enumerate() {
            let x = 48 + index as u32 * 92;
            if x + 86 > width.saturating_sub(130) {
                break;
            }
            controls.add(
                CONTROL_TASKBAR_BASE + index as u32,
                if window.minimized {
                    ControlKind::Status
                } else {
                    ControlKind::Button
                },
                Rect::new(
                    x as i32,
                    top as i32 + 4,
                    86,
                    TASKBAR_HEIGHT.saturating_sub(9),
                ),
            );
        }
        for (index, item) in self.desktop.items.iter().enumerate() {
            controls.add(
                CONTROL_DESKTOP_BASE + index as u32,
                ControlKind::Image,
                Rect::new(item.x, item.y, 80, 80),
            );
        }
        if self.start_menu.open {
            let menu_y = height.saturating_sub(TASKBAR_HEIGHT + 374);
            for index in 0..4 {
                let y = menu_y + 12 + index as u32 * 38;
                controls.add(
                    CONTROL_MENU_BASE + index as u32,
                    ControlKind::Button,
                    Rect::new(8, y as i32, 180, 32),
                );
            }
        }
        controls
    }

    pub fn handle_control_event(&mut self, event: ControlEvent) -> bool {
        match event {
            ControlEvent::Pressed(id) => self.select_control(id),
            ControlEvent::Activated(id) => self.activate_control(id),
            ControlEvent::Submit if self.start_menu.open => {
                let launched = self.start_menu.confirm().is_some();
                if launched {
                    self.launch_selected_start_program();
                }
                true
            }
            ControlEvent::Cancel => {
                self.start_menu.close();
                self.desktop.close_context_menu();
                true
            }
            _ => false,
        }
    }

    fn select_control(&mut self, id: u32) -> bool {
        if id >= CONTROL_DESKTOP_BASE {
            let index = (id - CONTROL_DESKTOP_BASE) as usize;
            if index < self.desktop.items.len() {
                self.desktop.select(index);
                return true;
            }
        }
        false
    }

    fn activate_control(&mut self, id: u32) -> bool {
        if id == CONTROL_START {
            self.start_menu.toggle();
            return true;
        }
        if id >= CONTROL_DESKTOP_BASE {
            let index = (id - CONTROL_DESKTOP_BASE) as usize;
            if index < self.desktop.items.len() {
                self.desktop.select(index);
                return true;
            }
        }
        if id >= CONTROL_TASKBAR_BASE {
            let index = (id - CONTROL_TASKBAR_BASE) as usize;
            if let Some(window) = self.taskbar.windows.get_mut(index) {
                window.minimized = false;
                self.selected_window = Some(window.id);
                return true;
            }
        }
        if id >= CONTROL_MENU_BASE && self.start_menu.open {
            let index = (id - CONTROL_MENU_BASE) as usize;
            let program = self.start_menu.all_programs().get(index).copied();
            if let Some(program) = program {
                self.start_menu.selected = Some(program);
                self.launch_selected_start_program();
                return true;
            }
        }
        false
    }

    pub fn open_window(&mut self, title: &str, x: i32, y: i32, width: i32, height: i32) -> u32 {
        self.open_window_with_kind(title, x, y, width, height, WindowKind::Generic)
    }

    pub fn open_window_with_kind(
        &mut self,
        title: &str,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        kind: WindowKind,
    ) -> u32 {
        let id = self.next_window_id;
        self.next_window_id += 1;
        self.taskbar
            .add_window_with_kind(id, title, x, y, width, height, kind);
        self.selected_window = Some(id);
        self.focus_window(id);
        id
    }

    pub fn close_window(&mut self, id: u32) {
        self.taskbar.windows.retain(|window| window.id != id);
        self.textboxes.retain(|textbox| textbox.id != id);
        if self.selected_window == Some(id) {
            self.selected_window = self.taskbar.windows.first().map(|window| window.id);
        }
        if let Some(selected) = self.selected_window {
            self.focus_window(selected);
        }
    }

    pub fn select_window(&mut self, id: u32) {
        if self.taskbar.windows.iter().any(|window| window.id == id) {
            self.selected_window = Some(id);
            self.focus_window(id);
        }
    }

    fn focus_window(&mut self, id: u32) {
        for textbox in &mut self.textboxes {
            textbox.focused = textbox.id == id;
        }
        if self.textboxes.iter().all(|textbox| textbox.id != id) {
            let mut textbox = TextBox::new(id);
            textbox.focused = true;
            self.textboxes.push(textbox);
        }
    }

    pub fn textbox(&self, id: u32) -> Option<&TextBox> {
        self.textboxes.iter().find(|textbox| textbox.id == id)
    }

    pub fn open_file_at(&mut self, window_id: u32, cursor_y: i32) -> bool {
        let Some(window) = self
            .taskbar
            .windows
            .iter()
            .find(|window| window.id == window_id)
        else {
            return false;
        };
        if window.minimized {
            return false;
        }
        if !matches!(window.kind, WindowKind::Explorer | WindowKind::Computer) {
            return false;
        }
        let row_top = window.y + 30 + 34;
        if cursor_y < row_top {
            return false;
        }
        let index = ((cursor_y - row_top) / 22) as usize;
        let current = self.files.current_directory();
        let Some(selected) = self.files.children(current).into_iter().nth(index) else {
            return false;
        };
        let name = selected.name.clone();
        let kind = selected.kind;
        let selected_id = selected.id;
        let (window_x, window_y, window_width, window_height) =
            (window.x, window.y, window.width, window.height);
        let _ = window;
        if kind == crate::file::NodeKind::Directory {
            let id = selected_id;
            self.files.navigate(id);
            return true;
        }
        let contents = self
            .files
            .node(selected_id)
            .map(|node| alloc::string::String::from_utf8_lossy(&node.contents).into_owned())
            .unwrap_or_default();
        let id = self.open_window_with_kind(
            &name,
            window_x + 32,
            window_y + 32,
            window_width.max(240),
            window_height.max(180),
            WindowKind::Notepad,
        );
        if let Some(textbox) = self.textboxes.iter_mut().find(|textbox| textbox.id == id) {
            textbox.text = contents;
        }
        true
    }

    pub fn begin_window_drag(&mut self, window_id: u32, cursor_x: i32, cursor_y: i32) -> bool {
        let Some(window) = self
            .taskbar
            .windows
            .iter()
            .find(|window| window.id == window_id)
        else {
            return false;
        };
        if window.minimized
            || !(window.x..window.x.saturating_add(window.width - 102)).contains(&cursor_x)
            || !(window.y..window.y + 30).contains(&cursor_y)
        {
            return false;
        }
        self.drag_window = Some(window_id);
        self.drag_offset = (cursor_x - window.x, cursor_y - window.y);
        true
    }

    pub fn drag_window_to(&mut self, cursor_x: i32, cursor_y: i32) -> bool {
        let Some(window_id) = self.drag_window else {
            return false;
        };
        let Some(window) = self
            .taskbar
            .windows
            .iter_mut()
            .find(|window| window.id == window_id)
        else {
            self.drag_window = None;
            return false;
        };
        window.x = cursor_x - self.drag_offset.0;
        window.y = cursor_y - self.drag_offset.1;
        true
    }

    pub fn release_window_drag(&mut self) {
        self.drag_window = None;
        self.drag_offset = (0, 0);
    }

    pub fn handle_window_control_click(
        &mut self,
        window_id: u32,
        cursor_x: i32,
        cursor_y: i32,
        screen_width: u32,
        work_area_height: u32,
    ) -> bool {
        let Some(window) = self
            .taskbar
            .windows
            .iter()
            .find(|window| window.id == window_id)
        else {
            return false;
        };
        let left = window.x + window.width.saturating_sub(102);
        if cursor_x < left || !(window.y + 7..window.y + 27).contains(&cursor_y) {
            return false;
        }
        let relative = cursor_x - left;
        if (0..29).contains(&relative) {
            if let Some(window) = self
                .taskbar
                .windows
                .iter_mut()
                .find(|window| window.id == window_id)
            {
                window.minimized = true;
                return true;
            }
            return true;
        }
        if (29..58).contains(&relative) {
            if let Some(window) = self
                .taskbar
                .windows
                .iter_mut()
                .find(|window| window.id == window_id)
            {
                window.maximized = !window.maximized;
                if window.maximized {
                    window.restore_geometry =
                        Some((window.x, window.y, window.width, window.height));
                    window.x = 0;
                    window.y = 0;
                    window.width = screen_width as i32;
                    window.height = work_area_height as i32;
                } else if let Some((x, y, width, height)) = window.restore_geometry.take() {
                    window.x = x;
                    window.y = y;
                    window.width = width;
                    window.height = height;
                }
                return true;
            }
            return true;
        }
        if (58..102).contains(&relative) {
            self.close_window(window_id);
            return true;
        }
        false
    }

    pub fn handle_window_key(&mut self, key: u8) -> bool {
        let Some(window_id) = self.selected_window else {
            return false;
        };
        let Some(textbox) = self
            .textboxes
            .iter_mut()
            .find(|textbox| textbox.id == window_id && textbox.focused)
        else {
            return false;
        };
        match key {
            8 => textbox.backspace(),
            0x0d => {
                textbox.insert('\r');
                textbox.insert('\n');
                true
            }
            0x25 => textbox.move_cursor_left(),
            0x27 => textbox.move_cursor_right(),
            0x20..=0x7e => {
                textbox.insert(key as char);
                true
            }
            _ => false,
        }
    }

    pub fn cycle_windows(&mut self) -> Option<u32> {
        if self.taskbar.windows.is_empty() {
            self.selected_window = None;
            return None;
        }
        let next = self
            .taskbar
            .windows
            .iter()
            .map(|window| window.id)
            .find(|id| Some(*id) > self.selected_window)
            .or_else(|| self.taskbar.windows.first().map(|window| window.id));
        self.selected_window = next;
        next
    }

    pub fn toggle_desktop(&mut self) {
        self.desktop_visible = !self.desktop_visible;
        if self.desktop_visible {
            self.taskbar.restore_all();
        } else {
            self.taskbar.show_desktop();
        }
    }

    pub fn snap_window(&mut self, id: u32, work_area: u32, side: SnapSide) {
        if let Some(window) = self
            .taskbar
            .windows
            .iter_mut()
            .find(|window| window.id == id)
        {
            let half = work_area / 2;
            match side {
                SnapSide::Left => window.x = 0,
                SnapSide::Right => window.x = half as i32,
            }
            window.y = 0;
            window.width = half as i32;
            window.height = work_area.saturating_sub(40) as i32;
        }
    }

    pub fn window_hit_test(&self, x: i32, y: i32) -> Option<u32> {
        self.taskbar
            .windows
            .iter()
            .filter(|window| {
                !window.minimized
                    && (window.x..window.x.saturating_add(window.width)).contains(&x)
                    && (window.y..window.y.saturating_add(window.height)).contains(&y)
            })
            .max_by_key(|window| window.id)
            .map(|window| window.id)
    }

    pub fn launch_selected_start_program(&mut self) -> Option<u32> {
        let program = self.start_menu.selected?;
        self.start_menu.close();
        let (title, kind) = match program {
            StartProgram::Notepad => (StartProgram::Notepad.title(), WindowKind::Notepad),
            StartProgram::CommandPrompt => (
                StartProgram::CommandPrompt.title(),
                WindowKind::CommandPrompt,
            ),
            StartProgram::Computer => (StartProgram::Computer.title(), WindowKind::Computer),
            StartProgram::Explorer => (StartProgram::Explorer.title(), WindowKind::Explorer),
        };
        Some(self.open_window_with_kind(title, 60, 60, 480, 320, kind))
    }
}

pub const CONTROL_START: u32 = 2001;
pub const CONTROL_TASKBAR_BASE: u32 = 2100;
pub const CONTROL_DESKTOP_BASE: u32 = 2200;
pub const CONTROL_MENU_BASE: u32 = 2300;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapSide {
    Left,
    Right,
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
    fn shell_model_can_switch_themes_and_rebuild_controls() {
        let mut shell = ShellModel::new(Theme::aero());
        shell.open_window("Test", 0, 0, 100, 80);
        shell.set_theme(crate::theme::ThemeKind::Classic, 800, 600);
        assert_eq!(shell.taskbar.theme.kind, crate::theme::ThemeKind::Classic);
        assert_eq!(shell.taskbar.theme.composition, false);
        assert!(shell
            .controls
            .control(crate::shell::CONTROL_START)
            .is_some());
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
        taskbar.add_window(1, "Notepad", 20, 20, 400, 300);
        taskbar.add_window(2, "CMD", 40, 40, 500, 300);
        taskbar.show_desktop();
        assert!(taskbar.windows.iter().all(|window| window.minimized));
        taskbar.restore_all();
        assert!(taskbar.windows.iter().all(|window| !window.minimized));
    }

    #[test]
    fn shell_model_supports_start_menu_window_and_focus_cycles() {
        let mut shell = ShellModel::new(Theme::aero());
        shell
            .desktop
            .add("Computer", ImageResource::Computer, 12, 12);
        shell.start_menu.toggle();
        shell.start_menu.selected = Some(StartProgram::Notepad);
        assert_eq!(shell.launch_selected_start_program(), Some(1));
        shell.open_window("Command Prompt", 20, 20, 320, 200);
        assert_eq!(shell.selected_window, Some(2));
        assert_eq!(shell.cycle_windows(), Some(1));
        assert_eq!(shell.window_hit_test(300, 300), Some(1));
        shell.close_window(1);
        assert_eq!(shell.selected_window, Some(2));
    }

    #[test]
    fn shell_model_maps_control_events_to_win7_actions() {
        let mut shell = ShellModel::new(Theme::aero());
        shell
            .desktop
            .add("Computer", ImageResource::Computer, 12, 12);

        assert!(shell.handle_control_event(ControlEvent::Activated(CONTROL_START)));
        assert!(shell.start_menu.open);
        shell.start_menu.selected = Some(StartProgram::Notepad);
        assert!(shell.handle_control_event(ControlEvent::Submit));
        assert!(!shell.start_menu.open);
        assert_eq!(shell.selected_window, Some(1));

        shell.open_window("Command Prompt", 20, 20, 320, 200);
        assert!(shell.handle_control_event(ControlEvent::Activated(CONTROL_TASKBAR_BASE)));
        assert_eq!(shell.selected_window, Some(1));
        assert!(!shell.taskbar.windows[0].minimized);

        shell.start_menu.toggle();
        assert!(shell.handle_control_event(ControlEvent::Activated(CONTROL_MENU_BASE + 1)));
        assert_eq!(shell.selected_window, Some(3));

        assert!(shell.handle_control_event(ControlEvent::Pressed(CONTROL_DESKTOP_BASE)));
        assert_eq!(shell.desktop.selected, vec![0]);
    }

    #[test]
    fn taskbar_groups_pins_moves_and_notifies() {
        let mut taskbar = TaskbarState::new(Theme::aero());
        taskbar.add_window(1, "Notepad", 0, 0, 100, 100);
        taskbar.add_window(2, "Notepad #2", 0, 0, 100, 100);
        taskbar.add_window(3, "CMD", 0, 0, 100, 100);
        taskbar.pin(4, "Command Prompt");
        taskbar.group_windows(1, &[2]);
        assert_eq!(taskbar.group_members(1), [1, 2]);
        assert!(taskbar
            .windows
            .iter()
            .any(|window| window.id == 4 && window.pinned));
        assert!(taskbar.move_button(3, -1));
        assert_eq!(taskbar.windows[1].id, 3);
        taskbar.add_notification(9, "Network");
        taskbar.update_notification(9, "Network connected");
        assert_eq!(taskbar.notifications[0].text, "Network connected");
        taskbar.remove_notification(9);
        assert!(taskbar.notifications.is_empty());
    }

    #[test]
    fn start_menu_searches_and_keyboard_selects_programs() {
        let mut start = StartMenu::new();
        start.set_query("expl");
        assert_eq!(start.search(), vec![StartProgram::Explorer]);
        assert_eq!(start.selected, Some(StartProgram::Explorer));
        start.set_query("");
        start.move_highlight(-1);
        assert_eq!(start.highlighted, 3);
        assert_eq!(start.confirm(), Some(StartProgram::Explorer));
    }

    #[test]
    fn start_menu_launches_typed_applications_and_file_rows_open_directories() {
        let mut shell = ShellModel::new(Theme::aero());
        shell.start_menu.selected = Some(StartProgram::Explorer);
        shell.launch_selected_start_program();
        assert_eq!(shell.taskbar.windows[0].kind, WindowKind::Explorer);

        let window_id = shell.taskbar.windows[0].id;
        let root = shell.files.root_id();
        let documents = shell.files.mkdir(root, "Documents");
        shell.files.touch(root, "readme.txt", b"NT61");
        let document_index = shell
            .files
            .children(root)
            .into_iter()
            .position(|node| node.id == documents)
            .unwrap();
        let first_y = shell.taskbar.windows[0].y + 30 + 34 + document_index as i32 * 22;
        assert!(shell.open_file_at(window_id, first_y));
        assert_eq!(shell.files.current_directory(), documents);

        shell.select_window(window_id);
        shell.files.navigate(root);
        let next_y = shell.taskbar.windows[0].y + 30 + 34 + 22;
        assert!(shell.open_file_at(window_id, next_y));
        let notepad = shell
            .taskbar
            .windows
            .iter()
            .find(|window| window.kind == WindowKind::Notepad)
            .unwrap();
        assert_eq!(notepad.title, "readme.txt");
    }

    #[test]
    fn windows_support_drag_minimize_maximize_and_close() {
        let mut shell = ShellModel::new(Theme::aero());
        let id = shell.open_window("Test", 100, 100, 300, 200);
        assert!(shell.begin_window_drag(id, 120, 110));
        assert!(shell.drag_window_to(180, 140));
        shell.release_window_drag();
        assert_eq!(
            shell
                .taskbar
                .windows
                .iter()
                .find(|window| window.id == id)
                .map(|window| (window.x, window.y)),
            Some((160, 130))
        );

        assert!(shell.handle_window_control_click(id, 368, 140, 800, 560));
        assert!(
            shell
                .taskbar
                .windows
                .iter()
                .find(|w| w.id == id)
                .unwrap()
                .minimized
        );
        shell.taskbar.windows[0].minimized = false;
        assert!(shell.handle_window_control_click(id, 398, 140, 800, 560));
        let maximized = shell.taskbar.windows.iter().find(|w| w.id == id).unwrap();
        assert!(maximized.maximized);
        assert_eq!(maximized.width, 800);
        assert_eq!(maximized.height, 560);
        assert!(shell.handle_window_control_click(id, 738, 7, 800, 560));
        let restored = shell.taskbar.windows.iter().find(|w| w.id == id).unwrap();
        assert_eq!((restored.x, restored.y), (160, 130));
        assert_eq!((restored.width, restored.height), (300, 200));
        assert!(shell.handle_window_control_click(id, 428, 140, 800, 560));
        assert!(shell.taskbar.windows.iter().all(|window| window.id != id));
    }

    #[test]
    fn desktop_supports_drag_auto_arrange_and_context_menu() {
        let mut desktop = DesktopState::new();
        desktop.add("B", ImageResource::Folder, 0, 0);
        desktop.add("A", ImageResource::Folder, 0, 0);
        assert!(desktop.begin_drag(0, 20, 30));
        assert!(desktop.auto_arrange(1024, 768) == ());
        assert_eq!(desktop.items[0].name, "A");
        desktop.open_context_menu();
        assert!(desktop.context_menu_open);
        desktop.close_context_menu();
        assert!(!desktop.context_menu_open);
    }
}
