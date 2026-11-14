use alloc::string::String;
use alloc::vec::Vec;

pub const HTCLIENT: u32 = 1;
pub const HTCAPTION: u32 = 2;
pub const HTLEFT: u32 = 10;
pub const HTRIGHT: u32 = 11;
pub const HTTOP: u32 = 12;
pub const HTTOPLEFT: u32 = 13;
pub const HTTOPRIGHT: u32 = 14;
pub const HTBOTTOM: u32 = 15;
pub const HTBOTTOMLEFT: u32 = 16;
pub const HTBOTTOMRIGHT: u32 = 17;
pub const HTCLOSE: u32 = 20;

pub const WM_NCCREATE: u32 = 0x0081;
pub const WM_NCCALCSIZE: u32 = 0x0083;
pub const WM_CREATE: u32 = 0x0001;
pub const WM_PAINT: u32 = 0x000F;
pub const WM_CLOSE: u32 = 0x0010;
pub const WM_DESTROY: u32 = 0x0002;
pub const WM_QUIT: u32 = 0x0012;
pub const WM_ACTIVATE: u32 = 0x0006;
pub const WM_SIZE: u32 = 0x0005;
pub const WM_MOVE: u32 = 0x0003;
pub const WM_MOUSEMOVE: u32 = 0x0200;
pub const WM_LBUTTONDOWN: u32 = 0x0201;
pub const WM_LBUTTONUP: u32 = 0x0202;
pub const WM_KEYDOWN: u32 = 0x0100;
pub const WM_KEYUP: u32 = 0x0101;
pub const WM_CHAR: u32 = 0x0102;
pub const WM_SYSCOMMAND: u32 = 0x0112;

pub const SC_CLOSE: u32 = 0xF060;
pub const SC_MINIMIZE: u32 = 0xF020;
pub const SC_MAXIMIZE: u32 = 0xF030;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowHandle(pub u32);

impl WindowHandle {
    pub const NONE: WindowHandle = WindowHandle(0);

