//! Sound properties backed by the detected PCM device and per-user settings.
use crate::{
    compositor::aero,
    graphics::{Rect, Surface},
    platform::services,
};
use alloc::{format, string::String};
pub const EVENTS: &[(&str, &str)] = &[
    ("startup", "Windows Startup"),
    ("shutdown", "Windows Shutdown"),
    ("logon", "Windows Logon"),
    ("logoff", "Windows Logoff"),
    ("default", "Default Beep"),
    ("error", "Critical Stop"),
    ("exclamation", "Exclamation"),
    ("notify", "System Notification"),
    ("device-connect", "Device Connect"),
    ("device-disconnect", "Device Disconnect"),
    ("device-fail", "Device Failed to Connect"),
    ("navigate", "Start Navigation"),
    ("recycle", "Empty Recycle Bin"),
    ("print", "Print Complete"),
    ("uac", "Windows User Account Control"),
];
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub tab: usize,
    pub selected: usize,
    pub offset: usize,
    pub volume: u8,
    pub muted: bool,
    pub enabled: bool,
    pub startup: bool,
    pub ready: bool,
    pub message: String,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            tab: 0,
            selected: 0,
            offset: 0,
            volume: 75,
            muted: false,
            enabled: true,
            startup: true,
            ready: false,
            message: String::new(),
        }
    }
}
impl Settings {
    pub fn refresh(&mut self) {
        match services::sound_settings() {
            Ok(b)
                if b.len() == 9
                    && &b[..4] == b"SND1"
                    && b[4] <= 100
                    && b[5..].iter().all(|v| *v <= 1) =>
            {
                self.volume = b[4];
                self.muted = b[5] != 0;
                self.enabled = b[6] != 0;
                self.startup = b[7] != 0;
                self.ready = b[8] != 0;
                self.message.clear();
            }
            Ok(_) => self.message = "Invalid audio settings response.".into(),
            Err(code) => self.message = format!("Cannot load sound settings (0x{code:08X})."),
        }
    }
    fn change(&mut self, key: &str, value: u8) {
        match services::play_sound(&format!("{key}:{value}")) {
            Ok(()) => self.refresh(),
            Err(code) => self.message = format!("Cannot save sound settings (0x{code:08X})."),
        }
    }
    pub fn click(&mut self, b: Rect, x: i32, y: i32) -> bool {
        if !b.contains(x, y) {
            return false;
        }
        for i in 0..4 {
            if tab(b, i).contains(x, y) {
                self.tab = i;
                self.message.clear();
                return true;
            }
        }
        if self.tab == 0 {
            let r = slider(b);
            if r.contains(x, y) {
                self.change(
                    "volume",
                    ((x - r.x).clamp(0, r.width as i32) * 100 / r.width as i32) as u8,
                );
            }
            if mute(b).contains(x, y) {
                self.change("mute", u8::from(!self.muted));
            }
            if test(b).contains(x, y) {
                self.preview("default");
            }
        }
        if self.tab == 2 {
            for i in 0..2 {
                if scheme(b, i).contains(x, y) {
                    self.change("scheme", i as u8);
                    return true;
                }
            }
            let list = list(b);
            let capacity = capacity(b);
            if list.contains(x, y) {
                let row = ((y - list.y) / 24) as usize;
                if let Some(i) = (self.offset + row < EVENTS.len()).then_some(self.offset + row) {
                    self.selected = i;
                }
            }
            if scroll(b, false).contains(x, y) {
                self.offset = self.offset.saturating_sub(capacity);
            }
            if scroll(b, true).contains(x, y) {
                self.offset = (self.offset + capacity).min(EVENTS.len().saturating_sub(capacity));
            }
            if startup(b).contains(x, y) {
                self.change("startup", u8::from(!self.startup));
            }
            if test(b).contains(x, y) {
                self.preview(EVENTS[self.selected].0);
            }
        }
        true
    }
    fn preview(&mut self, event: &str) {
        if let Err(code) = services::play_sound(event) {
            self.message = format!("Cannot play this sound (0x{code:08X}).");
        }
    }
}
fn tab(b: Rect, i: usize) -> Rect {
    Rect::new(b.x + 12 + i as i32 * 130, b.y + 12, 130, 28)
}
fn scheme(b: Rect, i: usize) -> Rect {
    Rect::new(b.x + 24 + i as i32 * 230, b.y + 78, 220, 28)
}
fn list(b: Rect) -> Rect {
    Rect::new(
        b.x + 24,
        b.y + 140,
        b.width.saturating_sub(100),
        capacity(b) as u32 * 24,
    )
}
fn capacity(b: Rect) -> usize {
    (b.height.saturating_sub(260) / 24).max(1) as usize
}
fn startup(b: Rect) -> Rect {
    Rect::new(b.x + 24, b.y + b.height as i32 - 106, 360, 25)
}
fn test(b: Rect) -> Rect {
    Rect::new(
        b.x + b.width as i32 - 120,
        b.y + b.height as i32 - 106,
        90,
        26,
    )
}
fn scroll(b: Rect, next: bool) -> Rect {
    Rect::new(
        b.x + b.width as i32 - 65,
        b.y + 140 + if next { 60 } else { 0 },
        40,
        32,
    )
}
fn slider(b: Rect) -> Rect {
    Rect::new(b.x + 100, b.y + 190, b.width.saturating_sub(170), 28)
}
fn mute(b: Rect) -> Rect {
    Rect::new(b.x + 100, b.y + 235, 240, 28)
}
pub fn draw(s: &Settings, f: &mut Surface, b: Rect, text: impl Fn(&mut Surface, &str, Rect, u32)) {
    f.fill_rect(b.x, b.y, b.width, b.height, 0xFFF0F0F0);
    let ink = 0xFF222222;
    let blue = 0xFF0066BB;
    for (i, label) in ["Playback", "Recording", "Sounds", "Communications"]
        .iter()
        .enumerate()
    {
        let r = tab(b, i);
        aero::blend_rounded_rect(f, r, 3, if s.tab == i { 0xFFFFFFFF } else { 0xFFE4EAF0 });
        text(
            f,
            label,
            Rect::new(r.x + 10, r.y + 6, r.width - 12, 22),
            ink,
        );
    }
    match s.tab {
        0 => {
            text(
                f,
                "Select a playback device to modify its settings:",
                Rect::new(b.x + 24, b.y + 60, 530, 24),
                ink,
            );
            text(
                f,
                if s.ready {
                    "Speakers — High Definition Audio Device"
                } else {
                    "No audio output device is installed."
                },
                Rect::new(b.x + 40, b.y + 100, 530, 28),
                blue,
            );
            if s.ready {
                text(
                    f,
                    "Default Device",
                    Rect::new(b.x + 40, b.y + 130, 400, 24),
                    0xFF39743A,
                );
            }
            text(
                f,
                &format!("Volume: {}%", s.volume),
                Rect::new(b.x + 24, b.y + 165, 200, 24),
                ink,
            );
            let r = slider(b);
            f.fill_rect(r.x, r.y + 11, r.width, 5, 0xFFAAAAAA);
            aero::blend_rounded_rect(
                f,
                Rect::new(
                    r.x + r.width as i32 * s.volume as i32 / 100 - 5,
                    r.y,
                    10,
                    27,
                ),
                3,
                0xFF62B7E9,
            );
            text(
                f,
                if s.muted { "[x] Mute" } else { "[ ] Mute" },
                mute(b),
                ink,
            );
        }
        1 => {
            text(
                f,
                "No audio recording device is available.",
                Rect::new(b.x + 24, b.y + 80, 530, 30),
                ink,
            );
        }
        2 => {
            text(
                f,
                "Sound Scheme:",
                Rect::new(b.x + 24, b.y + 53, 400, 24),
                ink,
            );
            for (i, label) in ["No Sounds", "Windows Default"].iter().enumerate() {
                let r = scheme(b, i);
                aero::blend_rounded_rect(
                    f,
                    r,
                    3,
                    if s.enabled == (i == 1) {
                        0xFFD5ECFC
                    } else {
                        0xFFFFFFFF
                    },
                );
                text(f, label, Rect::new(r.x + 8, r.y + 6, 210, 23), ink);
            }
            text(
                f,
                "Program Events:",
                Rect::new(b.x + 24, b.y + 114, 400, 24),
                ink,
            );
            let r = list(b);
            f.fill_rect(r.x, r.y, r.width, r.height, 0xFFFFFFFF);
            for (i, (index, (_, name))) in EVENTS
                .iter()
                .enumerate()
                .skip(s.offset)
                .take(capacity(b))
                .enumerate()
            {
                let row = Rect::new(r.x + 1, r.y + i as i32 * 24, r.width - 2, 24);
                if index == s.selected {
                    f.fill_rect(row.x, row.y, row.width, row.height, 0xFFCDE8FF);
                }
                text(
                    f,
                    name,
                    Rect::new(row.x + 8, row.y + 4, row.width - 10, 22),
                    ink,
                );
            }
            for next in [false, true] {
                let r = scroll(b, next);
                text(f, if next { "v" } else { "^" }, r, ink);
            }
            text(
                f,
                if s.startup {
                    "[x] Play Windows Startup sound"
                } else {
                    "[ ] Play Windows Startup sound"
                },
                startup(b),
                ink,
            );
        }
        _ => {
            text(
                f,
                "No communications application has registered an audio session.",
                Rect::new(b.x + 24, b.y + 80, b.width.saturating_sub(48), 50),
                ink,
            );
        }
    }
    if matches!(s.tab, 0 | 2) {
        let r = test(b);
        aero::blend_rounded_rect(f, r, 3, 0xFFDDEBF5);
        text(f, "Test", Rect::new(r.x + 25, r.y + 6, 60, 22), ink);
    }
    text(
        f,
        &s.message,
        Rect::new(
            b.x + 24,
            b.y + b.height as i32 - 65,
            b.width.saturating_sub(48),
            50,
        ),
        0xFF992222,
    );
}
