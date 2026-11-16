use super::{edit, input};
use crate::graphics::Rect;
use alloc::string::{String, ToString};

/// Windows common color dialog's six rows of basic colors, in display order.
pub const BASIC_COLORS: [u32; 48] = [
    0xffff8080, 0xffffff80, 0xff80ff80, 0xff00ff80, 0xff80ffff, 0xff0080ff, 0xffff80c0, 0xffff80ff,
    0xffff0000, 0xffffff00, 0xff80ff00, 0xff00ff40, 0xff00ffff, 0xff0080c0, 0xff8080c0, 0xffff00ff,
    0xff804040, 0xffff8040, 0xff00ff00, 0xff008080, 0xff004080, 0xff8080ff, 0xff800040, 0xffff0080,
    0xff800000, 0xffff8000, 0xff008000, 0xff008040, 0xff0000ff, 0xff0000a0, 0xff800080, 0xff8000ff,
    0xff400000, 0xff804000, 0xff004000, 0xff004040, 0xff000080, 0xff000040, 0xff400040, 0xff400080,
    0xff000000, 0xff808000, 0xff808040, 0xff808080, 0xff408080, 0xffc0c0c0, 0xff400040, 0xffffffff,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Drag {
    Spectrum,
    Luminosity,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColorDialog {
    pub color: u32,
    pub custom: [u32; 16],
    pub fields: [String; 6],
    pub focus: usize,
    pub selected: usize,
    pub custom_slot: usize,
    pub select: bool,
    pub error: String,
    drag: Option<Drag>,
}
impl ColorDialog {
    pub fn new(color: u32, custom: [u32; 16]) -> Self {
        let mut dialog = Self {
            color: color | 0xff000000,
            custom,
            fields: core::array::from_fn(|_| String::new()),
            focus: 0,
            selected: BASIC_COLORS
                .iter()
                .position(|value| *value == color)
                .unwrap_or(0),
            custom_slot: 0,
            select: true,
            error: String::new(),
            drag: None,
        };
        dialog.sync();
        dialog
    }
    fn sync(&mut self) {
        let (h, s, l) = rgb_to_hsl(self.color);
        self.fields = [
            h,
            s,
            l,
            (self.color >> 16) & 255,
            (self.color >> 8) & 255,
            self.color & 255,
        ]
        .map(|v| v.to_string());
        self.error.clear();
    }
    fn update(&mut self) -> bool {
        let mut values = [0u32; 6];
        for (index, value) in values.iter_mut().enumerate() {
            let Ok(parsed) = self.fields[index].parse::<u32>() else {
                self.error = "Enter whole numbers for each color component.".into();
                return false;
            };
            if parsed
                > if index == 0 {
                    239
                } else if index < 3 {
                    240
                } else {
                    255
                }
            {
                self.error = "Hue: 0-239; saturation/luminosity: 0-240; RGB: 0-255.".into();
                return false;
            }
            *value = parsed;
        }
        if (1..=3).contains(&self.focus) {
            self.color = hsl_to_rgb(values[0], values[1], values[2]);
            self.fields[3] = ((self.color >> 16) & 255).to_string();
            self.fields[4] = ((self.color >> 8) & 255).to_string();
            self.fields[5] = (self.color & 255).to_string();
        } else {
            self.color = 0xff000000 | (values[3] << 16) | (values[4] << 8) | values[5];
            let (h, s, l) = rgb_to_hsl(self.color);
            self.fields[0] = h.to_string();
            self.fields[1] = s.to_string();
            self.fields[2] = l.to_string();
        }
        self.error.clear();
        true
    }
    pub fn key(&mut self, key: u8) -> bool {
        match key {
            13 => return self.update(),
            9 => {
                self.focus = (self.focus + 1) % 8;
                self.select = true;
            }
            b' ' if self.focus == 7 => self.add_custom(),
            input::LEFT | input::RIGHT | input::UP | input::DOWN if self.focus == 0 => {
                self.selected = match key {
                    input::LEFT => self.selected.saturating_sub(1),
                    input::RIGHT => (self.selected + 1).min(63),
                    input::UP => self.selected.saturating_sub(8),
                    _ => (self.selected + 8).min(63),
                };
                self.choose(self.selected);
            }
            _ if (1..=6).contains(&self.focus) => {
                edit(
                    &mut self.fields[self.focus - 1],
                    &mut self.select,
                    key,
                    false,
                );
                self.update();
            }
            _ => {}
        }
        false
    }
    fn choose(&mut self, selected: usize) {
        self.color = if selected < 48 {
            BASIC_COLORS[selected]
        } else {
            self.custom_slot = selected - 48;
            self.custom[selected - 48]
        };
        self.sync();
    }
    fn add_custom(&mut self) {
        if self.update() {
            self.custom[self.custom_slot] = self.color;
            self.selected = 48 + self.custom_slot;
            self.custom_slot = (self.custom_slot + 1) % 16;
        }
    }
    pub fn pointer(&mut self, x: i32, y: i32) {
        for index in 0..64 {
            if swatch(index).contains(x, y) {
                self.selected = index;
                self.focus = 0;
                self.choose(index);
                return;
            }
        }
        for index in 0..6 {
            if field(index).contains(x, y) {
                self.focus = index + 1;
                self.select = true;
                return;
            }
        }
        if Rect::new(286, 300, 238, 27).contains(x, y) {
            self.add_custom();
        }
        self.drag = if Rect::new(286, 58, 218, 132).contains(x, y) {
            Some(Drag::Spectrum)
        } else if Rect::new(512, 58, 12, 132).contains(x, y) {
            Some(Drag::Luminosity)
        } else {
            None
        };
        self.drag_to(x, y, true);
    }
    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }
    /// Preserve HSL coordinates at black and white: RGB alone loses hue/saturation.
    pub fn hsl(&self) -> (u32, u32, u32) {
        (
            self.fields[0].parse::<u32>().unwrap_or(0).min(239),
            self.fields[1].parse::<u32>().unwrap_or(0).min(240),
            self.fields[2].parse::<u32>().unwrap_or(0).min(240),
        )
    }
    pub fn drag_to(&mut self, x: i32, y: i32, held: bool) -> bool {
        if !held {
            return self.drag.take().is_some();
        }
        let (mut h, mut s, mut l) = self.hsl();
        match self.drag {
            Some(Drag::Spectrum) => {
                h = (x - 286).clamp(0, 217) as u32 * 239 / 217;
                s = 240 - (y - 58).clamp(0, 131) as u32 * 240 / 131;
            }
            Some(Drag::Luminosity) => l = 240 - (y - 58).clamp(0, 131) as u32 * 240 / 131,
            None => return false,
        }
        self.color = hsl_to_rgb(h, s, l);
        self.fields = [
            h,
            s,
            l,
            (self.color >> 16) & 255,
            (self.color >> 8) & 255,
            self.color & 255,
        ]
        .map(|value| value.to_string());
        self.focus = 1;
        self.error.clear();
        true
    }
}
pub(super) fn swatch(index: usize) -> Rect {
    let row = if index < 48 {
        index / 8
    } else {
        (index - 48) / 8
    };
    Rect::new(
        14 + (index % 8) as i32 * 29,
        if index < 48 { 58 } else { 254 } + row as i32 * 26,
        25,
        22,
    )
}
pub(super) fn field(index: usize) -> Rect {
    Rect::new(
        if index < 3 { 331 } else { 466 },
        202 + (index % 3) as i32 * 30,
        58,
        24,
    )
}

/// Windows color dialogs use HLSMAX=240, with hue wrapping at 240.
pub(super) fn hsl_to_rgb(hue: u32, sat: u32, lum: u32) -> u32 {
    let chroma = ((240 - (2 * lum as i32 - 240).unsigned_abs()) * sat + 120) / 240;
    let segment = hue * 6;
    let x = chroma * (240 - ((segment % 480) as i32 - 240).unsigned_abs()) / 240;
    let (r, g, b) = match segment / 240 {
        0 => (chroma, x, 0),
        1 => (x, chroma, 0),
        2 => (0, chroma, x),
        3 => (0, x, chroma),
        4 => (x, 0, chroma),
        _ => (chroma, 0, x),
    };
    let base = lum - chroma / 2;
    let channel = |value| ((value + base) * 255 + 120) / 240;
    0xff000000 | (channel(r).min(255) << 16) | (channel(g).min(255) << 8) | channel(b).min(255)
}
pub(super) fn rgb_to_hsl(color: u32) -> (u32, u32, u32) {
    let r = ((color >> 16) & 255) as i32;
    let g = ((color >> 8) & 255) as i32;
    let b = (color & 255) as i32;
    let hi = r.max(g).max(b);
    let lo = r.min(g).min(b);
    let delta = hi - lo;
    let sum = hi + lo;
    let lum = (sum * 240 + 255) / 510;
    if delta == 0 {
        return (0, 0, lum as u32);
    }
    let sat = (delta * 240 + (255 - (sum - 255).abs()) / 2) / (255 - (sum - 255).abs());
    let hue = if hi == r {
        (g - b) * 40 / delta
    } else if hi == g {
        80 + (b - r) * 40 / delta
    } else {
        160 + (r - g) * 40 / delta
    };
    (hue.rem_euclid(240) as u32, sat as u32, lum as u32)
}