    pub fn is_valid(self) -> bool {
        self.0 != 0 && self.0 & 0xFF00_0000 == 0x0100_0000
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Message {
    pub window: WindowHandle,
    pub kind: u32,
    pub wparam: usize,
    pub lparam: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowVisibility {
    Hidden,
    Visible,
    Minimized,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Window {
    pub handle: WindowHandle,
    pub class: String,
    pub title: String,
    pub rect: Rect,
    pub visible: WindowVisibility,
    pub enabled: bool,
    pub focused: bool,
    pub z_order: u32,
    pub owner: WindowHandle,
    pub maximized: bool,
    pub pre_maximize: Option<Rect>,
}

impl Window {
    pub fn is_visible(&self) -> bool {
        self.visible == WindowVisibility::Visible
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x
            && x < self.x.saturating_add(self.width as i32)
            && y >= self.y
            && y < self.y.saturating_add(self.height as i32)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    pub id: u32,
    pub station: String,
    pub desktop: String,
    pub windows: Vec<Window>,
    pub queue: Vec<Message>,
    pub focus: WindowHandle,
    pub foreground: WindowHandle,
    pub capture: WindowHandle,
    pub pointer: (i32, i32),
    pub pointer_down: bool,
    next_handle: u32,
    drag: Option<DragState>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DragState {
    pub window: WindowHandle,
    pub origin: (i32, i32),
    pub rect: Rect,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DesktopSecurity {
    Default,
    Winlogon,
    ScreenSaver,
}

impl Session {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            station: String::from("WinSta0"),
            desktop: String::from("Default"),
            windows: Vec::new(),
            queue: Vec::new(),
            focus: WindowHandle::NONE,
            foreground: WindowHandle::NONE,
            capture: WindowHandle::NONE,
            pointer: (0, 0),
            pointer_down: false,
            next_handle: 1,
            drag: None,
        }
    }

    pub fn create_window(&mut self, class: &str, title: &str, rect: Rect) -> WindowHandle {
        let index = self.next_handle.min(0xFFFF);
        self.next_handle += 1;
        let handle = WindowHandle(0x0100_0000 | index);
        let next_z = self.windows.last().map_or(1, |window| window.z_order + 1);
        let window = Window {
            handle,
            class: String::from(class),
            title: String::from(title),
            rect,
            visible: WindowVisibility::Hidden,
            enabled: true,
            focused: false,
            z_order: next_z,
            owner: WindowHandle::NONE,
            maximized: false,
            pre_maximize: None,
        };
        self.windows.push(window);
        self.post(handle, WM_NCCREATE, 0, 0);
        self.post(handle, WM_NCCALCSIZE, 0, 0);
        self.post(handle, WM_CREATE, 0, 0);
        handle
    }

    pub fn post(&mut self, window: WindowHandle, kind: u32, wparam: usize, lparam: usize) {
        if self.window(window).is_some() {
            self.queue.push(Message {
                window,
                kind,
                wparam,
                lparam,
            });
        }
    }

    pub fn window(&self, handle: WindowHandle) -> Option<&Window> {
        self.windows.iter().find(|window| window.handle == handle)
    }

    pub fn window_mut(&mut self, handle: WindowHandle) -> Option<&mut Window> {
        self.windows
            .iter_mut()
            .find(|window| window.handle == handle)
    }

    pub fn show_window(&mut self, handle: WindowHandle, visibility: WindowVisibility) {
        if let Some(window) = self.window_mut(handle) {
            window.visible = visibility;
        }
        if visibility == WindowVisibility::Visible {
            self.set_foreground(handle);
        }
    }

    pub fn minimize_window(&mut self, handle: WindowHandle) -> bool {
        let Some(window) = self.window_mut(handle) else {
            return false;
        };
        window.visible = WindowVisibility::Minimized;
        true
    }

    pub fn maximize_window(&mut self, handle: WindowHandle, bounds: Rect) -> bool {
        let Some(window) = self.window_mut(handle) else {
            return false;
        };
        if !window.maximized {
            window.pre_maximize = Some(window.rect);
        }
        window.rect = bounds;
        window.maximized = true;
        window.visible = WindowVisibility::Visible;
        self.set_foreground(handle);
        true
    }

    pub fn restore_window(&mut self, handle: WindowHandle) -> bool {
        let Some(window) = self.window_mut(handle) else {
            return false;
        };
        if let Some(rect) = window.pre_maximize {
            window.rect = rect;
        }
        window.maximized = false;
        window.visible = WindowVisibility::Visible;
        self.set_foreground(handle);
        true
    }

    pub fn resize_window(&mut self, handle: WindowHandle, rect: Rect) -> bool {
        let Some(window) = self.window_mut(handle) else {
            return false;
        };
        if window.maximized || rect.width == 0 || rect.height == 0 {
            return false;
        }
        window.rect = rect;
        self.post(handle, WM_SIZE, 0, 0);
        self.post(handle, WM_MOVE, 0, 0);
        true
    }

    pub fn set_foreground(&mut self, handle: WindowHandle) {
        if self.window(handle).is_none() {
            return;
        }
        self.foreground = handle;
        self.focus = handle;
        for window in &mut self.windows {
            window.focused = window.handle == handle;
        }
        self.post(handle, WM_ACTIVATE, 1, 0);
    }

    pub fn destroy_window(&mut self, handle: WindowHandle) {
        if self.window(handle).is_none() {
            return;
        }
        self.post(handle, WM_DESTROY, 0, 0);
        self.windows.retain(|window| window.handle != handle);
        self.queue.retain(|message| message.window != handle);
        if self.focus == handle {
            self.focus = WindowHandle::NONE;
        }
        if self.foreground == handle {
            self.foreground = WindowHandle::NONE;
        }
        if self.capture == handle {
            self.capture = WindowHandle::NONE;
        }
        if self.drag.as_ref().map(|drag| drag.window) == Some(handle) {
            self.drag = None;
        }
    }

    pub fn topmost_at(&self, x: i32, y: i32) -> Option<WindowHandle> {
        self.windows
            .iter()
            .filter(|window| {
                window.visible == WindowVisibility::Visible && window.rect.contains(x, y)
            })
            .fold(None, |best: Option<&Window>, window| match best {
                Some(previous) if previous.z_order >= window.z_order => Some(previous),
                _ => Some(window),
            })
            .map(|window| window.handle)
    }

    pub fn hit_test(&self, handle: WindowHandle, x: i32, y: i32) -> u32 {
        let Some(window) = self.window(handle) else {
            return 0;
        };
        let border = 6i32;
        let relative_x = x - window.rect.x;
        let relative_y = y - window.rect.y;
        let on_left = relative_x < border;
        let on_right = relative_x >= window.rect.width as i32 - border;
        let on_top = relative_y < border;
        let on_bottom = relative_y >= window.rect.height as i32 - border;
        match (on_left, on_top, on_right, on_bottom) {
            (true, true, _, _) => HTTOPLEFT,
            (_, true, true, _) => HTTOPRIGHT,
            (true, _, _, true) => HTBOTTOMLEFT,
            (_, _, true, true) => HTBOTTOMRIGHT,
            (true, _, _, _) => HTLEFT,
            (_, _, true, _) => HTRIGHT,
            (_, true, _, _) => HTTOP,
            (_, _, _, true) => HTBOTTOM,
            (false, false, false, false) if relative_y < 30 => HTCAPTION,
            _ => HTCLIENT,
        }
    }

    pub fn route_pointer(&mut self, x: i32, y: i32, pressed: bool) -> Option<Message> {
        self.pointer = (x, y);
        let was_down = self.pointer_down;
        let handle = self
            .capture
            .is_valid()
            .then_some(self.capture)
            .or_else(|| self.topmost_at(x, y))?;
        let kind = if pressed {
            WM_LBUTTONDOWN
        } else if was_down {
            WM_LBUTTONUP
        } else {
            WM_MOUSEMOVE
        };
        let lparam = ((x as u32 as usize) << 16) | (y as u32 as usize);
        self.post(handle, kind, 0, lparam);
        if pressed {
            self.capture = handle;
            self.set_foreground(handle);
            if self.hit_test(handle, x, y) == HTCAPTION {
                self.drag = Some(DragState {
                    window: handle,
                    origin: (x, y),
                    rect: self.window(handle).map(|window| window.rect)?,
                });
            }
        } else if was_down {
            if let Some(drag) = self.drag.take() {
                let delta_x = x - drag.origin.0;
                let delta_y = y - drag.origin.1;
                if let Some(window) = self.window_mut(drag.window) {
                    window.rect.x = drag.rect.x + delta_x;
                    window.rect.y = drag.rect.y + delta_y;
                }
            }
            self.capture = WindowHandle::NONE;
        }
        self.pointer_down = pressed;
        Some(Message {
            window: handle,
            kind,
            wparam: 0,
            lparam,
        })
    }

    pub fn route_key(&mut self, key: u32, pressed: bool) -> Option<Message> {
        let handle = if self.focus.is_valid() {
            self.focus
        } else {
            self.foreground
        };
        let handle = self.window(handle).map(|_| handle)?;
        let kind = if pressed { WM_KEYDOWN } else { WM_KEYUP };
        self.post(handle, kind, key as usize, 0);
        if pressed {
            self.post(handle, WM_CHAR, key as usize, 0);
        }
        Some(Message {
            window: handle,
            kind,
            wparam: key as usize,
            lparam: 0,
        })
    }

    pub fn pop_message(&mut self) -> Option<Message> {
        if self.queue.is_empty() {
            None
        } else {
            Some(self.queue.remove(0))
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionManager {
    sessions: Vec<Session>,
    active: u32,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            sessions: Vec::new(),
            active: 0,
        }
    }

    pub fn create_session(&mut self, secure: DesktopSecurity) -> u32 {
        let id = self.sessions.len() as u32 + 1;
        let mut session = Session::new(id);
        session.desktop = match secure {
            DesktopSecurity::Default => String::from("Default"),
            DesktopSecurity::Winlogon => String::from("Winlogon"),
            DesktopSecurity::ScreenSaver => String::from("ScreenSaver"),
        };
        self.sessions.push(session);
        self.active = id;
        id
    }

    pub fn active_session(&self) -> Option<&Session> {
        self.sessions
            .iter()
            .find(|session| session.id == self.active)
    }

    pub fn active_session_mut(&mut self) -> Option<&mut Session> {
        self.sessions
            .iter_mut()
            .find(|session| session.id == self.active)
    }

    pub fn switch_desktop(&mut self, id: u32, desktop: DesktopSecurity) -> bool {
        if !self.sessions.iter().any(|session| session.id == id) {
            return false;
        }
        self.active = id;
        if let Some(session) = self.active_session_mut() {
            session.desktop = match desktop {
                DesktopSecurity::Default => String::from("Default"),
                DesktopSecurity::Winlogon => String::from("Winlogon"),
                DesktopSecurity::ScreenSaver => String::from("ScreenSaver"),
            };
        }
        true
    }

    pub fn post_to(&mut self, session_id: u32, window: WindowHandle, message: Message) -> bool {
        if self.active != session_id {
            return false;
        }
        if self
            .active_session()
            .map(|session| session.window(window).is_none())
            .unwrap_or(true)
        {
            return false;
        }
        if let Some(session) = self.active_session_mut() {
            session.queue.push(message);
        }
        true
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_lifecycle_posts_nonclient_then_client_messages() {
        let mut session = Session::new(1);
        let handle = session.create_window("Test", "Test", Rect::new(10, 10, 200, 100));
        assert!(handle.is_valid());
        assert_eq!(session.queue.remove(0).kind, WM_NCCREATE);
        assert_eq!(session.queue.remove(0).kind, WM_NCCALCSIZE);
        assert_eq!(session.queue.remove(0).kind, WM_CREATE);
    }

    #[test]
    fn window_can_be_resized_maximized_and_restored() {
        let mut session = Session::new(1);
        let handle = session.create_window("Test", "Test", Rect::new(10, 10, 200, 100));
        session.show_window(handle, WindowVisibility::Visible);
        assert!(session.resize_window(handle, Rect::new(20, 20, 240, 120)));
        assert_eq!(
            session.window(handle).unwrap().rect,
            Rect::new(20, 20, 240, 120)
        );
        assert!(session.maximize_window(handle, Rect::new(0, 0, 1024, 728)));
        assert!(session.window(handle).unwrap().maximized);
        assert!(session.restore_window(handle));
        assert_eq!(
            session.window(handle).unwrap().rect,
            Rect::new(20, 20, 240, 120)
        );
        assert!(!session.window(handle).unwrap().maximized);
    }

    #[test]
    fn hit_test_distinguishes_caption_edges_and_client_area() {
        let mut session = Session::new(1);
        let handle = session.create_window("Test", "Test", Rect::new(100, 100, 300, 200));
        session.show_window(handle, WindowVisibility::Visible);
        assert_eq!(session.hit_test(handle, 100, 100), HTTOPLEFT);
        assert_eq!(session.hit_test(handle, 110, 110), HTCAPTION);
        assert_eq!(session.hit_test(handle, 200, 200), HTCLIENT);
        assert_eq!(session.hit_test(handle, 396, 296), HTBOTTOMRIGHT);
    }

    #[test]
    fn pointer_routes_to_topmost_and_cross_desktop_post_is_rejected() {
        let mut session = Session::new(1);
        let bottom = session.create_window("Bottom", "Bottom", Rect::new(0, 0, 300, 200));
        let top = session.create_window("Top", "Top", Rect::new(50, 50, 200, 100));
        session.show_window(bottom, WindowVisibility::Visible);
        session.show_window(top, WindowVisibility::Visible);
        session.queue.clear();
        assert_eq!(session.topmost_at(100, 100), Some(top));
        let message = session.route_pointer(100, 100, true).unwrap();
        assert_eq!(message.window, top);
        assert_eq!(message.kind, WM_LBUTTONDOWN);
        assert_eq!(session.pop_message().unwrap().window, top);
        session.destroy_window(top);
        assert!(
            !session.queue.iter().any(|message| message.window == top)
        );
    }

    #[test]
    fn keyboard_messages_go_to_focused_window() {
        let mut session = Session::new(2);
        let handle = session.create_window("Test", "Test", Rect::new(0, 0, 100, 100));
        session.show_window(handle, WindowVisibility::Visible);
        session.queue.clear();
        assert_eq!(
            session.route_key(b'A' as u32, true).unwrap().wparam,
            b'A' as u32 as usize
        );
        assert_eq!(session.pop_message().unwrap().kind, WM_KEYDOWN);
        assert_eq!(session.pop_message().unwrap().kind, WM_CHAR);
    }

    #[test]
    fn caption_drag_moves_window_and_release_clears_capture() {
        let mut session = Session::new(1);
        let handle = session.create_window("Test", "Test", Rect::new(10, 10, 200, 100));
        session.show_window(handle, WindowVisibility::Visible);
        session.queue.clear();
        session.route_pointer(50, 20, true);
        session.route_pointer(90, 50, false);
        let window = session.window(handle).unwrap();
        assert_eq!(window.rect.x, 50);
        assert_eq!(window.rect.y, 40);
        assert_eq!(session.capture, WindowHandle::NONE);
    }

    #[test]
    fn session_manager_enforces_active_desktop_boundaries() {
        let mut manager = SessionManager::new();
        let default = manager.create_session(DesktopSecurity::Default);
        let winlogon = manager.create_session(DesktopSecurity::Winlogon);
        assert_eq!(manager.active_session().unwrap().desktop, "Winlogon");
        assert!(manager.switch_desktop(default, DesktopSecurity::Default));
        assert_eq!(manager.active_session().unwrap().desktop, "Default");
        let mut secure = Session::new(winlogon);
        let secure_window = secure.create_window("LogonUI", "LogonUI", Rect::new(0, 0, 10, 10));
        let message = Message {
            window: secure_window,
            kind: WM_CREATE,
            wparam: 0,
            lparam: 0,
        };
        assert!(!manager.post_to(winlogon, secure_window, message));
        assert!(manager.switch_desktop(winlogon, DesktopSecurity::Winlogon));
        if let Some(session) = manager.active_session_mut() {
            let handle = session.create_window("LogonUI", "LogonUI", Rect::new(0, 0, 10, 10));
            assert!(handle.is_valid());
            let message = Message {
                window: handle,
                kind: WM_CREATE,
                wparam: 0,
                lparam: 0,
            };
            assert!(manager.post_to(winlogon, handle, message));
        }
    }
}
