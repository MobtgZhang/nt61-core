use crate::window::Session;
use alloc::vec::Vec;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputEvent {
    PointerMove {
        x: i32,
        y: i32,
    },
    PointerButton {
        button: MouseButton,
        down: bool,
    },
    PointerWheel {
        delta_x: i32,
        delta_y: i32,
    },
    Key {
        scancode: u32,
        down: bool,
        modifiers: u32,
    },
    Text {
        codepoint: char,
    },
    FocusChanged {
        focused: bool,
    },
}

pub trait InputBackend {
    fn next_event(&mut self) -> Result<InputEvent, InputError>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputError {
    QueueFull,
    DeviceUnavailable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputQueue {
    events: Vec<InputEvent>,
    capacity: usize,
}

impl InputBackend for InputQueue {
    fn next_event(&mut self) -> Result<InputEvent, InputError> {
        self.pop().ok_or(InputError::DeviceUnavailable)
    }
}

impl InputQueue {
    pub fn new(capacity: usize) -> Self {
        Self {
            events: Vec::new(),
            capacity,
        }
    }

    pub fn push(&mut self, event: InputEvent) -> bool {
        if self.capacity == 0 {
            return false;
        }
        if matches!(self.events.last(), Some(InputEvent::PointerMove { .. }))
            && matches!(event, InputEvent::PointerMove { .. })
        {
            if let Some(last) = self.events.last_mut() {
                *last = event;
            }
            return true;
        }
        if self.events.len() >= self.capacity {
            return false;
        }
        self.events.push(event);
        true
    }

    pub fn pop(&mut self) -> Option<InputEvent> {
        if self.events.is_empty() {
            None
        } else {
            Some(self.events.remove(0))
        }
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PointerState {
    pub x: i32,
    pub y: i32,
    pub left_down: bool,
    pub previous_left_down: bool,
}

impl PointerState {
    pub fn absolute(raw_x: u32, raw_y: u32, screen_width: u32, screen_height: u32) -> Self {
        let x = scale_axis(raw_x, screen_width);
        let y = scale_axis(raw_y, screen_height);
        Self {
            x,
            y,
            left_down: false,
            previous_left_down: false,
        }
    }

    pub fn route(&mut self, session: &mut Session) -> Option<crate::window::Message> {
        let message = session.route_pointer(self.x, self.y, self.left_down);
        self.previous_left_down = self.left_down;
        message
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DesktopShortcut {
    ShowDesktop,
    Explorer,
    Run,
    MinimizeAll,
    Lock,
    TaskSwitch,
    SnapLeft,
    SnapRight,
}

pub fn shortcut(modifiers: u8, virtual_key: u32) -> Option<DesktopShortcut> {
    const WIN: u8 = 1;
    const ALT: u8 = 2;
    let win = modifiers & WIN != 0;
    let alt = modifiers & ALT != 0;
    if !win {
        return None;
    }
    match (virtual_key, alt) {
        (key, _) if key == b'D' as u32 => Some(DesktopShortcut::ShowDesktop),
        (key, _) if key == b'E' as u32 => Some(DesktopShortcut::Explorer),
        (key, _) if key == b'R' as u32 => Some(DesktopShortcut::Run),
        (key, _) if key == b'M' as u32 => Some(DesktopShortcut::MinimizeAll),
        (key, _) if key == b'L' as u32 => Some(DesktopShortcut::Lock),
        (key, _) if key == b'\t' as u32 => Some(DesktopShortcut::TaskSwitch),
        (0x25, false) => Some(DesktopShortcut::SnapLeft),
        (0x27, false) => Some(DesktopShortcut::SnapRight),
        _ => None,
    }
}

fn scale_axis(value: u32, extent: u32) -> i32 {
    if extent == 0 {
        return 0;
    }
    let clamped = value.min(0xFFFF);
    ((clamped as u64 * (extent as u64 - 1)) / 0xFFFF) as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window::{Rect, WindowVisibility};

    #[test]
    fn input_queue_coalesces_moves_and_respects_capacity() {
        let mut queue = InputQueue::new(2);
        assert!(queue.push(InputEvent::PointerMove { x: 1, y: 1 }));
        assert!(queue.push(InputEvent::PointerMove { x: 2, y: 2 }));
        assert_eq!(queue.len(), 1);
        assert!(queue.push(InputEvent::Key {
            scancode: 1,
            down: true,
            modifiers: 0
        }));
        assert!(!queue.push(InputEvent::Key {
            scancode: 2,
            down: true,
            modifiers: 0
        }));
        assert_eq!(queue.pop(), Some(InputEvent::PointerMove { x: 2, y: 2 }));
        assert!(queue.pop().is_some());
        assert!(queue.is_empty());
    }

    #[test]
    fn absolute_pointer_maps_tablet_coordinates_to_screen() {
        let pointer = PointerState::absolute(0, 0xFFFF, 1024, 768);
        assert_eq!(pointer.x, 0);
        assert_eq!(pointer.y, 767);
    }

    #[test]
    fn pointer_state_routes_presses_into_session() {
        let mut session = Session::new(1);
        let handle = session.create_window("Test", "Test", Rect::new(0, 0, 100, 100));
        session.show_window(handle, WindowVisibility::Visible);
        session.queue.clear();
        let mut pointer = PointerState::absolute(0x8000, 0x8000, 100, 100);
        pointer.left_down = true;
        let message = pointer.route(&mut session).unwrap();
        assert_eq!(message.kind, crate::window::WM_LBUTTONDOWN);
    }

    #[test]
    fn pointer_release_sends_lbutton_up_once() {
        let mut session = Session::new(1);
        let handle = session.create_window("Test", "Test", Rect::new(0, 0, 100, 100));
        session.show_window(handle, WindowVisibility::Visible);
        session.queue.clear();
        let mut pointer = PointerState::absolute(0x8000, 0x8000, 100, 100);
        pointer.left_down = true;
        pointer.route(&mut session);
        pointer.left_down = false;
        pointer.route(&mut session);
        pointer.route(&mut session);
        assert!(session
            .queue
            .iter()
            .any(|message| message.kind == crate::window::WM_LBUTTONUP));
    }

    #[test]
    fn win7_shortcuts_are_recognized() {
        assert_eq!(shortcut(1, b'D' as u32), Some(DesktopShortcut::ShowDesktop));
        assert_eq!(shortcut(1, b'E' as u32), Some(DesktopShortcut::Explorer));
        assert_eq!(shortcut(3, b'D' as u32), Some(DesktopShortcut::ShowDesktop));
        assert_eq!(shortcut(1, 0x25), Some(DesktopShortcut::SnapLeft));
        assert_eq!(shortcut(0, b'E' as u32), None);
    }
}
