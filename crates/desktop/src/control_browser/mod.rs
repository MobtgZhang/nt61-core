//! Window-local Control Panel navigation, shared layout and Win7 page rendering.
use crate::applications::explorer::theme;
use crate::{
    applications::{AppId, ApplicationSession},
    graphics::{Rect, Surface},
    shell::{Flyout, ShellModel, WindowKind},
};
use alloc::{string::String, vec::Vec};

pub mod paint;
pub mod strings;
#[cfg(test)]
mod tests;

pub const TOOLBAR: u32 = 36;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    AllItems,
    Properties(u8),
    DisplayAdvanced,
    TextSize,
    ResolutionHelp,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum View {
    #[default]
    Category,
    LargeIcons,
    SmallIcons,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Visit {
    pub kind: WindowKind,
    pub title: String,
    pub scroll: u32,
    pub query: String,
    pub view: View,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Browser {
    pub window_id: u32,
    pub history: Vec<Visit>,
    pub cursor: usize,
    pub view_open: bool,
    pub search_focus: bool,
    pub message: String,
}
impl Browser {
    pub fn new(window_id: u32, title: &str, kind: WindowKind) -> Self {
        Self {
            window_id,
            history: alloc::vec![Visit {
                kind,
                title: title.into(),
                scroll: 0,
                query: String::new(),
                view: if kind == WindowKind::ControlPage(Page::AllItems) {
                    View::LargeIcons
                } else {
                    View::Category
                }
            }],
            cursor: 0,
            view_open: false,
            search_focus: false,
            message: String::new(),
        }
    }
    pub fn current(&self) -> &Visit {
        &self.history[self.cursor]
    }
    pub fn current_mut(&mut self) -> &mut Visit {
        &mut self.history[self.cursor]
    }
    pub fn can_back(&self) -> bool {
        self.cursor > 0
    }
    pub fn can_forward(&self) -> bool {
        self.cursor + 1 < self.history.len()
    }
    pub fn visit(&mut self, title: &str, kind: WindowKind) {
        if self.current().kind == kind && self.current().query.is_empty() {
            return;
        }
        self.history.truncate(self.cursor + 1);
        self.history.push(Visit {
            kind,
            title: title.into(),
            scroll: 0,
            query: String::new(),
            view: if kind == WindowKind::ControlPage(Page::AllItems) {
                View::LargeIcons
            } else {
                View::Category
            },
        });
        if self.history.len() > 128 {
            self.history.remove(0);
        }
        self.cursor = self.history.len() - 1;
        self.view_open = false;
        self.search_focus = false;
        self.message.clear();
    }
}
pub fn is_page(kind: WindowKind) -> bool {
    matches!(
        kind,
        WindowKind::ControlPanel
            | WindowKind::ControlCategory(_)
            | WindowKind::System
            | WindowKind::DisplaySettings
            | WindowKind::ControlPage(_)
            | WindowKind::UserAccounts
            | WindowKind::SoundSettings
            | WindowKind::Personalization
            | WindowKind::ShellSettings(Flyout::Network)
    )
}
pub fn body(window: &crate::shell::WindowState) -> Rect {
    Rect::new(
        window.x + 6,
        window.y + 30,
        window.width.saturating_sub(12) as u32,
        window.height.saturating_sub(36) as u32,
    )
}
pub fn content(b: Rect) -> Rect {
    Rect::new(
        b.x,
        b.y + TOOLBAR as i32,
        b.width,
        b.height.saturating_sub(TOOLBAR),
    )
}
pub fn back(b: Rect) -> Rect {
    Rect::new(b.x + 3, b.y + 4, 27, 27)
}
pub fn forward(b: Rect) -> Rect {
    Rect::new(b.x + 33, b.y + 4, 27, 27)
}
pub fn search(b: Rect) -> Rect {
    let w = (b.width / 3).clamp(100, 220);
    Rect::new(b.x + b.width as i32 - w as i32 - 6, b.y + 6, w, 23)
}
pub fn address(b: Rect) -> Rect {
    Rect::new(
        b.x + 74,
        b.y + 6,
        b.width.saturating_sub(search(b).width + 114),
        23,
    )
}
/// Geometry is measured from the active font and shared by painting and input.
pub fn breadcrumbs(
    b: Rect,
    kind: WindowKind,
    font: &crate::graphics::font::FontLib,
) -> Vec<(Rect, &'static str, WindowKind)> {
    let mut kinds = alloc::vec![WindowKind::ControlPanel];
    match kind {
        WindowKind::ControlPanel => {}
        WindowKind::DisplaySettings => {
            kinds.extend([WindowKind::ControlPage(Page::TextSize), kind])
        }
        WindowKind::ControlPage(Page::DisplayAdvanced | Page::ResolutionHelp) => {
            kinds.extend([WindowKind::DisplaySettings, kind])
        }
        WindowKind::ControlPage(Page::Properties(_)) => kinds.extend([WindowKind::System, kind]),
        WindowKind::ControlCategory(_) | WindowKind::ControlPage(Page::AllItems) => {
            kinds.push(kind)
        }
        _ => kinds.extend([WindowKind::ControlPage(Page::AllItems), kind]),
    }
    let a = address(b);
    let available = a.width.saturating_sub(28);
    let measure = |k| font.measure(title(k), 12).map_or(0, |m| m.width) + 18;
    while kinds.len() > 2 && kinds.iter().map(|k| measure(*k)).sum::<u32>() > available {
        kinds.remove(1);
    }
    let mut x = a.x + 24;
    kinds
        .into_iter()
        .map(|k| {
            let width = measure(k).min((a.x + a.width as i32 - x).max(0) as u32);
            let r = Rect::new(x, a.y, width, a.height);
            x += width as i32;
            (r, title(k), k)
        })
        .collect()
}
pub fn view_rect(b: Rect) -> Rect {
    Rect::new(b.x + b.width as i32 - 150, b.y + 17, 132, 24)
}
pub fn category_rect(b: Rect, index: usize, font: &crate::graphics::font::FontLib) -> Rect {
    category_layout(b, index, font).0
}
pub fn category_layout(
    b: Rect,
    index: usize,
    font: &crate::graphics::font::FontLib,
) -> (Rect, Rect, Vec<Rect>) {
    let left = (b.width / 10).clamp(24, 72);
    let col = b.width.saturating_sub(left * 2) / 2;
    let width = col.saturating_sub(55);
    let height = |i: usize| {
        paint::wrapped_lines(font, CATEGORIES[i], width, 16) * 21
            + 3
            + SHORTCUTS[i]
                .iter()
                .map(|l| paint::wrapped_lines(font, l.text, width, 12) * 18)
                .sum::<u32>()
    };
    let y = b.y
        + 76
        + (index / 4 * 4..index)
            .map(|i| height(i).max(48) + 14)
            .sum::<u32>() as i32;
    let r = Rect::new(
        b.x + left as i32 + (index / 4) as i32 * col as i32,
        y,
        col,
        height(index).max(48),
    );
    let title = Rect::new(
        r.x + 55,
        r.y,
        width,
        paint::wrapped_lines(font, CATEGORIES[index], width, 16) * 21,
    );
    let mut y = title.y + title.height as i32 + 3;
    let links = SHORTCUTS[index]
        .iter()
        .map(|l| {
            let h = paint::wrapped_lines(font, l.text, width, 12) * 18;
            let link = Rect::new(title.x, y, width, h);
            y += h as i32;
            link
        })
        .collect();
    (r, title, links)
}
pub const CATEGORIES: [&str; 8] = [
    strings::SYSTEM_AND_SECURITY,
    strings::NETWORK_AND_INTERNET,
    strings::HARDWARE_AND_SOUND,
    strings::PROGRAMS,
    strings::USER_ACCOUNTS_AND_FAMILY_SAFETY,
    strings::APPEARANCE_AND_PERSONALIZATION,
    strings::CLOCK_LANGUAGE_AND_REGION,
    strings::EASE_OF_ACCESS,
];
#[derive(Clone, Copy)]
pub struct Link {
    pub text: &'static str,
    pub kind: WindowKind,
}
macro_rules! link {
    ($s:expr, $k:expr) => {
        Link { text: $s, kind: $k }
    };
}
pub const SHORTCUTS: [&[Link]; 8] = [
    &[
        link!(strings::REVIEW_YOUR_COMPUTER_S_STATUS, WindowKind::System),
        link!(
            strings::BACK_UP_YOUR_COMPUTER,
            WindowKind::ControlCategory(0)
        ),
        link!(
            strings::FIND_AND_FIX_PROBLEMS,
            WindowKind::ControlCategory(0)
        ),
    ],
    &[
        link!(
            strings::VIEW_NETWORK_STATUS_AND_TASKS,
            WindowKind::ShellSettings(Flyout::Network)
        ),
        link!(
            strings::CHOOSE_HOMEGROUP_AND_SHARING_OPTIONS,
            WindowKind::ShellSettings(Flyout::Network)
        ),
    ],
    &[
        link!(
            strings::VIEW_DEVICES_AND_PRINTERS,
            WindowKind::Application(AppId::DeviceManager)
        ),
        link!(
            strings::ADD_A_DEVICE,
            WindowKind::Application(AppId::DeviceManager)
        ),
        link!(strings::CONNECT_TO_A_PROJECTOR, WindowKind::DisplaySettings),
        link!(
            strings::ADJUST_COMMONLY_USED_MOBILITY_SETTINGS,
            WindowKind::ControlCategory(2)
        ),
    ],
    &[link!(
        strings::UNINSTALL_A_PROGRAM,
        WindowKind::Application(AppId::DefaultPrograms)
    )],
    &[
        link!(
            strings::ADD_OR_REMOVE_USER_ACCOUNTS,
            WindowKind::UserAccounts
        ),
        link!(
            strings::SET_UP_PARENTAL_CONTROLS_FOR_ANY_USER,
            WindowKind::UserAccounts
        ),
    ],
    &[
        link!(strings::CHANGE_THE_THEME, WindowKind::Personalization),
        link!(
            strings::CHANGE_DESKTOP_BACKGROUND,
            WindowKind::Personalization
        ),
        link!(
            strings::ADJUST_SCREEN_RESOLUTION,
            WindowKind::DisplaySettings
        ),
    ],
    &[
        link!(
            strings::CHANGE_KEYBOARDS_OR_OTHER_INPUT_METHODS,
            WindowKind::ControlCategory(6)
        ),
        link!(
            strings::CHANGE_DISPLAY_LANGUAGE,
            WindowKind::ControlCategory(6)
        ),
    ],
    &[
        link!(
            strings::LET_WINDOWS_SUGGEST_SETTINGS,
            WindowKind::Application(AppId::EaseOfAccessCenter)
        ),
        link!(
            strings::OPTIMIZE_VISUAL_DISPLAY,
            WindowKind::ControlPage(Page::TextSize)
        ),
    ],
];
pub const ITEMS: &[Link] = &[
    link!(strings::SYSTEM, WindowKind::System),
    link!(strings::SCREEN_RESOLUTION, WindowKind::DisplaySettings),
    link!(strings::DISPLAY, WindowKind::ControlPage(Page::TextSize)),
    link!(strings::PERSONALIZATION, WindowKind::Personalization),
    link!(strings::USER_ACCOUNTS, WindowKind::UserAccounts),
    link!(strings::SOUND, WindowKind::SoundSettings),
    link!(
        strings::DEVICE_MANAGER,
        WindowKind::Application(AppId::DeviceManager)
    ),
    link!(
        strings::NETWORK_AND_SHARING_CENTER,
        WindowKind::ShellSettings(Flyout::Network)
    ),
    link!(
        strings::DEFAULT_PROGRAMS,
        WindowKind::Application(AppId::DefaultPrograms)
    ),
    link!(
        strings::EASE_OF_ACCESS_CENTER,
        WindowKind::Application(AppId::EaseOfAccessCenter)
    ),
    link!(
        strings::DATE_AND_TIME,
        WindowKind::ShellSettings(Flyout::Clock)
    ),
    link!(
        strings::ADMINISTRATIVE_TOOLS,
        WindowKind::ComputerManagement
    ),
];
pub fn title(kind: WindowKind) -> &'static str {
    match kind {
        WindowKind::ControlPanel => strings::CONTROL_PANEL,
        WindowKind::ControlCategory(i) => CATEGORIES
            .get(i as usize)
            .copied()
            .unwrap_or(strings::CONTROL_PANEL),
        WindowKind::System => strings::SYSTEM,
        WindowKind::DisplaySettings => strings::SCREEN_RESOLUTION,
        WindowKind::ControlPage(Page::AllItems) => strings::ALL_CONTROL_PANEL_ITEMS,
        WindowKind::ControlPage(Page::Properties(_)) => strings::SYSTEM_PROPERTIES,
        WindowKind::ControlPage(Page::DisplayAdvanced) => strings::DISPLAY_ADAPTER_PROPERTIES,
        WindowKind::ControlPage(Page::TextSize) => strings::DISPLAY,
        WindowKind::ControlPage(Page::ResolutionHelp) => strings::CHOOSING_DISPLAY_SETTINGS,
        WindowKind::UserAccounts => strings::USER_ACCOUNTS,
        WindowKind::SoundSettings => strings::SOUND,
        WindowKind::Personalization => strings::PERSONALIZATION,
        WindowKind::ShellSettings(Flyout::Network) => strings::NETWORK_AND_SHARING_CENTER,
        WindowKind::ComputerManagement => strings::ADMINISTRATIVE_TOOLS,
        WindowKind::ShellSettings(Flyout::Clock) => strings::DATE_AND_TIME,
        WindowKind::Application(app) => app.application().title,
        _ => strings::CONTROL_PANEL,
    }
}

impl ShellModel {
    pub(crate) fn refresh_control_system(&mut self) {
        if let Ok(snapshot) = crate::platform::services::processes() {
            self.process_snapshot = Some(snapshot);
        }
        if let Ok(sample) = crate::platform::services::monitor() {
            self.monitor_sample = Some(sample);
        }
    }
    pub fn control_browser(&self, id: u32) -> Option<&Browser> {
        self.control_browsers.iter().find(|b| b.window_id == id)
    }
    pub fn control_browser_mut(&mut self, id: u32) -> Option<&mut Browser> {
        self.control_browsers.iter_mut().find(|b| b.window_id == id)
    }
    pub fn navigate_control(&mut self, id: u32, destination: WindowKind) -> bool {
        if self.control_browser(id).is_none() {
            return false;
        }
        // A pending display transition is reverted before leaving its page.
        if self
            .taskbar
            .windows
            .iter()
            .any(|w| w.id == id && w.kind == WindowKind::DisplaySettings)
        {
            self.display.open = false;
            self.revert_display();
            if self.display.previous.is_some() {
                return true;
            }
        }
        if destination == WindowKind::DisplaySettings {
            self.load_display_modes();
            self.display.close_requested = false;
        }
        if destination == WindowKind::UserAccounts {
            self.account_settings.refresh();
        }
        if destination == WindowKind::SoundSettings {
            self.refresh_audio();
        }
        if destination == WindowKind::Personalization {
            self.personalization.open(&mut self.files);
        }
        if destination == WindowKind::ShellSettings(Flyout::Network) {
            self.network_settings.refresh();
        }
        if let WindowKind::Application(app) = destination {
            self.applications.retain(|a| a.window_id != id);
            let mut session = ApplicationSession::new(id, app, id as u64 * 7919);
            session.notepad.fonts = self.fonts.clone();
            if let crate::applications::AppState::DeviceManager(m) = &mut session.state {
                m.refresh();
            }
            self.applications.push(session);
        } else {
            self.applications.retain(|a| a.window_id != id);
        }
        if let Some(browser) = self.control_browser_mut(id) {
            browser.visit(title(destination), destination);
        }
        self.sync_control_visit(id);
        true
    }
    fn sync_control_visit(&mut self, id: u32) {
        let Some(visit) = self.control_browser(id).map(|b| b.current().clone()) else {
            return;
        };
        if visit.kind == WindowKind::System {
            self.refresh_control_system();
        }
        if let Some(window) = self.taskbar.windows.iter_mut().find(|w| w.id == id) {
            window.kind = visit.kind;
            window.title = visit.title;
        }
        self.select_window(id);
    }
    pub fn control_history(&mut self, id: u32, forward: bool) -> bool {
        let Some(browser) = self.control_browser(id) else {
            return false;
        };
        if if forward {
            !browser.can_forward()
        } else {
            !browser.can_back()
        } {
            return true;
        }
        if browser.current().kind == WindowKind::DisplaySettings {
            self.display.open = false;
            self.revert_display();
            if self.display.previous.is_some() {
                return true;
            }
        }
        let Some(browser) = self.control_browser_mut(id) else {
            return false;
        };
        if forward {
            browser.cursor += 1;
        } else {
            browser.cursor -= 1;
        }
        browser.view_open = false;
        browser.search_focus = false;
        browser.message.clear();
        let kind = browser.current().kind;
        // Rebind an embedded app when crossing a history boundary.
        self.applications.retain(|a| a.window_id != id);
        if let WindowKind::Application(app) = kind {
            let mut session = ApplicationSession::new(id, app, id as u64 * 7919);
            if let crate::applications::AppState::DeviceManager(m) = &mut session.state {
                m.refresh();
            }
            self.applications.push(session);
        }
        match kind {
            WindowKind::DisplaySettings => self.load_display_modes(),
            WindowKind::UserAccounts => self.account_settings.refresh(),
            WindowKind::SoundSettings => self.refresh_audio(),
            WindowKind::Personalization => self.personalization.open(&mut self.files),
            WindowKind::ShellSettings(Flyout::Network) => self.network_settings.refresh(),
            _ => {}
        }
        self.sync_control_visit(id);
        true
    }
    pub fn control_scroll(&mut self, id: u32, delta: i32) -> bool {
        let Some(window) = self.taskbar.windows.iter().find(|w| w.id == id) else {
            return false;
        };
        let b = content(body(window));
        let Ok(font) = self.fonts.get(paint::FONT) else {
            return false;
        };
        let Some(browser) = self.control_browser_mut(id) else {
            return false;
        };
        let height = visit_height(browser.current(), b.width, &font);
        let max = height.saturating_sub(b.height);
        browser.current_mut().scroll = browser
            .current()
            .scroll
            .saturating_add_signed(delta)
            .min(max);
        true
    }
    pub fn control_key(&mut self, id: u32, key: u8) -> bool {
        use crate::applications::explorer::input;
        let Some(browser) = self.control_browser(id) else {
            return false;
        };
        if key == input::BACK {
            return self.control_history(id, false);
        }
        if key == input::FORWARD {
            return self.control_history(id, true);
        }
        if browser.search_focus {
            let Some(browser) = self.control_browser_mut(id) else {
                return false;
            };
            match key {
                27 => browser.search_focus = false,
                8 => {
                    browser.current_mut().query.pop();
                }
                13 | 10 => {
                    browser.search_focus = false;
                }
                0x20..=0x7e if browser.current().query.len() < 100 => {
                    browser.current_mut().query.push(key as char);
                    browser.current_mut().scroll = 0;
                }
                _ => {}
            }
            return true;
        }
        if matches!(
            browser.current().kind,
            WindowKind::ControlPage(Page::Properties(_))
        ) && key == 27
        {
            if browser.can_back() {
                return self.control_history(id, false);
            }
            self.close_window(id);
            return true;
        }
        if browser.current().kind == WindowKind::DisplaySettings {
            let handled = self.display_key(key);
            if self.display.close_requested {
                self.display.close_requested = false;
                self.close_window(id);
            }
            return handled;
        }
        if matches!(
            browser.current().kind,
            WindowKind::Application(_)
                | WindowKind::UserAccounts
                | WindowKind::SoundSettings
                | WindowKind::Personalization
                | WindowKind::ShellSettings(Flyout::Network)
        ) {
            return false;
        }
        match key {
            input::PAGE_DOWN | input::DOWN => self.control_scroll(id, 80),
            input::PAGE_UP | input::UP => self.control_scroll(id, -80),
            8 => self.control_history(id, false),
            _ => false,
        }
    }
    pub fn control_click(&mut self, id: u32, b: Rect, x: i32, y: i32) -> bool {
        let Some(browser) = self.control_browser(id) else {
            return false;
        };
        if !b.contains(x, y) {
            return false;
        }
        if back(b).contains(x, y) {
            return self.control_history(id, false);
        }
        if forward(b).contains(x, y) {
            return self.control_history(id, true);
        }
        if search(b).contains(x, y) {
            if let Some(browser) = self.control_browser_mut(id) {
                browser.search_focus = true;
            }
            return true;
        }
        let a = address(b);
        if y < b.y + TOOLBAR as i32 {
            if let Ok(font) = self.fonts.get(paint::FONT) {
                for (r, _, destination) in breadcrumbs(b, browser.current().kind, &font) {
                    if r.contains(x, y) {
                        return self.navigate_control(id, destination);
                    }
                }
            }
            if x >= a.x + a.width as i32 {
                if kind_is_display(browser.current().kind) {
                    self.load_display_modes();
                }
                return true;
            }
            return true;
        }
        let b = content(b);
        let kind = browser.current().kind;
        let scroll = browser.current().scroll;
        let query = browser.current().query.clone();
        let view = browser.current().view;
        if let Some(browser) = self.control_browser_mut(id) {
            browser.search_focus = false;
        }
        // Scrollbar controls remain fixed while the page itself moves.
        let Ok(font) = self.fonts.get(paint::FONT) else {
            return false;
        };
        let height = self
            .control_browser(id)
            .map_or(0, |browser| visit_height(browser.current(), b.width, &font));
        if height > b.height && x >= b.x + b.width as i32 - 16 {
            let down = y >= b.y + b.height as i32 / 2;
            return self.control_scroll(id, if down { 72 } else { -72 });
        }
        let page = page_rect(b, height, scroll);
        if matches!(
            kind,
            WindowKind::ControlPanel | WindowKind::ControlPage(Page::AllItems)
        ) && query.is_empty()
        {
            if self.control_browser(id).is_some_and(|b| b.view_open) {
                let r = view_rect(b);
                if let Some(index) =
                    (0..3).find(|i| Rect::new(r.x, r.y + 25 + i * 26, r.width, 26).contains(x, y))
                {
                    if let Some(browser) = self.control_browser_mut(id) {
                        browser.current_mut().view = match index {
                            0 => View::Category,
                            1 => View::LargeIcons,
                            _ => View::SmallIcons,
                        };
                        browser.view_open = false;
                        browser.current_mut().scroll = 0;
                        if index == 0 {
                            browser.current_mut().kind = WindowKind::ControlPanel;
                            browser.current_mut().title = strings::CONTROL_PANEL.into();
                        }
                    }
                    self.sync_control_visit(id);
                    return true;
                }
                if let Some(browser) = self.control_browser_mut(id) {
                    browser.view_open = false;
                }
                return true;
            }
            if view_rect(b).contains(x, y) {
                if let Some(browser) = self.control_browser_mut(id) {
                    browser.view_open = true;
                }
                return true;
            }
        }
        if !query.is_empty()
            || kind == WindowKind::ControlPage(Page::AllItems)
            || kind == WindowKind::ControlPanel && view != View::Category
        {
            for (index, item) in filtered_items(&query).enumerate() {
                if item_rect(page, index, view).contains(x, y) {
                    return self.navigate_control(id, item.kind);
                }
            }
            return true;
        }
        match kind {
            WindowKind::ControlPanel => {
                for index in 0..8 {
                    let font = self.fonts.get(paint::FONT).unwrap();
                    let (r, title, links) = category_layout(page, index, &font);
                    if title.contains(x, y) || Rect::new(r.x, r.y, 48, 48).contains(x, y) {
                        return self.navigate_control(id, WindowKind::ControlCategory(index as u8));
                    }
                    for (rect, link) in links.iter().zip(SHORTCUTS[index]) {
                        if rect.contains(x, y) {
                            return self.navigate_control(id, link.kind);
                        }
                    }
                }
                true
            }
            WindowKind::ControlCategory(_) | WindowKind::ComputerManagement => {
                if Rect::new(page.x + 14, page.y + 12, 150, 24).contains(x, y) {
                    return self.navigate_control(id, WindowKind::ControlPanel);
                }
                if matches!(kind, WindowKind::ControlCategory(_)) {
                    for i in 0..8 {
                        if Rect::new(page.x + 14, page.y + 58 + i * 30, 156, 24).contains(x, y) {
                            return self.navigate_control(id, WindowKind::ControlCategory(i as u8));
                        }
                    }
                }
                for (i, (_, destination)) in ShellModel::namespace_links(kind).iter().enumerate() {
                    if category_item_rect(page, i).contains(x, y) {
                        return self.navigate_control(id, *destination);
                    }
                }
                true
            }
            WindowKind::System => {
                if crate::applications::explorer::system::home_link(b).contains(x, y) {
                    return self.navigate_control(id, WindowKind::ControlPanel);
                }
                for i in 0..4 {
                    if crate::applications::explorer::system::sidebar_link(b, i).contains(x, y) {
                        return self.navigate_control(
                            id,
                            match i {
                                0 => WindowKind::Application(AppId::DeviceManager),
                                1 => WindowKind::ControlPage(Page::Properties(4)),
                                2 => WindowKind::ControlPage(Page::Properties(3)),
                                _ => WindowKind::ControlPage(Page::Properties(2)),
                            },
                        );
                    }
                }
                if crate::applications::explorer::system::change_link(page).contains(x, y) {
                    return self.navigate_control(id, WindowKind::ControlPage(Page::Properties(0)));
                }
                true
            }
            WindowKind::DisplaySettings => {
                use crate::display_settings as display;
                if self.display.previous.is_some() || self.display.open {
                    self.display_click(page, x, y, self.display.now);
                    if self.display.close_requested {
                        self.display.close_requested = false;
                        self.close_window(id);
                    }
                    return true;
                }
                if display::advanced_link(page).contains(x, y) {
                    return self
                        .navigate_control(id, WindowKind::ControlPage(Page::DisplayAdvanced));
                }
                if display::text_size_link(page).contains(x, y) {
                    return self.navigate_control(id, WindowKind::ControlPage(Page::TextSize));
                }
                if display::help_link(page).contains(x, y) {
                    return self
                        .navigate_control(id, WindowKind::ControlPage(Page::ResolutionHelp));
                }
                let action = self.display_click(page, x, y, self.display.now);
                if action && self.display.close_requested {
                    self.display.close_requested = false;
                    self.close_window(id);
                }
                true
            }
            WindowKind::ControlPage(Page::Properties(tab)) => {
                for i in 0..5 {
                    if paint::property_tab(page, i).contains(x, y) {
                        if let Some(browser) = self.control_browser_mut(id) {
                            browser.current_mut().kind =
                                WindowKind::ControlPage(Page::Properties(i as u8));
                        }
                        self.sync_control_visit(id);
                        return true;
                    }
                }
                if tab == 1 && paint::property_device_link(page).contains(x, y) {
                    return self
                        .navigate_control(id, WindowKind::Application(AppId::DeviceManager));
                }
                if paint::property_ok(page).contains(x, y)
                    || paint::property_cancel(page).contains(x, y)
                {
                    if self.control_browser(id).is_some_and(|b| b.can_back()) {
                        return self.control_history(id, false);
                    }
                    self.close_window(id);
                    return true;
                }
                true
            }
            WindowKind::ControlPage(
                Page::DisplayAdvanced | Page::TextSize | Page::ResolutionHelp,
            ) => {
                if paint::return_link(page).contains(x, y) {
                    return self.navigate_control(id, WindowKind::DisplaySettings);
                }
                true
            }
            _ => false,
        }
    }
}
pub fn category_item_rect(b: Rect, index: usize) -> Rect {
    Rect::new(
        b.x + 196,
        b.y + 70 + index as i32 * 65,
        b.width.saturating_sub(222),
        56,
    )
}
pub fn item_rect(b: Rect, index: usize, view: View) -> Rect {
    let columns = if b.width >= 600 { 3 } else { 2 };
    let h = if view == View::SmallIcons { 36 } else { 68 };
    let w = b.width.saturating_sub(48) / columns;
    Rect::new(
        b.x + 24 + (index as u32 % columns * w) as i32,
        b.y + 66 + (index as u32 / columns * h) as i32,
        w,
        h - 4,
    )
}
pub fn filtered_items(query: &str) -> impl Iterator<Item = &'static Link> + '_ {
    let query = query.to_lowercase();
    ITEMS.iter().filter(move |item| {
        query.is_empty()
            || item.text.to_lowercase().contains(&query)
            || title(item.kind).to_lowercase().contains(&query)
            || match item.kind {
                WindowKind::System => "system computer".contains(&query),
                WindowKind::DisplaySettings | WindowKind::ControlPage(Page::TextSize) => {
                    "display resolution".contains(&query)
                }
                _ => false,
            }
    })
}

fn kind_is_display(kind: WindowKind) -> bool {
    matches!(
        kind,
        WindowKind::DisplaySettings | WindowKind::ControlPage(Page::DisplayAdvanced)
    )
}
pub fn page_height(kind: WindowKind) -> u32 {
    match kind {
        WindowKind::System => 550,
        WindowKind::ControlPanel => 392,
        WindowKind::DisplaySettings => 376,
        WindowKind::ControlCategory(_) | WindowKind::ComputerManagement => {
            (90 + ShellModel::namespace_links(kind).len() as u32 * 65).max(326)
        }
        _ => 480,
    }
}
pub fn visit_height(visit: &Visit, width: u32, font: &crate::graphics::font::FontLib) -> u32 {
    if !visit.query.is_empty()
        || visit.kind == WindowKind::ControlPage(Page::AllItems)
        || visit.kind == WindowKind::ControlPanel && visit.view != View::Category
    {
        let columns = if width >= 600 { 3 } else { 2 };
        return 76
            + (filtered_items(&visit.query).count() as u32).div_ceil(columns)
                * if visit.view == View::SmallIcons {
                    36
                } else {
                    68
                };
    }
    if visit.kind == WindowKind::ControlPanel {
        let b = Rect::new(0, 0, width.saturating_sub(16), 0);
        return [3, 7]
            .iter()
            .map(|i| {
                let r = category_layout(b, *i, &font).0;
                r.y as u32 + r.height + 12
            })
            .max()
            .unwrap_or(392);
    }
    // Existing interactive applets own their scrolling and should receive their full client area.
    if matches!(
        visit.kind,
        WindowKind::Application(_)
            | WindowKind::UserAccounts
            | WindowKind::SoundSettings
            | WindowKind::Personalization
            | WindowKind::ShellSettings(_)
    ) {
        0
    } else {
        page_height(visit.kind)
    }
}
pub fn page_rect(b: Rect, height: u32, scroll: u32) -> Rect {
    let scroll = scroll.min(height.saturating_sub(b.height));
    Rect::new(
        b.x,
        b.y - scroll as i32,
        b.width
            .saturating_sub(if height > b.height { 16 } else { 0 }),
        b.height.max(height),
    )
}
