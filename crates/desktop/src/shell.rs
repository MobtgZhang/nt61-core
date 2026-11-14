use crate::applications::{AppId,ApplicationSession,explorer::Explorer};
use crate::controls::{ControlEvent, ControlKind, ControlTree};
use crate::graphics::Rect;
use crate::resources::ImageResource;
use crate::theme::Theme;
use crate::widgets::textbox::TextBox;

pub mod layout;
pub mod chrome;
pub use chrome::WindowCursor;
use chrome::{ResizeDrag, CAPTION_HEIGHT};
use layout::ShellLayout;
pub use layout::{
    NOTIFICATION_AREA_WIDTH, PROGRAM_BUTTON_WIDTH, START_MENU_HEIGHT, START_MENU_WIDTH,
    TASKBAR_HEIGHT,
};
pub const START_BUTTON_WIDTH: u32 = 54;
pub const PROGRAM_BUTTON_GAP: u32 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flyout {
    Overflow,
    Volume,
    Network,
    ActionCenter,
    Clock,
    Power,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowKind {
    Generic,
    Application(AppId),
    ShellSettings(Flyout),
    Notepad,
    CommandPrompt,
    Computer,
    Explorer,
    ControlPanel,
    RecycleBin,
    System,
    ComputerManagement,
    RecycleBinProperties,
    ConfirmEmptyRecycleBin,
    DisplaySettings,
    Personalization,
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

impl DesktopItem {
    pub fn bounds(&self) -> Rect { Rect::new(self.x, self.y, 96, 88) }
}

pub fn taskbar_program_x(index: u32) -> u32 {
    START_BUTTON_WIDTH
        + 4
        + layout::PINNED_WIDTH
        + index * (PROGRAM_BUTTON_WIDTH + PROGRAM_BUTTON_GAP)
}

pub fn notification_area_left(width: u32) -> u32 {
    width.saturating_sub(NOTIFICATION_AREA_WIDTH)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortMode {
    Name,
    Type,
    Size,
    Date,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextMenuTarget {
    Desktop,
    DesktopItem(usize),
    TaskbarTask(usize),
    Taskbar,
    StartButton,
    Volume,
    Network,
    ActionCenter,
    Clock,
    ShowDesktop,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextMenuCommand {
    Open,
    CloseWindow,
    Refresh,
    SortByName,
    SortByType,
    ShowDesktop,
    OpenTaskbarProperties,
    OpenTaskManager,
    OpenPrograms,
    OpenRun,
    ShutDown,
    OpenVolumeMixer,
    OpenNetworkSharing,
    OpenActionCenter,
    OpenDateTime,
    Properties,
    Manage,
    EmptyRecycleBin,
    ScreenResolution,
    Personalize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuGroup {
    DesktopView,
    DesktopSortBy,
    DesktopNew,
    StartPrograms,
    StartDocuments,
    StartPictures,
    StartMusic,
    StartComputer,
    StartControlPanel,
    StartDevices,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SubmenuState {
    pub group: MenuGroup,
    pub x: i32,
    pub y: i32,
    pub highlighted: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContextMenuState {
    pub target: ContextMenuTarget,
    pub x: i32,
    pub y: i32,
    pub submenu: Option<SubmenuState>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DesktopState {
    pub items: Vec<DesktopItem>,
    pub selected: Vec<usize>,
    pub sort_mode: SortMode,
    pub context_menu: Option<ContextMenuState>,
}

impl DesktopState {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            selected: Vec::new(),
            sort_mode: SortMode::Name,
            context_menu: None,
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
        let slots: Vec<_> = self.items.iter().map(|item| (item.x, item.y)).collect();
        let mut indexed: Vec<_> = core::mem::take(&mut self.items).into_iter().enumerate().collect();
        indexed.sort_by(|(_, left), (_, right)| match self.sort_mode {
            SortMode::Name => left.name.cmp(&right.name),
            SortMode::Type => left.icon.cmp(&right.icon).then(left.name.cmp(&right.name)),
            SortMode::Size | SortMode::Date => left.name.cmp(&right.name),
        });
        let selected = core::mem::take(&mut self.selected);
        for (slot, (original, mut item)) in indexed.into_iter().enumerate() {
            (item.x, item.y) = slots[slot];
            if selected.contains(&original) {
                self.selected.push(slot);
            }
            self.items.push(item);
        }
    }

    pub fn hit_test(&self, x: i32, y: i32) -> Option<usize> {
        self.items.iter().position(|item| item.bounds().contains(x, y))
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
        let rows = (height.saturating_sub(TASKBAR_HEIGHT + 12) / 96).max(1) as usize;
        let columns = (width.saturating_sub(12) / 96).max(1) as usize;
        for (index, item) in self.items.iter_mut().enumerate() {
            item.x = 12 + (index / rows).min(columns - 1) as i32 * 96;
            item.y = 12 + (index % rows) as i32 * 96;
        }
    }

    pub fn close_context_menu(&mut self) {
        self.context_menu = None;
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
    pub date: Option<CalendarDate>,
}

impl TaskbarState {
    pub fn new(theme: Theme) -> Self {
        Self {
            windows: Vec::new(),
            theme,
            notifications: Vec::new(),
            clock: String::from("--:--"),
            date: None,
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
            if left + 80 <= (width - 220) as i32
                && (left..left + 80).contains(&x)
                && (y..y + taskbar_height.saturating_sub(20) as i32).contains(&pointer_y)
            {
                return Some(window.id);
            }
        }
        None
    }

    pub fn window_hit_test(&self, x: i32, y: i32) -> Option<u32> {
        for (index, window) in self.windows.iter().enumerate() {
            let left = taskbar_program_x(index as u32);
            if (left..left.saturating_add(PROGRAM_BUTTON_WIDTH)).contains(&(x as u32))
                && (4..TASKBAR_HEIGHT.saturating_sub(5)).contains(&(y as u32))
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
pub struct CalendarDate {
    pub year: u16,
    pub month: u8,
    pub day: u8,
}
impl CalendarDate {
    pub fn days_in_month(self) -> u8 {
        match self.month {
            2 if self.year % 4 == 0 && (self.year % 100 != 0 || self.year % 400 == 0) => 29,
            2 => 28,
            4 | 6 | 9 | 11 => 30,
            _ => 31,
        }
    }
    pub fn weekday(self) -> usize {
        // Gregorian calendar, Sunday = 0. Validated years are 1601..9999.
        let y = self.year as i32 - i32::from(self.month < 3);
        let table = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
        ((y + y / 4 - y / 100 + y / 400 + table[self.month as usize - 1] + self.day as i32) % 7)
            as usize
    }
    pub fn shifted_month(self, offset: i32) -> Self {
        let index = (self.year as i32 * 12 + self.month as i32 - 1 + offset)
            .clamp(1601 * 12, 9999 * 12 + 11);
        Self {
            year: (index / 12) as u16,
            month: (index % 12 + 1) as u8,
            day: 1,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgramCategory {Accessories,SystemTools,EaseOfAccess,Games,Maintenance,Startup}
impl ProgramCategory {
    pub fn title(self)->&'static str {match self {Self::Accessories=>"Accessories",Self::SystemTools=>"System Tools",Self::EaseOfAccess=>"Ease of Access",Self::Games=>"Games",Self::Maintenance=>"Maintenance",Self::Startup=>"Startup"}}
    pub fn parent(self)->Option<Self> {
        match self {
            Self::SystemTools|Self::EaseOfAccess=>Some(Self::Accessories),
            _=>None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartProgram {
    Folder(ProgramCategory),
    Application(AppId),
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
    pub submenu: Option<SubmenuState>,
    pub all_programs_view: bool,
    pub category: Option<ProgramCategory>,
}

impl StartMenu {
    pub fn new() -> Self {
        Self {
            open: false,
            selected: None,
            query: String::new(),
            highlighted: 0,
            submenu: None,
            all_programs_view: false,
            category: None,
        }
    }

    pub fn toggle(&mut self) {
        if self.open {
            self.close();
        } else {
            self.open = true;
        }
    }

    pub fn close(&mut self) {
        self.open = false;
        self.selected = None;
        self.query.clear();
        self.highlighted = 0;
        self.submenu = None;
        self.all_programs_view = false;
        self.category = None;
    }

    pub fn all_programs(&self) -> Vec<StartProgram> {
        let mut programs = alloc::vec![
            StartProgram::Notepad,
            StartProgram::CommandPrompt,
            StartProgram::Computer,
            StartProgram::Explorer,
        ];
        programs.extend(crate::applications::APPLICATIONS.iter().filter(|a|!matches!(a.id,AppId::Notepad|AppId::Explorer|AppId::CommandPrompt)).map(|a|StartProgram::Application(a.id)));
        programs
    }

    pub fn search(&self) -> Vec<StartProgram> {
        let query = self.query.to_ascii_lowercase();
        if query.is_empty() {
            return Vec::new();
        }
        self.all_programs()
            .into_iter()
            .filter(|program| program.title().to_ascii_lowercase().contains(&query) || match program {StartProgram::Application(id)=>(id.application().install_path().is_some() && id.application().executable.to_ascii_lowercase().contains(&query)) || id.application().chinese.contains(&query),StartProgram::Notepad=>"notepad.exe 记事本".contains(&query),StartProgram::Explorer=>"explorer.exe 资源管理器".contains(&query),StartProgram::CommandPrompt=>"cmd.exe 命令提示符".contains(&query),_=>false})
            .collect()
    }

    pub fn visible_programs(&self) -> Vec<StartProgram> {
        if !self.query.is_empty() {return self.search();}
        if !self.all_programs_view {return self.all_programs().into_iter().take(4).collect();}
        if let Some(category)=self.category {
            let mut programs:Vec<_>=self.all_programs().into_iter().filter(|program|match program {
                StartProgram::Application(id)=>id.application().category==category.title(),
                StartProgram::Computer=>category==ProgramCategory::SystemTools,
                StartProgram::Notepad|StartProgram::Explorer|StartProgram::CommandPrompt=>category==ProgramCategory::Accessories,
                _=>false,
            }).collect();
            if category==ProgramCategory::Accessories {
                programs.extend([StartProgram::Folder(ProgramCategory::EaseOfAccess),StartProgram::Folder(ProgramCategory::SystemTools)]);
            }
            programs.sort_by_key(|p|p.title());programs
        }else{alloc::vec![StartProgram::Application(AppId::DefaultPrograms),StartProgram::Folder(ProgramCategory::Accessories),StartProgram::Folder(ProgramCategory::Games),StartProgram::Folder(ProgramCategory::Maintenance),StartProgram::Folder(ProgramCategory::Startup)]}
    }

    pub fn program_offset(&self) -> usize {
        if self.query.is_empty() && !self.all_programs_view {0} else {self.highlighted.saturating_sub(6)}
    }

    pub fn set_query(&mut self, query: &str) {
        self.query = String::from(query);
        self.highlighted = 0;
        self.selected = self.search().first().copied();
    }

    pub fn move_highlight(&mut self, offset: i32) {
        let count = self.visible_programs().len();
        if count == 0 {
            return;
        }
        let current = self.highlighted as i32 + offset;
        self.highlighted = current.rem_euclid(count as i32) as usize;
    }

    pub fn confirm(&mut self) -> Option<StartProgram> {
        let results = self.visible_programs();
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
            StartProgram::Folder(category)=>category.title(),
            StartProgram::Application(id) => id.application().title,
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

#[cfg(test)]
mod program_catalog_tests {
    use super::*;

    #[test]
    fn accessories_contains_nested_system_and_accessibility_folders() {
        let mut menu = StartMenu::new();
        menu.all_programs_view = true;
        let root = menu.visible_programs();
        assert!(root.contains(&StartProgram::Folder(ProgramCategory::Accessories)));
        assert!(root.contains(&StartProgram::Folder(ProgramCategory::Startup)));
        assert!(!root.contains(&StartProgram::Folder(ProgramCategory::SystemTools)));
        assert!(!root.contains(&StartProgram::Folder(ProgramCategory::EaseOfAccess)));
        menu.category = Some(ProgramCategory::Accessories);
        let accessories = menu.visible_programs();
        assert!(accessories.contains(&StartProgram::Folder(ProgramCategory::SystemTools)));
        assert!(accessories.contains(&StartProgram::Folder(ProgramCategory::EaseOfAccess)));
        assert!(accessories.contains(&StartProgram::CommandPrompt));
        menu.category = Some(ProgramCategory::SystemTools);
        assert!(menu.visible_programs().contains(&StartProgram::Application(AppId::ResourceMonitor)));
        menu.category = Some(ProgramCategory::EaseOfAccess);
        assert!(menu.visible_programs().contains(&StartProgram::Application(AppId::EaseOfAccessCenter)));
        menu.category = Some(ProgramCategory::Startup);
        assert!(menu.visible_programs().is_empty());
    }

    #[test]
    fn back_from_nested_folder_returns_to_accessories_before_program_root() {
        let mut shell = ShellModel::new(Theme::aero());
        shell.start_menu.open = true;
        shell.start_menu.all_programs_view = true;
        shell.start_menu.category = Some(ProgramCategory::SystemTools);
        shell.activate_control(CONTROL_ALL_PROGRAMS);
        assert_eq!(shell.start_menu.category, Some(ProgramCategory::Accessories));
        shell.activate_control(CONTROL_ALL_PROGRAMS);
        assert_eq!(shell.start_menu.category, None);
        assert!(shell.start_menu.all_programs_view);
        shell.activate_control(CONTROL_ALL_PROGRAMS);
        assert!(!shell.start_menu.all_programs_view);
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ShellModel {
    pub applications: Vec<ApplicationSession>,
    pub process_snapshot: Option<crate::applications::processes::Snapshot>,
    pub explorers: Vec<Explorer>,
    pub text_target: Option<u32>,
    pub desktop: DesktopState,
    pub taskbar: TaskbarState,
    pub start_menu: StartMenu,
    pub files: crate::file::FileSystem,
    pub next_window_id: u32,
    pub selected_window: Option<u32>,
    pub desktop_visible: bool,
    desktop_restore: Vec<u32>,
    pub controls: ControlTree,
    pub(crate) viewport: (u32, u32),
    pub textboxes: Vec<TextBox>,
    pub drag_window: Option<u32>,
    pub drag_offset: (i32, i32),
    resize_drag: Option<ResizeDrag>,
    pub context_menu: Option<ContextMenuState>,
    pub context_submenu: Option<SubmenuState>,
    pub flyout: Option<Flyout>,
    pub volume: u8,
    pub network_adapters: u32,
    pub audio_available: bool,
    pub muted: bool,
    pub calendar_month: Option<CalendarDate>,
    pub power_action: Option<PowerAction>,
}

impl ShellModel {
    pub fn new(theme: Theme) -> Self {
        Self {
            applications: Vec::new(),
            process_snapshot: None,
            explorers: Vec::new(),
            text_target: None,
            desktop: DesktopState::new(),
            taskbar: TaskbarState::new(theme),
            start_menu: StartMenu::new(),
            files: crate::file::FileSystem::new(),
            next_window_id: 1,
            selected_window: None,
            desktop_visible: true,
            desktop_restore: Vec::new(),
            controls: ControlTree::new(),
            viewport: (1024, 768),
            textboxes: Vec::new(),
            drag_window: None,
            drag_offset: (0, 0),
            resize_drag: None,
            context_menu: None,
            context_submenu: None,
            flyout: None,
            volume: 75,
            muted: false,
            network_adapters: 0,
            audio_available: false,
            calendar_month: None,
            power_action: None,
        }
    }

    pub fn set_theme(&mut self, kind: crate::theme::ThemeKind, width: u32, height: u32) {
        self.taskbar.theme = Theme::for_theme(kind);
        self.refresh_controls(width, height);
    }

    pub fn refresh_controls(&mut self, width: u32, height: u32) {
        self.viewport = (width, height);
        let new_start = self.start_menu.open && self.controls.control(CONTROL_SEARCH).is_none();
        let next = self.build_controls(width, height);
        self.controls.replace_layout(next);
        if new_start {
            self.controls.set_focus(CONTROL_SEARCH);
        }
        if self.context_menu.is_some()
            && !(CONTROL_CONTEXT_MENU_BASE..CONTROL_CONTEXT_MENU_BASE + 100)
                .contains(&self.controls.focus())
            && !(CONTROL_SUBMENU_BASE..CONTROL_SUBMENU_BASE + 100).contains(&self.controls.focus())
        {
            self.controls.set_focus(CONTROL_CONTEXT_MENU_BASE);
        }
        if self.flyout.is_some() {
            let id = self.controls.focus();
            if !((CONTROL_FLYOUT_LINK..=CONTROL_CALENDAR_NEXT).contains(&id)
                || (CONTROL_POWER_BASE..CONTROL_POWER_BASE + 4).contains(&id))
            {
                self.controls
                    .set_focus(if self.flyout == Some(Flyout::Power) {
                        CONTROL_POWER_BASE
                    } else {
                        CONTROL_FLYOUT_LINK
                    });
            }
        }
    }

    fn build_controls(&self, width: u32, height: u32) -> ControlTree {
        let mut c = ControlTree::new();
        let l = ShellLayout::new(width, height);
        let top = l.taskbar_top();
        for (index, item) in self.desktop.items.iter().enumerate() {
            c.add(
                CONTROL_DESKTOP_BASE + index as u32,
                ControlKind::Image,
                item.bounds(),
            );
        }
        c.add(
            CONTROL_START,
            ControlKind::Button,
            Rect::new(0, top, START_BUTTON_WIDTH, TASKBAR_HEIGHT),
        );
        c.add(CONTROL_PIN_EXPLORER, ControlKind::Button, l.pinned());
        for (index, _) in self.taskbar.windows.iter().enumerate() {
            c.add(
                CONTROL_TASKBAR_BASE + index as u32,
                ControlKind::Button,
                l.window_button(index, self.taskbar.windows.len()),
            );
        }
        for (index, id) in [
            CONTROL_OVERFLOW,
            CONTROL_TRAY_ACTION_CENTER,
            CONTROL_TRAY_NETWORK,
            CONTROL_TRAY_VOLUME,
        ]
        .iter()
        .enumerate()
        {
            c.add(*id, ControlKind::Button, l.tray(index as u32));
        }
        c.add(CONTROL_CLOCK, ControlKind::Button, l.clock());
        c.add(CONTROL_SHOW_DESKTOP, ControlKind::Button, l.peek());
        if self.start_menu.open {
            c.add(CONTROL_POPUP_BACKGROUND, ControlKind::Panel, l.start());
            for index in self.start_menu.program_offset()..self.start_menu.visible_programs().len().min(self.start_menu.program_offset()+7) {
                c.add(
                    CONTROL_MENU_BASE + index as u32,
                    ControlKind::Button,
                    l.program(index-self.start_menu.program_offset()),
                );
            }
            for index in 0..7 {
                c.add(
                    CONTROL_START_LINK_BASE + index as u32,
                    ControlKind::Button,
                    l.link(index),
                );
            }
            if self.start_menu.visible_programs().len()>7 {
                c.add(CONTROL_PROGRAM_PREV,ControlKind::Button,l.program_page(false));
                c.add(CONTROL_PROGRAM_NEXT,ControlKind::Button,l.program_page(true));
            }
            c.add(CONTROL_ALL_PROGRAMS, ControlKind::Button, l.all_programs());
            c.add(CONTROL_SEARCH, ControlKind::Edit, l.search());
            c.add(CONTROL_SHUTDOWN, ControlKind::Button, l.shutdown());
            c.add(CONTROL_POWER_ARROW, ControlKind::Button, l.power_arrow());
        }
        if let Some(bounds) = self.context_menu_bounds(width, height) {
            c.add(CONTROL_POPUP_BACKGROUND, ControlKind::Panel, bounds);
            for index in 0..self.context_menu_items().len() {
                if self.context_item_enabled(index) {
                    if let Some(row) = self.context_item_bounds(index, width, height) {
                        c.add(CONTROL_CONTEXT_MENU_BASE + index as u32, ControlKind::Button, row);
                    }
                }
            }
        }
        if let Some(bounds) = self.context_submenu_bounds(width, height) {
            let state = self.context_submenu.or(self.start_menu.submenu).unwrap();
            c.add(CONTROL_POPUP_BACKGROUND, ControlKind::Panel, bounds);
            for index in 0..Self::submenu_items(state.group).len() {
                if !Self::submenu_item_enabled(state.group, index) { continue; }
                c.add(
                    CONTROL_SUBMENU_BASE + index as u32,
                    ControlKind::Button,
                    Rect::new(
                        bounds.x + 4,
                        bounds.y + 4 + index as i32 * 28,
                        bounds.width.saturating_sub(8),
                        28,
                    ),
                );
            }
        }
        if let Some(kind) = self.flyout {
            let b = l.flyout(kind);
            c.add(CONTROL_POPUP_BACKGROUND, ControlKind::Panel, b);
            if kind != Flyout::Power {
                c.add(
                    CONTROL_FLYOUT_LINK,
                    ControlKind::Button,
                    l.flyout_footer(kind),
                );
            }
            match kind {
                Flyout::Volume => {
                    c.add(
                        CONTROL_VOLUME_TRACK,
                        ControlKind::TrackBar,
                        l.volume_track(),
                    );
                    c.add(
                        CONTROL_MUTE,
                        ControlKind::Button,
                        Rect::new(b.x + 54, b.y + b.height as i32 - 70, 52, 30),
                    );
                }
                Flyout::Clock if self.calendar_month.is_some() => {
                    c.add(
                        CONTROL_CALENDAR_PREV,
                        ControlKind::Button,
                        Rect::new(b.x + 12, b.y + 42, 26, 26),
                    );
                    c.add(
                        CONTROL_CALENDAR_NEXT,
                        ControlKind::Button,
                        Rect::new(b.x + b.width as i32 - 38, b.y + 42, 26, 26),
                    );
                }
                Flyout::Power => {
                    for index in 0..4 {
                        c.add(
                            CONTROL_POWER_BASE + index,
                            ControlKind::Button,
                            Rect::new(b.x + 4, b.y + 4 + index as i32 * 34, b.width - 8, 34),
                        );
                    }
                }
                _ => {}
            }
        }
        c
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
                self.close_context_menu();
                self.flyout = None;
                true
            }
            ControlEvent::HoverChanged(_) => true,
            _ => false,
        }
    }

    fn select_control(&mut self, id: u32) -> bool {
        if (CONTROL_DESKTOP_BASE..CONTROL_MENU_BASE).contains(&id) {
            let index = (id - CONTROL_DESKTOP_BASE) as usize;
            if index < self.desktop.items.len() {
                self.desktop.select(index);
                return true;
            }
        }
        false
    }

    fn activate_control(&mut self, id: u32) -> bool {
        match id {
            CONTROL_START => {
                self.flyout = None;
                self.close_context_menu();
                self.start_menu.toggle();
                true
            }
            CONTROL_PIN_EXPLORER => {
                self.start_menu.selected = Some(StartProgram::Explorer);
                self.launch_selected_start_program();
                true
            }
            CONTROL_OVERFLOW => self.toggle_flyout(Flyout::Overflow),
            CONTROL_TRAY_VOLUME => self.toggle_flyout(Flyout::Volume),
            CONTROL_TRAY_NETWORK => self.toggle_flyout(Flyout::Network),
            CONTROL_TRAY_ACTION_CENTER => self.toggle_flyout(Flyout::ActionCenter),
            CONTROL_CLOCK => self.toggle_flyout(Flyout::Clock),
            CONTROL_POWER_ARROW => self.toggle_flyout(Flyout::Power),
            CONTROL_PROGRAM_PREV|CONTROL_PROGRAM_NEXT => {
                let count=self.start_menu.visible_programs().len();
                let offset=self.start_menu.program_offset();
                self.start_menu.highlighted=if id==CONTROL_PROGRAM_NEXT {(offset+13).min(count.saturating_sub(1))}else{offset.saturating_sub(7)};
                true
            }
            CONTROL_ALL_PROGRAMS => {
                if let Some(category)=self.start_menu.category {
                    self.start_menu.category=category.parent();
                }else{self.start_menu.all_programs_view = !self.start_menu.all_programs_view;}
                self.start_menu.set_query("");
                true
            }
            CONTROL_SHUTDOWN => {
                self.power_action = Some(PowerAction::ShutDown);
                true
            }
            CONTROL_MUTE => {
                self.muted = !self.muted;
                true
            }
            CONTROL_CALENDAR_PREV | CONTROL_CALENDAR_NEXT => {
                self.calendar_month = self.calendar_month.map(|date| {
                    date.shifted_month(if id == CONTROL_CALENDAR_PREV { -1 } else { 1 })
                });
                true
            }
            CONTROL_FLYOUT_LINK => {
                let Some(kind) = self.flyout.take() else {
                    return false;
                };
                let title = match kind {
                    Flyout::Volume => "Volume Mixer",
                    Flyout::Network => "Network and Sharing Center",
                    Flyout::ActionCenter => "Action Center",
                    Flyout::Clock => "Date and Time",
                    Flyout::Overflow => "Notification Area Icons",
                    _ => return false,
                };
                self.open_window_with_kind(
                    title,
                    220,
                    160,
                    480,
                    320,
                    WindowKind::ShellSettings(kind),
                );
                true
            }
            CONTROL_SHOW_DESKTOP => {
                self.close_context_menu();
                self.flyout = None;
                self.toggle_desktop();
                true
            }
            id if (CONTROL_POWER_BASE..CONTROL_POWER_BASE + 4).contains(&id) => {
                self.power_action = Some(match id - CONTROL_POWER_BASE {
                    0 => PowerAction::Lock,
                    1 => PowerAction::LogOff,
                    2 => PowerAction::Restart,
                    _ => PowerAction::ShutDown,
                });
                true
            }
            id if (CONTROL_START_LINK_BASE..CONTROL_START_LINK_BASE + 7).contains(&id) => {
                self.activate_start_link((id - CONTROL_START_LINK_BASE) as usize)
            }
            id if (CONTROL_SUBMENU_BASE..CONTROL_SUBMENU_BASE + 100).contains(&id) => {
                self.activate_context_submenu((id - CONTROL_SUBMENU_BASE) as usize)
            }
            id if (CONTROL_CONTEXT_MENU_BASE..CONTROL_CONTEXT_MENU_BASE + 100).contains(&id) => {
                self.activate_context_menu_command((id - CONTROL_CONTEXT_MENU_BASE) as usize)
            }
            id if (CONTROL_MENU_BASE..CONTROL_MENU_BASE + 100).contains(&id)
                && self.start_menu.open =>
            {
                let program = self
                    .start_menu
                    .visible_programs()
                    .get((id - CONTROL_MENU_BASE) as usize)
                    .copied();
                if let Some(program) = program {
                    self.start_menu.selected = Some(program);
                    self.launch_selected_start_program();
                    true
                } else {
                    false
                }
            }
            id if (CONTROL_TASKBAR_BASE..CONTROL_TASKBAR_BASE + 100).contains(&id) => {
                if let Some(window) = self
                    .taskbar
                    .windows
                    .get_mut((id - CONTROL_TASKBAR_BASE) as usize)
                {
                    window.minimized = Some(window.id) == self.selected_window && !window.minimized;
                    self.selected_window = if window.minimized {
                        None
                    } else {
                        Some(window.id)
                    };
                    true
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    /// Kernel wall clock in NT 100ns units. Zero means no platform RTC.
    pub fn update_clock(&mut self, ticks: u64) -> bool {
        let date = if ticks == 0 {
            None
        } else {
            let mut days = ticks / 864_000_000_000;
            let mut year = 1601u16;
            while year < 9999 {
                let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
                let count = if leap { 366 } else { 365 };
                if days < count {
                    break;
                }
                days -= count;
                year += 1;
            }
            let mut d = CalendarDate {
                year,
                month: 1,
                day: 1,
            };
            while d.month < 12 && days >= d.days_in_month() as u64 {
                days -= d.days_in_month() as u64;
                d.month += 1;
            }
            if days >= d.days_in_month() as u64 {
                None
            } else {
                d.day = days as u8 + 1;
                Some(d)
            }
        };
        let seconds = ticks / 10_000_000;
        let time = if date.is_some() {
            alloc::format!("{:02}:{:02}", seconds / 3600 % 24, seconds / 60 % 60)
        } else {
            String::from("--:--")
        };
        let changed = self.taskbar.clock != time || self.taskbar.date != date;
        self.taskbar.clock = time;
        self.taskbar.date = date;
        if self.flyout == Some(Flyout::Clock) && self.calendar_month.is_none() {
            self.calendar_month = date.map(|d| CalendarDate { day: 1, ..d });
        }
        changed
    }

    pub fn show_power_unavailable(&mut self) {
        self.power_action = None;
        self.flyout = None;
        self.start_menu.close();
        let id = self.open_window("Power options", 180, 150, 490, 180);
        if let Some(text) = self.textboxes.iter_mut().find(|t| t.id == id) {
            text.text = String::from("The session power service is not available yet.");
        }
    }

    pub fn toggle_flyout(&mut self, kind: Flyout) -> bool {
        if kind != Flyout::Power {
            self.start_menu.close();
        }
        self.close_context_menu();
        self.flyout = if self.flyout == Some(kind) {
            None
        } else {
            Some(kind)
        };
        if kind == Flyout::Clock {
            self.calendar_month = self.taskbar.date.map(|d| CalendarDate { day: 1, ..d });
        }
        true
    }

    pub fn popup_contains(&self, x: i32, y: i32, width: u32, height: u32) -> bool {
        (self.start_menu.open && ShellLayout::new(width, height).start().contains(x, y))
            || self
                .context_menu_bounds(width, height)
                .is_some_and(|b| b.contains(x, y))
            || self
                .context_submenu_bounds(width, height)
                .is_some_and(|b| b.contains(x, y))
            || self
                .flyout
                .is_some_and(|f| ShellLayout::new(width, height).flyout(f).contains(x, y))
    }

    pub fn update_menu_hover(&mut self, id: u32, width: u32, height: u32) -> bool {
        if (CONTROL_START_LINK_BASE..CONTROL_START_LINK_BASE + 7).contains(&id) {
            let index = (id - CONTROL_START_LINK_BASE) as usize;
            if index < 4 {
                let l = ShellLayout::new(width, height);
                let b = l.start();
                let row = l.link(index);
                let group = Self::start_submenu_group(index).unwrap();
                if self.start_menu.submenu.is_some_and(|s| s.group == group) {
                    return false;
                }
                return self.open_start_submenu(index, b.x + b.width as i32 - 4, row.y);
            }
            return self.start_menu.submenu.take().is_some();
        }
        if (CONTROL_CONTEXT_MENU_BASE..CONTROL_CONTEXT_MENU_BASE + 100).contains(&id) {
            let index = (id - CONTROL_CONTEXT_MENU_BASE) as usize;
            if let (Some(group), Some(b)) = (
                self.context_submenu_group(index),
                self.context_menu_bounds(width, height),
            ) {
                if self.context_submenu.is_some_and(|s| s.group == group) {
                    return false;
                }
                return self.open_context_submenu(
                    index,
                    b.x + b.width as i32 - 6,
                    self.context_item_bounds(index, width, height).map_or(b.y, |row| row.y),
                );
            }
            return self.context_submenu.take().is_some();
        }
        if (CONTROL_MENU_BASE..CONTROL_MENU_BASE + 100).contains(&id)
            || (CONTROL_SEARCH..=CONTROL_POWER_ARROW).contains(&id)
        {
            return self.start_menu.submenu.take().is_some();
        }
        false
    }

    pub fn set_volume_at(&mut self, y: i32, width: u32, height: u32) -> bool {
        let t = ShellLayout::new(width, height).volume_track();
        let volume =
            (100 - (y - t.y).clamp(0, t.height as i32) * 100 / t.height.max(1) as i32) as u8;
        let changed = self.volume != volume;
        self.volume = volume;
        changed
    }

    pub fn open_context_menu(&mut self, x: i32, y: i32, width: u32, height: u32) -> bool {
        self.start_menu.close();
        self.flyout = None;
        let top = height.saturating_sub(TASKBAR_HEIGHT) as i32;
        let target = if y >= top {
            if self.controls.hit_test(x, y) == CONTROL_START {
                ContextMenuTarget::StartButton
            } else if let Some(index) = (0..self.taskbar.windows.len()).find(|index| {
                ShellLayout::new(width, height)
                    .window_button(*index, self.taskbar.windows.len())
                    .contains(x, y)
            }) {
                ContextMenuTarget::TaskbarTask(index)
            } else if x >= notification_area_left(width) as i32 {
                match self.controls.hit_test(x, y) {
                    CONTROL_TRAY_VOLUME => ContextMenuTarget::Volume,
                    CONTROL_TRAY_NETWORK => ContextMenuTarget::Network,
                    CONTROL_TRAY_ACTION_CENTER => ContextMenuTarget::ActionCenter,
                    CONTROL_CLOCK => ContextMenuTarget::Clock,
                    CONTROL_SHOW_DESKTOP => ContextMenuTarget::ShowDesktop,
                    _ => ContextMenuTarget::Volume,
                }
            } else {
                ContextMenuTarget::Taskbar
            }
        } else if let Some(index) = self.desktop.hit_test(x, y) {
            self.desktop.select(index);
            ContextMenuTarget::DesktopItem(index)
        } else {
            ContextMenuTarget::Desktop
        };
        self.context_menu = Some(ContextMenuState {
            target,
            x,
            y,
            submenu: None,
        });
        self.context_submenu = None;
        true
    }

    pub fn close_context_menu(&mut self) {
        self.context_menu = None;
        self.context_submenu = None;
    }

    pub fn start_menu_bounds(&self, framebuffer_width: u32, framebuffer_height: u32) -> Rect {
        ShellLayout::new(framebuffer_width, framebuffer_height).start()
    }

    pub fn context_menu_bounds(
        &self,
        framebuffer_width: u32,
        framebuffer_height: u32,
    ) -> Option<Rect> {
        let menu = self.context_menu.as_ref()?;
        let items = self.context_menu_items();
        let width = items
            .iter()
            .map(|item| item.0.len() * 8 + 60)
            .max()
            .unwrap_or(100)
            .min(framebuffer_width as usize) as u32;
        let height =
            (items.iter().map(|item| if item.0.is_empty() { 7 } else { 24 }).sum::<u32>() + 6)
                .min(framebuffer_height.saturating_sub(TASKBAR_HEIGHT));
        Some(Rect::new(
            (menu.x.max(0) as u32).min(framebuffer_width.saturating_sub(width)) as i32,
            (menu.y.max(0) as u32).min(framebuffer_height.saturating_sub(TASKBAR_HEIGHT + height))
                as i32,
            width,
            height,
        ))
    }

    pub fn context_item_enabled(&self, index: usize) -> bool {
        self.context_menu_items().get(index).is_some_and(|item| {
            !item.0.is_empty() && (item.1.is_some() || self.context_submenu_group(index).is_some())
                && (item.1 != Some(ContextMenuCommand::EmptyRecycleBin) || !self.files.recycle_bin().is_empty())
        })
    }

    pub fn context_item_bounds(&self, index: usize, width: u32, height: u32) -> Option<Rect> {
        let b = self.context_menu_bounds(width, height)?;
        let items = self.context_menu_items();
        let item = items.get(index)?;
        let offset: u32 = items[..index].iter().map(|item| if item.0.is_empty() { 7 } else { 24 }).sum();
        Some(Rect::new(b.x + 3, b.y + 3 + offset as i32, b.width.saturating_sub(6), if item.0.is_empty() { 7 } else { 24 }))
    }

    pub fn context_submenu_bounds(
        &self,
        framebuffer_width: u32,
        framebuffer_height: u32,
    ) -> Option<Rect> {
        let submenu = self
            .context_submenu
            .as_ref()
            .or(self.start_menu.submenu.as_ref())?;
        let items = Self::submenu_items(submenu.group);
        let width = items
            .iter()
            .map(|item| item.len() * 8 + 30)
            .max()
            .unwrap_or(100)
            .min(framebuffer_width as usize) as u32;
        let height =
            (items.len() as u32 * 28 + 8)
                .min(framebuffer_height.saturating_sub(TASKBAR_HEIGHT));
        let mut x = submenu.x.max(0) as u32;
        if x.saturating_add(width) > framebuffer_width {
            if let Some(parent) = self.context_menu_bounds(framebuffer_width, framebuffer_height) {
                x = parent.x.saturating_sub(width as i32 - 6).max(0) as u32;
            }
        }
        Some(Rect::new(
            x.min(framebuffer_width.saturating_sub(width)) as i32,
            (submenu.y.max(0) as u32)
                .min(framebuffer_height.saturating_sub(TASKBAR_HEIGHT + height)) as i32,
            width,
            height,
        ))
    }

    pub fn close_popups_if_outside(&mut self, x: i32, y: i32, width: u32, height: u32) -> bool {
        let l = ShellLayout::new(width, height);
        let submenu = self.context_submenu_bounds(width, height);
        let mut changed = false;
        let power_inside =
            self.flyout == Some(Flyout::Power) && l.flyout(Flyout::Power).contains(x, y);
        if self.start_menu.open
            && !l.start().contains(x, y)
            && !Rect::new(0, l.taskbar_top(), START_BUTTON_WIDTH, TASKBAR_HEIGHT).contains(x, y)
            && !submenu.is_some_and(|b| b.contains(x, y))
            && !power_inside
        {
            self.start_menu.close();
            changed = true;
        }
        if let Some(kind) = self.flyout {
            let anchor = match kind {
                Flyout::Overflow => l.tray(0),
                Flyout::ActionCenter => l.tray(1),
                Flyout::Network => l.tray(2),
                Flyout::Volume => l.tray(3),
                Flyout::Clock => l.clock(),
                Flyout::Power => l.power_arrow(),
            };
            if !l.flyout(kind).contains(x, y) && !anchor.contains(x, y) {
                self.flyout = None;
                changed = true;
            }
        }
        if self.context_menu.is_some()
            && !self
                .context_menu_bounds(width, height)
                .is_some_and(|b| b.contains(x, y))
            && !submenu.is_some_and(|b| b.contains(x, y))
        {
            self.close_context_menu();
            changed = true;
        }
        changed
    }

    pub fn context_submenu_group(&self, index: usize) -> Option<MenuGroup> {
        let title = self.context_menu_items().get(index)?.0;
        let target = self.context_menu.as_ref()?.target;
        match (target, title) {
            (ContextMenuTarget::Desktop, "View") => Some(MenuGroup::DesktopView),
            (ContextMenuTarget::Desktop, "Sort by") => Some(MenuGroup::DesktopSortBy),
            (ContextMenuTarget::Desktop, "New") => Some(MenuGroup::DesktopNew),
            _ => None,
        }
    }

    pub fn open_context_submenu(&mut self, index: usize, x: i32, y: i32) -> bool {
        let Some(group) = self.context_submenu_group(index) else {
            return false;
        };
        let state = SubmenuState {
            group,
            x,
            y,
            highlighted: 0,
        };
        self.context_submenu = Some(state);
        if let Some(menu) = self.context_menu.as_mut() {
            menu.submenu = Some(state);
        }
        true
    }

    pub fn submenu_items(group: MenuGroup) -> &'static [&'static str] {
        const VIEW: &[&str] = &["Large icons", "Medium icons", "Small icons", "Auto arrange"];
        const SORT: &[&str] = &["Name", "Size", "Item type", "Date modified"];
        const NEW: &[&str] = &["Folder", "Shortcut", "Text Document", "Bitmap image"];
        const PROGRAMS: &[&str] = &["Windows Explorer", "Command Prompt", "Notepad"];
        const DOCUMENTS: &[&str] = &["readme.txt", "Recent documents"];
        const PICTURES: &[&str] = &["Pictures library", "Sample Pictures"];
        const MUSIC: &[&str] = &["Music library", "Sample Music"];
        const COMPUTER: &[&str] = &["Computer", "System properties"];
        const CONTROL: &[&str] = &["Control Panel", "Programs and Features"];
        const DEVICES: &[&str] = &["Devices and Printers", "Device Manager"];
        match group {
            MenuGroup::DesktopView => VIEW,
            MenuGroup::DesktopSortBy => SORT,
            MenuGroup::DesktopNew => NEW,
            MenuGroup::StartPrograms => PROGRAMS,
            MenuGroup::StartDocuments => DOCUMENTS,
            MenuGroup::StartPictures => PICTURES,
            MenuGroup::StartMusic => MUSIC,
            MenuGroup::StartComputer => COMPUTER,
            MenuGroup::StartControlPanel => CONTROL,
            MenuGroup::StartDevices => DEVICES,
        }
    }

    pub fn activate_context_submenu(&mut self, index: usize) -> bool {
        if self.context_submenu.is_none() && self.start_menu.submenu.is_some() {
            return self.activate_start_submenu(index);
        }
        let Some(submenu) = self.context_submenu else {
            return false;
        };
        let group = submenu.group;
        if !Self::submenu_item_enabled(group, index) { return false; }
        self.context_menu = None;
        self.context_submenu = None;
        match group {
            MenuGroup::DesktopSortBy if index == 0 => {
                self.desktop.sort_mode = SortMode::Name;
                self.desktop.sort();
                true
            }
            MenuGroup::DesktopSortBy if index == 2 => {
                self.desktop.sort_mode = SortMode::Type;
                self.desktop.sort();
                true
            }
            MenuGroup::DesktopView if index == 1 => true, // Current medium icon layout.
            MenuGroup::DesktopView if index == 3 => {
                self.desktop.auto_arrange(self.viewport.0, self.viewport.1);
                true
            }
            _ => false,
        }
    }

    pub fn submenu_item_enabled(group: MenuGroup, index: usize) -> bool {
        if index >= Self::submenu_items(group).len() { return false; }
        match group {
            MenuGroup::DesktopView => matches!(index, 1 | 3),
            MenuGroup::DesktopSortBy => matches!(index, 0 | 2),
            MenuGroup::DesktopNew => false,
            _ => true,
        }
    }

    pub fn open_start_submenu(&mut self, index: usize, x: i32, y: i32) -> bool {
        let Some(group) = Self::start_submenu_group(index) else {
            return false;
        };
        self.start_menu.submenu = Some(SubmenuState {
            group,
            x,
            y,
            highlighted: 0,
        });
        true
    }

    pub fn activate_start_submenu(&mut self, index: usize) -> bool {
        let Some(submenu) = self.start_menu.submenu else {
            return false;
        };
        let Some(title) = Self::submenu_items(submenu.group).get(index) else {
            return false;
        };
        if submenu.group == MenuGroup::StartPrograms {
            self.start_menu.selected = Some(match index {
                0 => StartProgram::Explorer,
                1 => StartProgram::CommandPrompt,
                _ => StartProgram::Notepad,
            });
            self.launch_selected_start_program();
            return true;
        }
        self.start_menu.close();
        self.open_window(title, 80, 80, 480, 320);
        true
    }

    pub fn start_submenu_group(index: usize) -> Option<MenuGroup> {
        const GROUPS: [MenuGroup; 7] = [
            MenuGroup::StartPrograms,
            MenuGroup::StartDocuments,
            MenuGroup::StartPictures,
            MenuGroup::StartMusic,
            MenuGroup::StartComputer,
            MenuGroup::StartControlPanel,
            MenuGroup::StartDevices,
        ];
        GROUPS.get(index).copied()
    }

    pub fn activate_start_link(&mut self, index: usize) -> bool {
        let Some(group) = Self::start_submenu_group(index) else {
            return false;
        };
        let title = match group {
            MenuGroup::DesktopView | MenuGroup::DesktopSortBy | MenuGroup::DesktopNew => {
                "Desktop Item"
            }
            MenuGroup::StartPrograms => "Programs",
            MenuGroup::StartDocuments => "Documents",
            MenuGroup::StartPictures => "Pictures",
            MenuGroup::StartMusic => "Music",
            MenuGroup::StartComputer => "Computer",
            MenuGroup::StartControlPanel => "Control Panel",
            MenuGroup::StartDevices => "Devices and Printers",
        };
        let kind = match group {
            MenuGroup::StartComputer => WindowKind::Computer,
            MenuGroup::StartDocuments
            | MenuGroup::StartPictures
            | MenuGroup::StartMusic
            | MenuGroup::StartPrograms => WindowKind::Explorer,
            _ => WindowKind::Generic,
        };
        if matches!(
            group,
            MenuGroup::StartDocuments | MenuGroup::StartPictures | MenuGroup::StartMusic
        ) {
            let root = self.files.root_id();
            let directory = self
                .files
                .children(root)
                .iter()
                .find(|n| n.name == title)
                .map(|n| n.id)
                .unwrap_or_else(|| self.files.mkdir(root, title));
            self.files.navigate(directory);
        } else if kind == WindowKind::Computer {
            self.files.navigate(self.files.root_id());
        }
        let opened = self.open_window_with_kind(title, 80, 80, 480, 320, kind) > 0;
        self.start_menu.close();
        self.context_menu = None;
        self.context_submenu = None;
        opened
    }

    pub fn context_menu_items(&self) -> &'static [(&'static str, Option<ContextMenuCommand>)] {
        const DESKTOP: &[(&str, Option<ContextMenuCommand>)] = &[
            ("View", None),
            ("Sort by", None),
            ("Refresh", Some(ContextMenuCommand::Refresh)),
            ("", None),
            ("Paste", None),
            ("Paste shortcut", None),
            ("", None),
            ("New", None),
            ("", None),
            ("Screen resolution", Some(ContextMenuCommand::ScreenResolution)),
            ("Personalize", Some(ContextMenuCommand::Personalize)),
        ];
        const COMPUTER: &[(&str, Option<ContextMenuCommand>)] = &[
            ("Open", Some(ContextMenuCommand::Open)),
            ("Manage", Some(ContextMenuCommand::Manage)),
            ("", None),
            ("Properties", Some(ContextMenuCommand::Properties)),
        ];
        const ITEM: &[(&str, Option<ContextMenuCommand>)] = &[
            ("Open", Some(ContextMenuCommand::Open)), ("", None),
            ("Properties", Some(ContextMenuCommand::Properties)),
        ];
        const RECYCLE: &[(&str, Option<ContextMenuCommand>)] = &[
            ("Open", Some(ContextMenuCommand::Open)),
            ("Empty Recycle Bin", Some(ContextMenuCommand::EmptyRecycleBin)),
            ("", None), ("Properties", Some(ContextMenuCommand::Properties)),
        ];
        const TASK: &[(&str, Option<ContextMenuCommand>)] = &[
            ("Open", Some(ContextMenuCommand::Open)),
            ("Pin this program to taskbar", None),
            ("Close window", Some(ContextMenuCommand::CloseWindow)),
        ];
        const TASKBAR: &[(&str, Option<ContextMenuCommand>)] = &[
            ("Show desktop", Some(ContextMenuCommand::ShowDesktop)),
            (
                "Start Task Manager",
                Some(ContextMenuCommand::OpenTaskManager),
            ),
            (
                "Properties",
                Some(ContextMenuCommand::OpenTaskbarProperties),
            ),
        ];
        const START: &[(&str, Option<ContextMenuCommand>)] = &[
            (
                "Programs and Features",
                Some(ContextMenuCommand::OpenPrograms),
            ),
            ("Run...", Some(ContextMenuCommand::OpenRun)),
            ("Task Manager", Some(ContextMenuCommand::OpenTaskManager)),
            ("Shut Down", Some(ContextMenuCommand::ShutDown)),
        ];
        const VOLUME: &[(&str, Option<ContextMenuCommand>)] = &[
            (
                "Open Volume Mixer",
                Some(ContextMenuCommand::OpenVolumeMixer),
            ),
            ("Playback devices", None),
            ("Adjust audio properties", None),
        ];
        const NETWORK: &[(&str, Option<ContextMenuCommand>)] = &[
            (
                "Open Network and Sharing Center",
                Some(ContextMenuCommand::OpenNetworkSharing),
            ),
            ("Troubleshoot problems", None),
            ("Change adapter settings", None),
        ];
        const ACTION: &[(&str, Option<ContextMenuCommand>)] = &[
            (
                "Open Action Center",
                Some(ContextMenuCommand::OpenActionCenter),
            ),
            ("Review recent messages and resolve problems", None),
        ];
        const CLOCK: &[(&str, Option<ContextMenuCommand>)] = &[
            ("Adjust Date/Time", Some(ContextMenuCommand::OpenDateTime)),
            ("Open Calendar", None),
        ];
        const SHOW_DESKTOP: &[(&str, Option<ContextMenuCommand>)] = &[
            ("Show desktop", Some(ContextMenuCommand::ShowDesktop)),
            ("Properties", None),
        ];
        let items = match self.context_menu.as_ref().map(|menu| menu.target) {
            Some(ContextMenuTarget::Desktop) => &DESKTOP,
            Some(ContextMenuTarget::DesktopItem(index)) => match self.desktop.items.get(index).map(|item| item.icon) {
                Some(ImageResource::Computer) => &COMPUTER,
                Some(ImageResource::RecycleBin) => &RECYCLE,
                _ => &ITEM,
            },
            Some(ContextMenuTarget::TaskbarTask(_)) => &TASK,
            Some(ContextMenuTarget::Taskbar) => &TASKBAR,
            Some(ContextMenuTarget::StartButton) => &START,
            Some(ContextMenuTarget::Volume) => &VOLUME,
            Some(ContextMenuTarget::Network) => &NETWORK,
            Some(ContextMenuTarget::ActionCenter) => &ACTION,
            Some(ContextMenuTarget::Clock) => &CLOCK,
            Some(ContextMenuTarget::ShowDesktop) => &SHOW_DESKTOP,
            None => &DESKTOP,
        };
        items
    }

    pub fn activate_context_menu_command(&mut self, index: usize) -> bool {
        if !self.context_item_enabled(index) { return false; }
        let menu_target = self.context_menu.as_ref().map(|menu| menu.target);
        let Some(command) = self.context_menu_items().get(index).and_then(|item| item.1) else {
            return false;
        };
        self.context_menu = None;
        self.context_submenu = None;
        match command {
            ContextMenuCommand::CloseWindow => {
                if let Some(ContextMenuTarget::TaskbarTask(index)) = menu_target {
                    if let Some(id) = self.taskbar.windows.get(index).map(|w| w.id) {
                        self.close_window(id);
                        return true;
                    }
                }
                false
            }
            ContextMenuCommand::Open => {
                let opened = match menu_target {
                    Some(ContextMenuTarget::DesktopItem(index)) => {
                        self.open_desktop_item(index)
                    }
                    Some(ContextMenuTarget::TaskbarTask(index)) => {
                        if let Some(window) = self.taskbar.windows.get_mut(index) {
                            window.minimized = false;
                            self.selected_window = Some(window.id);
                            true
                        } else {
                            false
                        }
                    }
                    _ => false,
                };
                opened
            }
            ContextMenuCommand::Properties => {
                let Some(ContextMenuTarget::DesktopItem(index)) = menu_target else { return false; };
                let Some(icon) = self.desktop.items.get(index).map(|item| item.icon) else { return false; };
                let (title, kind) = match icon {
                    ImageResource::Computer => ("System", WindowKind::System),
                    ImageResource::Network => ("Network and Sharing Center", WindowKind::ShellSettings(Flyout::Network)),
                    ImageResource::RecycleBin => ("Recycle Bin Properties", WindowKind::RecycleBinProperties),
                    _ => ("Control Panel", WindowKind::ControlPanel),
                };
                self.open_window_with_kind(title, 180, 110, 560, 390, kind);
                true
            }
            ContextMenuCommand::Manage => {
                self.open_window_with_kind("Computer Management", 170, 100, 580, 400, WindowKind::ComputerManagement);
                true
            }
            ContextMenuCommand::EmptyRecycleBin => {
                self.open_window_with_kind("Delete Items", 260, 220, 460, 180, WindowKind::ConfirmEmptyRecycleBin);
                true
            }
            ContextMenuCommand::ScreenResolution => {
                self.open_window_with_kind("Screen Resolution", 180, 110, 560, 390, WindowKind::DisplaySettings);
                true
            }
            ContextMenuCommand::Personalize => {
                self.open_window_with_kind("Personalization", 180, 110, 560, 390, WindowKind::Personalization);
                true
            }
            ContextMenuCommand::Refresh => true,
            ContextMenuCommand::SortByName => {
                self.desktop.sort_mode = SortMode::Name;
                self.desktop.sort();
                true
            }
            ContextMenuCommand::SortByType => {
                self.desktop.sort_mode = SortMode::Type;
                self.desktop.sort();
                true
            }
            ContextMenuCommand::ShowDesktop => {
                self.toggle_desktop();
                true
            }
            ContextMenuCommand::OpenTaskbarProperties => {
                self.open_window("Taskbar and Start Menu Properties", 160, 140, 420, 260);
                true
            }
            ContextMenuCommand::OpenTaskManager => {
                self.launch_application(AppId::TaskManager);
                true
            }
            ContextMenuCommand::OpenPrograms => {
                self.open_window("Programs and Features", 180, 160, 480, 340);
                true
            }
            ContextMenuCommand::OpenRun => {
                self.launch_application(AppId::Run);
                true
            }
            ContextMenuCommand::ShutDown => {
                self.power_action = Some(PowerAction::ShutDown);
                true
            }
            ContextMenuCommand::OpenVolumeMixer => {
                self.open_window_with_kind(
                    "Volume Mixer",
                    220,
                    160,
                    480,
                    320,
                    WindowKind::ShellSettings(Flyout::Volume),
                );
                true
            }
            ContextMenuCommand::OpenNetworkSharing => {
                self.open_window_with_kind(
                    "Network and Sharing Center",
                    220,
                    160,
                    480,
                    320,
                    WindowKind::ShellSettings(Flyout::Network),
                );
                true
            }
            ContextMenuCommand::OpenActionCenter => {
                self.open_window_with_kind(
                    "Action Center",
                    220,
                    160,
                    480,
                    320,
                    WindowKind::ShellSettings(Flyout::ActionCenter),
                );
                true
            }
            ContextMenuCommand::OpenDateTime => {
                self.open_window_with_kind(
                    "Date and Time",
                    220,
                    160,
                    480,
                    320,
                    WindowKind::ShellSettings(Flyout::Clock),
                );
                true
            }
        }
    }

    pub fn namespace_links(kind: WindowKind) -> &'static [(&'static str, WindowKind)] {
        const CONTROL: &[(&str, WindowKind)] = &[
            ("System and Security", WindowKind::System),
            ("Screen resolution", WindowKind::DisplaySettings),
            ("Personalization", WindowKind::Personalization),
            ("Network and Sharing Center", WindowKind::ShellSettings(Flyout::Network)),
            ("Sound", WindowKind::ShellSettings(Flyout::Volume)),
            ("Date and Time", WindowKind::ShellSettings(Flyout::Clock)),
        ];
        const MANAGE: &[(&str, WindowKind)] = &[
            ("System information", WindowKind::System),
            ("Display adapter", WindowKind::DisplaySettings),
            ("Network adapters", WindowKind::ShellSettings(Flyout::Network)),
            ("Sound devices", WindowKind::ShellSettings(Flyout::Volume)),
        ];
        match kind { WindowKind::ControlPanel => CONTROL, WindowKind::ComputerManagement => MANAGE, _ => &[] }
    }

    pub fn open_desktop_item(&mut self, index: usize) -> bool {
        let Some(item) = self.desktop.items.get(index) else { return false; };
        let name = item.name.clone();
        let kind = match item.icon {
            ImageResource::Computer => WindowKind::Computer,
            ImageResource::Network => WindowKind::ShellSettings(Flyout::Network),
            ImageResource::RecycleBin => WindowKind::RecycleBin,
            ImageResource::ControlPanel => WindowKind::ControlPanel,
            _ => WindowKind::Explorer,
        };
        if kind == WindowKind::Computer { self.files.navigate(self.files.root_id()); }
        self.open_window_with_kind(&name, 130, 90, 600, 420, kind);
        true
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
        if matches!(kind, WindowKind::Explorer | WindowKind::Computer) {
            let directory=self.files.current_directory();
            let result=self.files.refresh_directory(directory);
            let mut explorer=Explorer::new(id,directory);
            explorer.prepare(&mut self.files);
            if let Err(status)=result{explorer.status=alloc::format!("Cannot open folder (0x{status:08X})");}
            self.explorers.push(explorer);
        }
        self.selected_window = Some(id);
        self.focus_window(id);
        id
    }

    pub fn close_window(&mut self, id: u32) {
        if self.drag_window == Some(id) { self.release_window_drag(); }
        self.applications.retain(|app|app.window_id!=id);
        self.explorers.retain(|app|app.window_id!=id);
        if self.text_target==Some(id) {self.text_target=None;}
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
        if self.taskbar.windows.iter().any(|w|w.id==id && matches!(w.kind,WindowKind::Notepad | WindowKind::Application(AppId::Notepad|AppId::WordPad|AppId::StickyNotes))) {self.text_target=Some(id);}
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
        if window.minimized || window.maximized || !window.bounds().contains(cursor_x, cursor_y) {
            return false;
        }
        let edge = window.resize_edge_at(cursor_x, cursor_y);
        if edge.is_none() && (cursor_y >= window.y + CAPTION_HEIGHT
            || window.caption_control_at(cursor_x, cursor_y).is_some()) {
            return false;
        }
        self.resize_drag = edge.map(|edge| ResizeDrag {
            edge,
            origin: (cursor_x, cursor_y),
            geometry: (window.x, window.y, window.width, window.height),
        });
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
            self.resize_drag = None;
            return false;
        };
        let previous = (window.x, window.y, window.width, window.height);
        if let Some(drag) = self.resize_drag {
            let next = drag.edge.resize(drag.geometry,
                cursor_x.saturating_sub(drag.origin.0), cursor_y.saturating_sub(drag.origin.1));
            (window.x, window.y, window.width, window.height) = next;
        } else {
            window.x = cursor_x.saturating_sub(self.drag_offset.0);
            window.y = cursor_y.saturating_sub(self.drag_offset.1);
        }
        previous != (window.x, window.y, window.width, window.height)
    }

    pub fn release_window_drag(&mut self) {
        self.drag_window = None;
        self.drag_offset = (0, 0);
        self.resize_drag = None;
    }

    pub fn window_cursor_at(&self, x: i32, y: i32) -> WindowCursor {
        if let Some(drag) = self.resize_drag { return drag.edge.cursor(); }
        self.window_hit_test(x, y)
            .and_then(|id| self.taskbar.windows.iter().find(|w| w.id == id))
            .and_then(|w| w.resize_edge_at(x, y))
            .map_or(WindowCursor::Arrow, |edge| edge.cursor())
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
        if window.minimized || window.resize_edge_at(cursor_x, cursor_y).is_some() {
            return false;
        }
        let kind = window.kind;
        let links = Self::namespace_links(kind);
        if (window.x+18..window.x+window.width-18).contains(&cursor_x) && cursor_y >= window.y+86 {
            let row = (cursor_y-window.y-86) / 36;
            if let Some((title, destination)) = links.get(row as usize) {
                self.open_window_with_kind(title, 190, 120, 560, 390, *destination);
                return true;
            }
        }
        if kind == WindowKind::ConfirmEmptyRecycleBin {
            let yes = Rect::new(window.x+window.width-200,window.y+window.height-42,80,26);
            let no = Rect::new(window.x+window.width-108,window.y+window.height-42,80,26);
            if yes.contains(cursor_x,cursor_y) {
                let ids: Vec<u32> = self.files.recycle_bin().iter().map(|n| n.id).collect();
                for id in ids { self.files.recycle_permanently(id); }
                self.request_application_close(window_id);
                return true;
            }
            if no.contains(cursor_x,cursor_y) { self.close_window(window_id); return true; }
        }
        if kind == WindowKind::RecycleBin && Rect::new(window.x+18,window.y+70,140,26).contains(cursor_x,cursor_y) {
            let ids: Vec<u32> = self.files.recycle_bin().iter().map(|n| n.id).collect();
            for id in ids { self.files.restore(id); }
            return true;
        }
        let Some(button) = window.caption_control_at(cursor_x, cursor_y) else { return false; };
        if button == 0 {
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
        if button == 1 {
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
        if button == 2 {
            self.request_application_close(window_id);
            return true;
        }
        false
    }

    pub fn handle_window_key(&mut self, key: u8) -> bool {
        let Some(window_id) = self.selected_window else {
            return false;
        };
        if self.application_key(window_id,key) {return true;}
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

    pub fn handle_shell_key(&mut self, key: u8) -> bool {
        match key {
            27 => {
                if self.context_menu.is_some() || self.start_menu.open || self.flyout.is_some() {
                    self.close_context_menu();
                    self.flyout = None;
                    self.start_menu.close();
                    return true;
                }
                false
            }
            5 => {
                self.desktop.sort();
                true
            }
            1 => {
                let count = self.desktop.items.len();
                self.desktop.selected = (0..count).collect();
                true
            }
            13 | 10 if self.context_menu.is_some() => self.activate_context_menu_command(0),
            _ => false,
        }
    }

    pub fn handle_popup_key(&mut self, key: u8) -> bool {
        if key == 27 {
            return self.handle_control_event(ControlEvent::Cancel);
        }
        if self.flyout == Some(Flyout::Power) && (key == 0x26 || key == 0x28) {
            let current = self.controls.focus().saturating_sub(CONTROL_POWER_BASE) as i32;
            let next = (current + if key == 0x26 { -1 } else { 1 }).rem_euclid(4);
            self.controls.set_focus(CONTROL_POWER_BASE + next as u32);
            return true;
        }
        if self.context_menu.is_some() && (key == 0x26 || key == 0x28) {
            let count = self.context_menu_items().len() as i32;
            let current = self
                .controls
                .focus()
                .saturating_sub(CONTROL_CONTEXT_MENU_BASE) as i32;
            let direction = if key == 0x26 { -1 } else { 1 };
            let next = (1..=count).map(|step| (current + direction * step).rem_euclid(count))
                .find(|index| self.context_item_enabled(*index as usize)).unwrap_or(current);
            self.controls.set_focus(CONTROL_CONTEXT_MENU_BASE + next as u32);
            return true;
        }
        if key == 9 {
            let ids: Vec<u32> = self
                .controls
                .controls()
                .iter()
                .filter(|c| c.kind == ControlKind::Button || c.kind == ControlKind::Edit)
                .filter(|c| {
                    if self.flyout.is_some() {
                        c.id == CONTROL_FLYOUT_LINK
                            || (CONTROL_MUTE..=CONTROL_CALENDAR_NEXT).contains(&c.id)
                            || (CONTROL_POWER_BASE..CONTROL_POWER_BASE + 4).contains(&c.id)
                    } else if self.context_menu.is_some() {
                        (CONTROL_CONTEXT_MENU_BASE..CONTROL_START_LINK_BASE).contains(&c.id)
                    } else {
                        (CONTROL_MENU_BASE..CONTROL_MENU_BASE + 100).contains(&c.id)
                            || (CONTROL_START_LINK_BASE..CONTROL_START_LINK_BASE + 7)
                                .contains(&c.id)
                            || (CONTROL_SEARCH..=CONTROL_POWER_ARROW).contains(&c.id)
                    }
                })
                .map(|c| c.id)
                .collect();
            if !ids.is_empty() {
                let next = ids
                    .iter()
                    .position(|id| *id == self.controls.focus())
                    .map(|i| (i + 1) % ids.len())
                    .unwrap_or(0);
                self.controls.set_focus(ids[next]);
            }
            return true;
        }
        if self.start_menu.open && self.flyout.is_none() {
            if key == 13 || key == 10 {
                let focus = self.controls.focus();
                if focus == CONTROL_SEARCH || focus == CONTROL_START {
                    return self.handle_start_menu_key(key);
                }
                return self.activate_control(focus);
            }
            if key == 0x26 || key == 0x28 {
                self.start_menu
                    .move_highlight(if key == 0x26 { -1 } else { 1 });
                self.controls
                    .set_focus(CONTROL_MENU_BASE + self.start_menu.highlighted as u32);
                return true;
            }
            self.controls.set_focus(CONTROL_SEARCH);
            self.start_menu.submenu = None;
            return self.handle_start_menu_key(key);
        }
        if self.flyout == Some(Flyout::Volume) {
            match key {
                0x26 | b'+' => {
                    self.volume = self.volume.saturating_add(5).min(100);
                    return true;
                }
                0x28 | b'-' => {
                    self.volume = self.volume.saturating_sub(5);
                    return true;
                }
                b'm' | b'M' => {
                    self.muted = !self.muted;
                    return true;
                }
                _ => {}
            }
        }
        if key == 13 || key == 10 || key == b' ' {
            return self.activate_control(self.controls.focus());
        }
        false
    }

    pub fn handle_start_menu_key(&mut self, key: u8) -> bool {
        if !self.start_menu.open {
            return false;
        }
        let changed = match key {
            27 => {
                self.start_menu.close();
                self.flyout = None;
                true
            }
            13 | 10 => {
                self.start_menu.confirm();
                self.launch_selected_start_program();
                true
            }
            0x26 => {
                self.start_menu.move_highlight(-1);
                true
            }
            0x28 => {
                self.start_menu.move_highlight(1);
                true
            }
            8 => {
                self.start_menu.query.pop();
                self.start_menu.highlighted = 0;
                true
            }
            0x20..=0x7e => {
                let mut query = alloc::string::String::from(&self.start_menu.query);
                query.push(key as char);
                self.start_menu.set_query(&query);
                true
            }
            _ => false,
        };
        if changed {
            self.start_menu.selected = None;
        }
        changed
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
            for window in &mut self.taskbar.windows {
                if self.desktop_restore.contains(&window.id) {
                    window.minimized = false;
                }
            }
            self.desktop_restore.clear();
        } else {
            self.desktop_restore = self
                .taskbar
                .windows
                .iter()
                .filter(|w| !w.minimized)
                .map(|w| w.id)
                .collect();
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
            .max_by_key(|window| (Some(window.id) == self.selected_window, window.id))
            .map(|window| window.id)
    }

    pub fn launch_selected_start_program(&mut self) -> Option<u32> {
        let program = self.start_menu.selected?;
        if let StartProgram::Folder(category)=program {self.start_menu.category=Some(category);self.start_menu.highlighted=0;self.start_menu.selected=None;return None;}
        self.start_menu.close();
        if let StartProgram::Application(id) = program { return Some(self.launch_application(id)); }
        if program==StartProgram::Notepad {return Some(self.launch_application(AppId::Notepad));}
        if program==StartProgram::CommandPrompt {let id=self.launch_application(AppId::CommandPrompt);if let Some(w)=self.taskbar.windows.iter_mut().find(|w|w.id==id){w.x=60;w.y=60;w.width=480;w.height=320;}return Some(id);}
        let (title, kind) = match program {
            StartProgram::Application(_) | StartProgram::Folder(_) => unreachable!(),
            StartProgram::Notepad => (StartProgram::Notepad.title(), WindowKind::Notepad),
            StartProgram::CommandPrompt => (
                StartProgram::CommandPrompt.title(),
                WindowKind::CommandPrompt,
            ),
            StartProgram::Computer => (StartProgram::Computer.title(), WindowKind::Computer),
            StartProgram::Explorer => (StartProgram::Explorer.title(), WindowKind::Explorer),
        };
        let (width,height)=if matches!(kind,WindowKind::Explorer|WindowKind::Computer){(720,500)}else{(480,320)};
        Some(self.open_window_with_kind(title, 60, 60, width, height, kind))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerAction {
    Lock,
    LogOff,
    Restart,
    ShutDown,
}
pub const CONTROL_POPUP_BACKGROUND: u32 = 2900;
pub const CONTROL_OVERFLOW: u32 = 2901;
pub const CONTROL_PIN_EXPLORER: u32 = 2902;
pub const CONTROL_SEARCH: u32 = 2903;
pub const CONTROL_ALL_PROGRAMS: u32 = 2904;
pub const CONTROL_SHUTDOWN: u32 = 2905;
pub const CONTROL_POWER_ARROW: u32 = 2906;
pub const CONTROL_FLYOUT_LINK: u32 = 2907;
pub const CONTROL_VOLUME_TRACK: u32 = 2908;
pub const CONTROL_MUTE: u32 = 2909;
pub const CONTROL_CALENDAR_PREV: u32 = 2910;
pub const CONTROL_CALENDAR_NEXT: u32 = 2911;
pub const CONTROL_POWER_BASE: u32 = 2920;
pub const CONTROL_PROGRAM_PREV:u32=2930;
pub const CONTROL_PROGRAM_NEXT:u32=2931;
pub const CONTROL_START: u32 = 2001;
pub const CONTROL_TASKBAR_BASE: u32 = 2100;
pub const CONTROL_DESKTOP_BASE: u32 = 2200;
pub const CONTROL_MENU_BASE: u32 = 2300;
pub const CONTROL_TRAY_BASE: u32 = 2400;
pub const CONTROL_TRAY_VOLUME: u32 = CONTROL_TRAY_BASE;
pub const CONTROL_TRAY_NETWORK: u32 = CONTROL_TRAY_BASE + 1;
pub const CONTROL_TRAY_ACTION_CENTER: u32 = CONTROL_TRAY_BASE + 2;
pub const CONTROL_CLOCK: u32 = 2500;
pub const CONTROL_SHOW_DESKTOP: u32 = 2501;
pub const TRAY_CONTROL_COUNT: u32 = 3;
pub const CONTROL_CONTEXT_MENU_BASE: u32 = 2600;
pub const CONTROL_SUBMENU_BASE: u32 = 2700;
pub const CONTROL_START_LINK_BASE: u32 = 2800;

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
    fn desktop_namespace_context_open_uses_the_real_application() {
        let mut shell = ShellModel::new(Theme::aero());
        shell.desktop.add("Computer", ImageResource::Computer, 12, 12);
        shell.open_context_menu(30, 30, 1024, 768);
        assert!(shell.activate_context_menu_command(0));
        assert_eq!(shell.taskbar.windows.last().unwrap().kind, WindowKind::Computer);
    }

    #[test]
    fn desktop_pointer_and_context_hit_regions_agree() {
        let mut shell = ShellModel::new(Theme::aero());
        shell.desktop.add("Computer", ImageResource::Computer, 12, 12);
        shell.refresh_controls(1024, 768);
        for (x, y) in [(20, 20), (100, 80), (108, 80), (80, 108)] {
            let desktop = shell.desktop.hit_test(x, y).is_some();
            let control = shell.controls.hit_test(x, y) == CONTROL_DESKTOP_BASE;
            assert_eq!(desktop, control, "hit regions disagree at ({x}, {y})");
        }
    }

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
        assert!(!shell.taskbar.theme.composition);
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
        assert_eq!(start.search(), vec![StartProgram::Explorer,StartProgram::Application(AppId::GamesExplorer)]);
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
    fn window_borders_resize_in_all_eight_directions() {
        for (point, delta, expected) in [
            ((100, 200), (-40, 0), (60, 100, 440, 300)),
            ((499, 200), (40, 0), (100, 100, 440, 300)),
            ((250, 100), (0, -40), (100, 60, 400, 340)),
            ((250, 399), (0, 40), (100, 100, 400, 340)),
            ((100, 100), (-40, -40), (60, 60, 440, 340)),
            ((499, 100), (40, -40), (100, 60, 440, 340)),
            ((100, 399), (-40, 40), (60, 100, 440, 340)),
            ((499, 399), (40, 40), (100, 100, 440, 340)),
        ] {
            let mut shell = ShellModel::new(Theme::aero());
            let id = shell.open_window("Resize", 100, 100, 400, 300);
            assert!(shell.begin_window_drag(id, point.0, point.1), "border {point:?}");
            assert!(shell.drag_window_to(point.0 + delta.0, point.1 + delta.1));
            let w = &shell.taskbar.windows[0];
            assert_eq!((w.x, w.y, w.width, w.height), expected, "border {point:?}");
            shell.release_window_drag();
            assert!(!shell.drag_window_to(800, 600));
        }
    }

    #[test]
    fn window_caption_controls_leave_the_outer_frame_available_for_resizing() {
        let mut shell = ShellModel::new(Theme::aero());
        let id = shell.open_window("Controls", 100, 100, 400, 300);
        assert!(!shell.handle_window_control_click(id, 499, 110, 1024, 728));
        assert_eq!(shell.taskbar.windows.len(), 1);
        let id = shell.open_window_with_kind("Control Panel", 100, 100, 400, 120, WindowKind::ControlPanel);
        assert!(!shell.handle_window_control_click(id, 250, 219, 1024, 728));
        assert!(shell.begin_window_drag(id, 250, 219));
        assert!(shell.drag_window_to(250, 260));
        assert_eq!(shell.taskbar.windows.len(), 2);
    }

    #[test]
    fn window_resize_clamps_at_minimum_and_retains_the_opposite_corner() {
        let mut shell = ShellModel::new(Theme::aero());
        let id = shell.open_window("Resize", 100, 100, 400, 300);
        assert!(shell.begin_window_drag(id, 100, 100));
        assert!(shell.drag_window_to(900, 700));
        let w = &shell.taskbar.windows[0];
        assert_eq!((w.x, w.y, w.width, w.height), (260, 280, 240, 120));
        assert_eq!(shell.window_cursor_at(900, 700), WindowCursor::NorthWestSouthEast);
        assert!(shell.drag_window_to(60, 60));
        let w = &shell.taskbar.windows[0];
        assert_eq!((w.x, w.y, w.width, w.height), (60, 60, 440, 340));
        shell.release_window_drag();
        assert_eq!(shell.window_cursor_at(900, 700), WindowCursor::Arrow);
        assert!(shell.begin_window_drag(id, 60, 60));
        shell.close_window(id);
        assert!(!shell.drag_window_to(30, 30));
    }

    #[test]
    fn window_controls_and_resize_use_signed_geometry_and_topmost_window() {
        let mut shell = ShellModel::new(Theme::aero());
        let id = shell.open_window("Clipped", -120, -10, 400, 300);
        let w = &shell.taskbar.windows[0];
        assert_eq!(w.caption_control_group(), Rect::new(172, -9, 102, 20));
        assert_eq!(w.caption_control_at(250, 5), Some(2));
        assert_eq!(shell.window_hit_test(279, 50), Some(id));
        assert_eq!(shell.window_cursor_at(279, 50), WindowCursor::Horizontal);
        let overlay = shell.open_window("Overlay", 200, 0, 400, 300);
        assert_eq!(shell.window_hit_test(279, 50), Some(overlay));
        assert_eq!(shell.window_cursor_at(279, 50), WindowCursor::Arrow);
        let button = shell.taskbar.windows[1].caption_controls()[1];
        assert!(shell.handle_window_control_click(overlay, button.x + 10, button.y + 10, 1024, 728));
        assert!(!shell.begin_window_drag(overlay, 0, 0));
        assert_eq!(shell.window_cursor_at(0, 0), WindowCursor::Arrow);
        let button = shell.taskbar.windows[1].caption_controls()[1];
        assert!(shell.handle_window_control_click(overlay, button.x + 10, button.y + 10, 1024, 728));
        assert_eq!(shell.taskbar.windows[1].bounds(), Rect::new(200, 0, 400, 300));
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
        desktop.auto_arrange(1024, 768);
        assert_eq!(desktop.items[0].name, "A");
        let mut shell = ShellModel::new(Theme::aero());
        shell.desktop = desktop.clone();
        shell.refresh_controls(1024, 768);
        assert!(shell.open_context_menu(24, 24, 1024, 768));
        assert_eq!(
            shell.context_menu.as_ref().map(|menu| menu.target),
            Some(ContextMenuTarget::DesktopItem(0))
        );
        assert!(shell.desktop.context_menu.is_none());
        shell.close_context_menu();
        assert!(shell.context_menu.is_none());
    }

    #[test]
    fn context_menu_items_are_clickable_and_activate_commands() {
        let mut shell = ShellModel::new(Theme::aero());
        shell.refresh_controls(1024, 768);
        assert!(shell.open_context_menu(32, 320, 1024, 768));
        shell.refresh_controls(1024, 768);
        let menu_id = shell
            .controls
            .hit_test(60, 332)
            .max(shell.controls.hit_test(70, 344));
        assert!(menu_id >= CONTROL_CONTEXT_MENU_BASE);
        assert!(shell.activate_context_menu_command(2));
        assert!(shell.context_menu.is_none());
        assert!(shell.desktop.context_menu.is_none());
    }

    #[test]
    fn context_submenus_open_and_activate_sort_commands() {
        let mut shell = ShellModel::new(Theme::aero());
        shell.desktop.sort_mode = SortMode::Type;
        shell.desktop.add("B", ImageResource::Folder, 0, 0);
        shell.desktop.add("A", ImageResource::Folder, 0, 0);
        shell.refresh_controls(1024, 768);
        assert!(shell.open_context_menu(32, 320, 1024, 768));
        assert!(shell.context_submenu_group(1).is_some());
        assert!(shell.context_submenu_group(2).is_none());
        assert!(shell.open_context_submenu(1, 180, 324));
        shell.refresh_controls(1024, 768);
        assert!(shell.context_submenu.is_some());
        assert!(shell.context_submenu_bounds(1024, 768).is_some());
        assert!(shell
            .controls
            .control(CONTROL_SUBMENU_BASE)
            .unwrap()
            .rect
            .contains(200, 336));
        assert_eq!(shell.controls.hit_test(200, 336), CONTROL_SUBMENU_BASE);
        assert!(shell.activate_context_submenu(0));
        assert!(shell.context_menu.is_none());
        assert!(shell.context_submenu.is_none());
        assert_eq!(shell.desktop.sort_mode, SortMode::Name);
    }

    #[test]
    fn outside_pointer_closes_shell_popups() {
        let mut shell = ShellModel::new(Theme::aero());
        shell.start_menu.open = true;
        assert!(shell.open_context_menu(500, 300, 1024, 768));
        assert!(shell.close_popups_if_outside(900, 100, 1024, 768));
        assert!(!shell.start_menu.open);
        assert!(shell.context_menu.is_none());
        assert!(shell.context_submenu.is_none());
    }

    #[test]
    fn start_menu_links_have_submenu_commands() {
        let mut shell = ShellModel::new(Theme::aero());
        shell.start_menu.open = true;
        shell.refresh_controls(1024, 768);
        assert!(shell.open_start_submenu(0, 300, 300));
        shell.refresh_controls(1024, 768);
        assert!(shell.controls.hit_test(320, 312) >= CONTROL_SUBMENU_BASE);
        assert!(shell.activate_context_submenu(0));
        assert!(!shell.start_menu.open);
        assert_eq!(shell.taskbar.windows[0].title, "Windows Explorer");
    }
}

#[cfg(test)]
mod ui_regression_tests {
    use super::*;
    #[test]
    fn sorting_moves_icons_between_slots_and_preserves_selected_identity() {
        let mut desktop = DesktopState::new();
        desktop.add("Z", ImageResource::Folder, 12, 12);
        desktop.add("A", ImageResource::Folder, 12, 108);
        desktop.select(0);
        desktop.sort();
        assert_eq!(desktop.items[0].name, "A");
        assert_eq!((desktop.items[0].x, desktop.items[0].y), (12, 12));
        assert_eq!(desktop.selected, [1]);
        assert_eq!(desktop.items[desktop.selected[0]].name, "Z");
    }

    #[test]
    fn unsupported_desktop_submenu_commands_cannot_open_placeholder_windows() {
        let mut shell = ShellModel::new(Theme::aero());
        shell.open_context_menu(500, 300, 1024, 768);
        shell.open_context_submenu(1, 600, 300);
        assert!(!shell.activate_context_submenu(1));
        assert!(shell.taskbar.windows.is_empty());
        assert!(shell.context_submenu.is_some());
    }
    #[test]
    fn clock_and_desktop_strip_are_reachable() {
        let mut shell = ShellModel::new(Theme::aero());
        shell.refresh_controls(1024, 768);
        assert_eq!(shell.controls.hit_test(974, 748), CONTROL_CLOCK);
        assert!(shell.handle_control_event(ControlEvent::Activated(CONTROL_CLOCK)));
        assert!(
            shell.taskbar.windows.is_empty(),
            "clock should open a flyout"
        );
        assert_eq!(shell.controls.hit_test(1020, 748), CONTROL_SHOW_DESKTOP);
    }
    #[test]
    fn filtered_start_click_launches_the_displayed_result() {
        let mut shell = ShellModel::new(Theme::aero());
        shell.start_menu.open = true;
        shell.start_menu.set_query("expl");
        shell.handle_control_event(ControlEvent::Activated(CONTROL_MENU_BASE));
        assert_eq!(shell.taskbar.windows[0].kind, WindowKind::Explorer);
    }
    #[test]
    fn start_links_are_activated_by_their_control_ids() {
        let mut shell = ShellModel::new(Theme::aero());
        shell.start_menu.open = true;
        assert!(shell.handle_control_event(ControlEvent::Activated(CONTROL_START_LINK_BASE + 4)));
        assert_eq!(shell.taskbar.windows[0].title, "Computer");
    }
}

#[cfg(test)]
mod shell_popup_tests {
    use super::*;
    use crate::controls::PointerAction;

    fn click(shell: &mut ShellModel, id: u32) {
        shell.refresh_controls(1024, 768);
        let b = shell.controls.control(id).unwrap().rect;
        let x = b.x + b.width as i32 / 2;
        let y = b.y + b.height as i32 / 2;
        if let Some(event) = shell.controls.route_pointer(PointerAction::Down, x, y) {
            shell.handle_control_event(event);
        }
        shell.refresh_controls(1024, 768);
        if let Some(event) = shell.controls.route_pointer(PointerAction::Up, x, y) {
            shell.handle_control_event(event);
        }
    }
    #[test]
    fn power_split_pointer_opens_over_an_existing_window() {
        let mut shell = ShellModel::new(Theme::aero());
        shell.open_window("Command Prompt", 60, 60, 480, 320);
        click(&mut shell, CONTROL_START);
        shell.refresh_controls(1024, 768);
        assert_eq!(shell.controls.hit_test(416, 704), CONTROL_POWER_ARROW);
        assert!(shell.popup_contains(416, 704, 1024, 768));
        assert!(!shell.close_popups_if_outside(416, 704, 1024, 768));
        click(&mut shell, CONTROL_POWER_ARROW);
        shell.refresh_controls(1024, 768);
        assert_eq!(shell.flyout, Some(Flyout::Power));
        assert!(shell.start_menu.open);
        assert!(shell.controls.hit_test(460, 668) >= CONTROL_POWER_BASE);
        click(&mut shell, CONTROL_POWER_ARROW);
        assert_eq!(shell.flyout, None);
    }

    #[test]
    fn power_keyboard_cycles_only_visible_commands() {
        let mut shell = ShellModel::new(Theme::aero());
        click(&mut shell, CONTROL_START);
        click(&mut shell, CONTROL_POWER_ARROW);
        shell.refresh_controls(1024, 768);
        assert!(shell.controls.control(CONTROL_FLYOUT_LINK).is_none());
        assert_eq!(shell.controls.focus(), CONTROL_POWER_BASE);
        shell.handle_popup_key(0x26);
        assert_eq!(shell.controls.focus(), CONTROL_POWER_BASE + 3);
        shell.handle_popup_key(0x28);
        assert_eq!(shell.controls.focus(), CONTROL_POWER_BASE);
        shell.handle_popup_key(9);
        assert_eq!(shell.controls.focus(), CONTROL_POWER_BASE + 1);
        shell.handle_popup_key(13);
        assert_eq!(shell.power_action, Some(PowerAction::LogOff));
    }

    #[test]
    fn pointer_capture_survives_relayout_and_popups_toggle_exclusively() {
        let mut shell = ShellModel::new(Theme::aero());
        click(&mut shell, CONTROL_START);
        assert!(shell.start_menu.open);
        for id in [
            CONTROL_OVERFLOW,
            CONTROL_TRAY_ACTION_CENTER,
            CONTROL_TRAY_NETWORK,
            CONTROL_TRAY_VOLUME,
            CONTROL_CLOCK,
        ] {
            click(&mut shell, id);
            assert!(shell.flyout.is_some());
            assert!(!shell.start_menu.open);
            click(&mut shell, id);
            assert_eq!(shell.flyout, None);
        }
        assert!(shell.taskbar.windows.is_empty());
    }
    #[test]
    fn tray_controls_and_start_rows_use_disjoint_visible_geometry() {
        for (w, h) in [(640, 480), (800, 600), (1024, 768), (1920, 1080)] {
            let mut shell = ShellModel::new(Theme::aero());
            shell.start_menu.open = true;
            shell.refresh_controls(w, h);
            let ids = [
                CONTROL_OVERFLOW,
                CONTROL_TRAY_ACTION_CENTER,
                CONTROL_TRAY_NETWORK,
                CONTROL_TRAY_VOLUME,
                CONTROL_CLOCK,
                CONTROL_SHOW_DESKTOP,
            ];
            for (i, id) in ids.iter().enumerate() {
                let b = shell.controls.control(*id).unwrap().rect;
                assert_eq!(
                    shell.controls.hit_test(b.x + b.width as i32 / 2, b.y + 15),
                    *id
                );
                for other in &ids[i + 1..] {
                    assert!(b
                        .intersection(&shell.controls.control(*other).unwrap().rect)
                        .is_none());
                }
            }
            let l = ShellLayout::new(w, h);
            assert!(l.start().y >= 0);
            assert!(l.start().x + l.start().width as i32 <= w as i32);
            let last = l.link(6);
            assert!(last.y + last.height as i32 <= l.shutdown().y);
            for kind in [
                Flyout::Overflow,
                Flyout::Volume,
                Flyout::Network,
                Flyout::ActionCenter,
                Flyout::Clock,
                Flyout::Power,
            ] {
                let b = l.flyout(kind);
                assert!(b.x >= 0 && b.y >= 0);
                assert!(b.x + b.width as i32 <= w as i32);
                assert!(b.y + b.height as i32 <= l.taskbar_top());
            }
        }
    }
    #[test]
    fn clock_calendar_handles_gregorian_edges_and_unavailable_rtc() {
        let mut shell = ShellModel::new(Theme::aero());
        shell.update_clock(0);
        assert_eq!(shell.taskbar.date, None);
        assert_eq!(shell.taskbar.clock, "--:--");
        shell.update_clock(116_444_736_000_000_000);
        assert_eq!(
            shell.taskbar.date,
            Some(CalendarDate {
                year: 1970,
                month: 1,
                day: 1
            })
        );
        assert_eq!(shell.taskbar.date.unwrap().weekday(), 4);
        assert_eq!(
            CalendarDate {
                year: 2000,
                month: 2,
                day: 1
            }
            .days_in_month(),
            29
        );
        assert_eq!(
            CalendarDate {
                year: 2100,
                month: 2,
                day: 1
            }
            .days_in_month(),
            28
        );
        click(&mut shell, CONTROL_CLOCK);
        click(&mut shell, CONTROL_CALENDAR_PREV);
        assert_eq!(
            shell.calendar_month,
            Some(CalendarDate {
                year: 1969,
                month: 12,
                day: 1
            })
        );
    }
    #[test]
    fn submenu_edge_clamping_matches_controls_and_outside_dismissal() {
        let mut shell = ShellModel::new(Theme::aero());
        shell.start_menu.open = true;
        shell.open_start_submenu(0, 1000, 700);
        shell.refresh_controls(1024, 768);
        let b = shell.context_submenu_bounds(1024, 768).unwrap();
        let c = shell.controls.control(CONTROL_SUBMENU_BASE).unwrap().rect;
        assert!(b.contains(c.x, c.y));
        assert!(!shell.close_popups_if_outside(c.x + 10, c.y + 10, 1024, 768));
        click(&mut shell, CONTROL_SUBMENU_BASE + 2);
        assert_eq!(shell.taskbar.windows[0].kind, WindowKind::Application(AppId::Notepad));
        shell.open_context_menu(1020, 760, 1024, 768);
        shell.refresh_controls(1024, 768);
        let b = shell.context_menu_bounds(1024, 768).unwrap();
        let c = shell
            .controls
            .control(CONTROL_CONTEXT_MENU_BASE)
            .unwrap()
            .rect;
        let row = shell.context_item_bounds(0, 1024, 768).unwrap();
        assert_eq!(c, row);
        assert!(b.contains(c.x, c.y));
        assert!(shell.close_popups_if_outside(500, 100, 1024, 768));
    }
    #[test]
    fn keyboard_search_and_taskbar_minimize_restore_remain_usable() {
        let mut shell = ShellModel::new(Theme::aero());
        click(&mut shell, CONTROL_START);
        for key in b"cmd" {
            shell.handle_popup_key(*key);
            shell.refresh_controls(1024, 768);
        }
        assert_eq!(
            shell.start_menu.visible_programs(),
            vec![StartProgram::CommandPrompt]
        );
        shell.handle_popup_key(13);
        assert_eq!(shell.taskbar.windows[0].kind, WindowKind::CommandPrompt);
        click(&mut shell, CONTROL_TASKBAR_BASE);
        assert!(shell.taskbar.windows[0].minimized);
        click(&mut shell, CONTROL_TASKBAR_BASE);
        assert!(!shell.taskbar.windows[0].minimized);
        click(&mut shell, CONTROL_TRAY_VOLUME);
        click(&mut shell, CONTROL_MUTE);
        assert!(shell.muted);
        let t = ShellLayout::new(1024, 768).volume_track();
        shell.set_volume_at(t.y, 1024, 768);
        assert_eq!(shell.volume, 100);
        shell.set_volume_at(t.y + t.height as i32, 1024, 768);
        assert_eq!(shell.volume, 0);
        shell.handle_popup_key(27);
        assert_eq!(shell.flyout, None);
    }

    #[test]
    fn crowded_taskbar_keeps_last_window_accessible_and_targets_its_context_menu() {
        let mut shell = ShellModel::new(Theme::aero());
        for _ in 0..8 {
            shell.open_window("Window", 80, 80, 400, 240);
        }
        shell.refresh_controls(640, 480);
        let last = shell
            .controls
            .control(CONTROL_TASKBAR_BASE + 7)
            .unwrap()
            .rect;
        assert!(last.x + last.width as i32 <= notification_area_left(640) as i32);
        click(&mut shell, CONTROL_TASKBAR_BASE + 7);
        assert!(shell.taskbar.windows[7].minimized);
        shell.open_context_menu(last.x + 1, last.y + 10, 640, 480);
        assert_eq!(
            shell.context_menu.as_ref().unwrap().target,
            ContextMenuTarget::TaskbarTask(7)
        );
        shell.activate_context_menu_command(2);
        assert_eq!(shell.taskbar.windows.len(), 7);
    }

    #[test]
    fn tray_settings_open_status_view_and_context_keyboard_stays_inside_menu() {
        let mut shell = ShellModel::new(Theme::aero());
        click(&mut shell, CONTROL_TRAY_NETWORK);
        click(&mut shell, CONTROL_FLYOUT_LINK);
        assert_eq!(
            shell.taskbar.windows[0].kind,
            WindowKind::ShellSettings(Flyout::Network)
        );
        shell.open_context_menu(900, 400, 1024, 768);
        shell.refresh_controls(1024, 768);
        assert_eq!(shell.controls.focus(), CONTROL_CONTEXT_MENU_BASE);
        shell.handle_popup_key(0x28);
        assert_eq!(shell.controls.focus(), CONTROL_CONTEXT_MENU_BASE + 1);
    }
}
