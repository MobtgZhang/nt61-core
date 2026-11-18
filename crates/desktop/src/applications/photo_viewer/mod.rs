//! Windows 7 style picture browsing. Image data and file mutations are supplied by the shell.
pub mod codec;
pub mod printing;
use super::{
    explorer::input,
    system_icons::{self, Glyph},
    view::{Button, Command as ViewCommand},
    AppAction, AppState, ApplicationSession,
};
use crate::{
    compositor::aero,
    graphics::{Rect, Surface},
};
use alloc::{format, string::String, vec::Vec};
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    Navigate(i32),
    Page(i32),
    Rotate(bool),
    Delete,
    Edit,
    Wallpaper,
    Properties,
    Print,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Menu(u8),
    Navigate(i32),
    Zoom(i32),
    Fit,
    Actual,
    Slideshow,
    Pause,
    ZoomMenu,
    ToggleFit,
    Rotate(bool),
    Delete,
    ConfirmDelete,
    Cancel,
    Open,
    SaveAs,
    Print,
    PrintLayout(usize),
    PrintPaper,
    PrintCopies(i32),
    PrintFill,
    PrintSubmit,
    PrintCancel,
    Edit,
    Wallpaper,
    Properties,
    Help,
    Unavailable(u8),
    Speed(u64),
    Page(i32),
    Close,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Viewer {
    pub current: Option<u32>,
    pub name: String,
    pub image: Option<Surface>,
    pub siblings: Vec<u32>,
    pub page: usize,
    pub pages: usize,
    pub zoom: u32,
    pub fit: bool,
    pub pan: (i32, i32),
    pub drag: Option<(i32, i32)>,
    pub slideshow: bool,
    pub paused: bool,
    pub interval: u64,
    pub next_frame: u64,
    pub menu: Option<u8>,
    pub menu_selection: usize,
    pub menu_point: (i32, i32),
    pub zoom_grab: bool,
    pub viewport: Rect,
    pub message: String,
    pub delete_confirm: bool,
    pub print: Option<printing::Options>,
}
impl Default for Viewer {
    fn default() -> Self {
        Self {
            current: None,
            name: String::new(),
            image: None,
            siblings: Vec::new(),
            page: 0,
            pages: 1,
            zoom: 100,
            fit: true,
            pan: (0, 0),
            drag: None,
            slideshow: false,
            paused: false,
            interval: 5000,
            next_frame: 0,
            menu: None,
            menu_selection: 0,
            menu_point: (0, 0),
            zoom_grab: false,
            viewport: Rect::new(0, 0, 640, 460),
            message: String::new(),
            delete_confirm: false,
            print: None,
        }
    }
}
impl Viewer {
    pub fn load(
        &mut self,
        id: u32,
        name: String,
        bytes: &[u8],
        siblings: Vec<u32>,
        page: usize,
    ) -> Result<(), &'static str> {
        let (image, pages) = codec::decode(bytes, page)?;
        self.current = Some(id);
        self.name = name;
        self.image = Some(image);
        self.siblings = siblings;
        self.page = page;
        self.pages = pages;
        self.reset_view();
        self.message.clear();
        self.delete_confirm = false;
        Ok(())
    }
    pub fn reset_view(&mut self) {
        self.fit = true;
        self.zoom = 100;
        self.pan = (0, 0);
        self.drag = None;
    }
    pub fn neighbor(&self, delta: i32) -> Option<u32> {
        let n = self.siblings.len();
        if n == 0 {
            return None;
        }
        let at = self
            .siblings
            .iter()
            .position(|id| Some(*id) == self.current)
            .unwrap_or(0);
        self.siblings
            .get((at as i64 + delta as i64).rem_euclid(n as i64) as usize)
            .copied()
    }
    pub fn tick(&mut self, now: u64) -> bool {
        if !self.slideshow || self.paused || self.image.is_none() {
            return false;
        }
        if self.next_frame == 0 {
            self.next_frame = now.saturating_add(self.interval);
            return false;
        }
        if now < self.next_frame {
            return false;
        }
        self.next_frame = now.saturating_add(self.interval);
        true
    }
    pub fn zoom_by(&mut self, delta: i32, b: Rect) {
        if delta == 0 || self.image.is_none() {
            return;
        }
        let base = if self.fit { self.scale(b) } else { self.zoom };
        self.zoom = if delta > 0 {
            base.saturating_mul(5) / 4
        } else {
            base.saturating_mul(4) / 5
        }
        .clamp(10, 1600);
        self.fit = false;
        self.clamp_pan(b);
    }
    fn scale(&self, b: Rect) -> u32 {
        let Some(s) = &self.image else {
            return 100;
        };
        ((b.width.saturating_sub(8) as u64 * 100 / s.width() as u64)
            .min(b.height.saturating_sub(8) as u64 * 100 / s.height() as u64)
            .min(100) as u32)
            .max(1)
    }
    fn clamp_pan(&mut self, b: Rect) {
        if let Some(s) = &self.image {
            let z = if self.fit { self.scale(b) } else { self.zoom };
            let maxx = (s.width() as i64 * z as i64 / 100 - b.width as i64).max(0) / 2;
            let maxy = (s.height() as i64 * z as i64 / 100 - b.height as i64).max(0) / 2;
            self.pan.0 = self.pan.0.clamp(-maxx as i32, maxx as i32);
            self.pan.1 = self.pan.1.clamp(-maxy as i32, maxy as i32);
        }
    }
    pub fn captures_pointer(&self) -> bool {
        self.drag.is_some() || self.zoom_grab
    }
}
pub fn image_bounds(b: Rect, full: bool) -> Rect {
    if full {
        b
    } else {
        Rect::new(
            b.x + 1,
            b.y + 29,
            b.width.saturating_sub(2),
            b.height.saturating_sub(91),
        )
    }
}
fn toolbar(b: Rect) -> Rect {
    Rect::new(
        b.x + (b.width as i32 - 404) / 2,
        b.y + b.height as i32 - 48,
        404,
        34,
    )
}
fn menu_bounds(b: Rect, v: &Viewer, kind: u8) -> Rect {
    if kind == 6 {
        let t = toolbar(b);
        return Rect::new(t.x + 14, t.y - 181, 44, 176);
    }
    let width = 260.min(b.width.saturating_sub(4));
    let height = menu_items(kind).len() as u32 * 26;
    let (x, y) = match kind {
        0..=4 => (b.x + 9 + kind as i32 * 63, b.y + 28),
        6 => {
            let t = toolbar(b);
            (t.x + 18, t.y - height as i32 - 3)
        }
        _ => v.menu_point,
    };
    Rect::new(
        x.clamp(b.x + 2, b.x + (b.width - width).max(2) as i32),
        y.clamp(
            b.y + 2,
            b.y + (b.height.saturating_sub(height)).max(2) as i32,
        ),
        width,
        height,
    )
}
const ZOOM_STOPS: [u32; 19] = [
    10, 12, 16, 20, 25, 33, 50, 66, 75, 100, 125, 150, 200, 300, 400, 600, 800, 1200, 1600,
];
fn slider_zoom(v: &mut Viewer, b: Rect, y: i32) {
    let popup = menu_bounds(b, v, 6);
    let at = ((popup.y + 156 - y).clamp(0, 136) as usize * (ZOOM_STOPS.len() - 1) + 68) / 136;
    v.zoom = ZOOM_STOPS[at];
    v.fit = false;
    v.clamp_pan(image_bounds(b, v.slideshow));
}
fn menu_items(kind: u8) -> Vec<(&'static str, Command)> {
    match kind {
        0 => alloc::vec![
            ("Open...", Command::Open),
            ("Make a copy...", Command::SaveAs),
            ("Delete    Del", Command::Delete),
            ("Set as desktop background", Command::Wallpaper),
            ("Properties", Command::Properties),
            ("Exit", Command::Close)
        ],
        1 => alloc::vec![
            ("Print...    Ctrl+P", Command::Print),
            ("Order prints...", Command::Unavailable(0))
        ],
        2 => alloc::vec![("Attach picture to an e-mail", Command::Unavailable(1))],
        3 => alloc::vec![("Data disc...", Command::Unavailable(2))],
        4 => alloc::vec![
            ("Paint", Command::Edit),
            ("Choose a picture...", Command::Open)
        ],
        6 => alloc::vec![
            ("Zoom in    +", Command::Zoom(1)),
            ("Zoom out    -", Command::Zoom(-1)),
            ("Actual size    0", Command::Actual),
            ("Fit to window    F", Command::Fit)
        ],
        _ => alloc::vec![
            ("Pause / Play", Command::Pause),
            ("Next", Command::Navigate(1)),
            ("Previous", Command::Navigate(-1)),
            ("Slideshow speed: slow", Command::Speed(7000)),
            ("Slideshow speed: medium", Command::Speed(5000)),
            ("Slideshow speed: fast", Command::Speed(3000)),
            ("Exit slideshow", Command::Slideshow)
        ],
    }
}
pub fn buttons(app: &ApplicationSession, b: Rect) -> Vec<Button> {
    let AppState::PhotoViewer(v) = &app.state else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut push = |r, label: &str, c| {
        out.push(Button {
            bounds: r,
            label: label.into(),
            command: ViewCommand::Photo(c),
        })
    };
    if !v.message.is_empty() {
        push(
            Rect::new(
                b.x + b.width as i32 / 2 - 40,
                b.y + b.height as i32 / 2 + 65,
                80,
                25,
            ),
            "OK",
            Command::Cancel,
        );
        return out;
    }
    if v.delete_confirm {
        push(
            Rect::new(
                b.x + b.width as i32 / 2 - 92,
                b.y + b.height as i32 / 2 + 54,
                80,
                25,
            ),
            "Yes",
            Command::ConfirmDelete,
        );
        push(
            Rect::new(
                b.x + b.width as i32 / 2 + 8,
                b.y + b.height as i32 / 2 + 54,
                80,
                25,
            ),
            "No",
            Command::Cancel,
        );
        return out;
    }
    if v.print.is_some() {
        return printing::buttons(b);
    }
    if !v.slideshow {
        for (i, label) in ["File", "Print", "E-mail", "Burn", "Open"]
            .iter()
            .enumerate()
        {
            push(
                Rect::new(b.x + 9 + i as i32 * 63, b.y + 1, 62, 26),
                label,
                Command::Menu(i as u8),
            );
        }
        push(
            Rect::new(b.x + b.width as i32 - 29, b.y + 4, 20, 20),
            "Help",
            Command::Help,
        );
        let t = toolbar(b);
        for (x, w, c, label) in [
            (18, 33, Command::ZoomMenu, "Zoom"),
            (
                74,
                28,
                Command::ToggleFit,
                if v.fit {
                    "Actual size"
                } else {
                    "Fit to window"
                },
            ),
            (120, 48, Command::Navigate(-1), "Previous picture"),
            (170, 48, Command::Slideshow, "Play slideshow"),
            (220, 48, Command::Navigate(1), "Next picture"),
            (282, 30, Command::Rotate(false), "Rotate counterclockwise"),
            (316, 30, Command::Rotate(true), "Rotate clockwise"),
            (363, 28, Command::Delete, "Delete"),
        ] {
            push(Rect::new(t.x + x, t.y, w, 34), label, c);
        }
        if v.pages > 1 {
            push(
                Rect::new(b.x + 7, b.y + b.height as i32 - 79, 30, 20),
                "<",
                Command::Page(-1),
            );
            push(
                Rect::new(b.x + 100, b.y + b.height as i32 - 79, 30, 20),
                ">",
                Command::Page(1),
            );
        }
    }
    if let Some(menu) = v.menu.filter(|m| *m != 6) {
        for (i, (label, c)) in menu_items(menu).into_iter().enumerate() {
            let r = menu_bounds(b, v, menu);
            push(Rect::new(r.x, r.y + i as i32 * 26, r.width, 26), label, c);
        }
    }
    out
}
pub fn execute(app: &mut ApplicationSession, c: Command, b: Rect) -> AppAction {
    let AppState::PhotoViewer(v) = &mut app.state else {
        return AppAction::None;
    };
    v.viewport = b;
    v.zoom_grab = false;
    if !matches!(c, Command::Menu(_)) {
        v.menu = None;
    }
    match c {
        Command::Menu(i) => {v.menu = if v.menu == Some(i) {None} else {Some(i)};v.menu_selection=0;},
        Command::Navigate(d) => return AppAction::Photo(Request::Navigate(d)),
        Command::Page(d) => return AppAction::Photo(Request::Page(d)),
        Command::Rotate(right) => return AppAction::Photo(Request::Rotate(right)),
        Command::Zoom(d) => v.zoom_by(d, image_bounds(b, v.slideshow)),
        Command::Fit => v.reset_view(),
        Command::Actual => {v.fit=false;v.zoom=100;v.pan=(0,0);},
        Command::Slideshow => {
            if v.image.is_some() {v.slideshow=!v.slideshow;v.paused=false;v.next_frame=0;v.reset_view();}
        },
        Command::Pause => {v.paused=!v.paused;v.next_frame=0;},
        Command::ZoomMenu => {if v.image.is_some() {v.menu=Some(6);}},
        Command::ToggleFit => {
            if v.fit {v.fit=false;v.zoom=100;v.pan=(0,0);} else {v.reset_view();}
        },
        Command::Speed(ms) => {v.interval=ms;v.next_frame=0;},
        Command::Delete => v.delete_confirm=v.current.is_some(),
        Command::ConfirmDelete => {v.delete_confirm=false;return AppAction::Photo(Request::Delete);},
        Command::Cancel => {v.message.clear();v.delete_confirm=false;},
        Command::Open => return AppAction::Open,
        Command::SaveAs => {if v.image.is_some() {return AppAction::SaveAs;}},
        Command::Print => {if v.image.is_some() {v.print=Some(printing::Options::default());v.slideshow=false;}},
        Command::PrintLayout(layout) => {if let Some(o)=&mut v.print {o.layout=layout.min(printing::LAYOUTS.len()-1);}},
        Command::PrintPaper => {if let Some(o)=&mut v.print {o.paper=(o.paper+1)%printing::PAPERS.len();}},
        Command::PrintCopies(delta) => {if let Some(o)=&mut v.print {o.copies=(i32::from(o.copies)+delta).clamp(1,999) as u16;}},
        Command::PrintFill => {if let Some(o)=&mut v.print {o.fill=!o.fill;}},
        Command::PrintSubmit => {if v.print.is_some() {return AppAction::Photo(Request::Print);}},
        Command::PrintCancel => v.print=None,
        Command::Edit => return AppAction::Photo(Request::Edit),
        Command::Wallpaper => return AppAction::Photo(Request::Wallpaper),
        Command::Properties => return AppAction::Photo(Request::Properties),
        Command::Close => return AppAction::RequestClose,
        Command::Help => v.message="Windows Photo Viewer\nLeft / Right: previous / next picture\n+ / -: zoom; 0: actual size; F: fit\nF11: slideshow; Space: pause / play; Esc: exit slideshow\nCtrl+P: print; Ctrl+O: open a picture\nDrag to pan a zoomed picture.".into(),
        Command::Unavailable(n) => v.message=match n {
            0=>"No online photo printing provider is installed.",
            1=>"There is no e-mail program associated to perform the requested action.",
            _=>"No writable optical-disc service is available."
        }.into(),
    }
    AppAction::None
}
pub fn key(app: &mut ApplicationSession, key: u8) -> AppAction {
    app.hot = None;
    let AppState::PhotoViewer(v) = &mut app.state else {
        return AppAction::None;
    };
    if !v.message.is_empty() || v.delete_confirm {
        return match key {
            13 if v.delete_confirm => {
                v.delete_confirm = false;
                AppAction::Photo(Request::Delete)
            }
            13 | 27 => {
                v.message.clear();
                v.delete_confirm = false;
                AppAction::None
            }
            _ => AppAction::None,
        };
    }
    if let Some(o) = &mut v.print {
        match key {
            27 => v.print = None,
            13 => return AppAction::Photo(Request::Print),
            input::UP => o.layout = o.layout.saturating_sub(1),
            input::DOWN => o.layout = (o.layout + 1).min(printing::LAYOUTS.len() - 1),
            b'+' | b'=' => o.copies = o.copies.saturating_add(1).min(999),
            b'-' => o.copies = o.copies.saturating_sub(1).max(1),
            b' ' => o.fill = !o.fill,
            _ => {}
        }
        return AppAction::None;
    }
    let b = v.viewport;
    if let Some(menu) = v.menu {
        if key == 27 {
            v.menu = None;
            v.zoom_grab = false;
            return AppAction::None;
        }
        if menu == 6 {
            match key {
                input::UP | input::RIGHT | b'+' | b'=' => {
                    v.zoom_by(1, image_bounds(b, v.slideshow))
                }
                input::DOWN | input::LEFT | b'-' => v.zoom_by(-1, image_bounds(b, v.slideshow)),
                input::HOME => {
                    v.fit = false;
                    v.zoom = 1600;
                    v.clamp_pan(image_bounds(b, v.slideshow));
                }
                input::END => {
                    v.fit = false;
                    v.zoom = 10;
                    v.clamp_pan(image_bounds(b, v.slideshow));
                }
                13 => v.menu = None,
                _ => {}
            }
            return AppAction::None;
        }
        let items = menu_items(menu);
        match key {
            input::UP => v.menu_selection = (v.menu_selection + items.len() - 1) % items.len(),
            input::DOWN => v.menu_selection = (v.menu_selection + 1) % items.len(),
            input::HOME => v.menu_selection = 0,
            input::END => v.menu_selection = items.len() - 1,
            input::LEFT | input::RIGHT if menu <= 4 => {
                v.menu = Some(if key == input::LEFT {
                    (menu + 4) % 5
                } else {
                    (menu + 1) % 5
                });
                v.menu_selection = 0;
            }
            13 => {
                let c = items[v.menu_selection.min(items.len() - 1)].1.clone();
                return execute(app, c, b);
            }
            _ => {}
        }
        return AppAction::None;
    }
    let c = match key {
        input::LEFT | input::PAGE_UP => Command::Navigate(-1),
        input::RIGHT | input::PAGE_DOWN => Command::Navigate(1),
        b'+' | b'=' => Command::Zoom(1),
        b'-' => Command::Zoom(-1),
        b'0' => Command::Actual,
        b'f' | b'F' => Command::Fit,
        input::FULLSCREEN => Command::Slideshow,
        b' ' if v.slideshow => Command::Pause,
        b' ' => Command::Slideshow,
        input::DELETE => Command::Delete,
        15 => Command::Open,
        16 => Command::Print,
        input::HELP => Command::Help,
        input::ALT_FILE => Command::Menu(0),
        27 => {
            if v.menu.take().is_none() {
                v.slideshow = false;
                v.drag = None;
            }
            return AppAction::None;
        }
        _ => return AppAction::None,
    };
    execute(app, c, b)
}
pub fn pointer(
    app: &mut ApplicationSession,
    b: Rect,
    x: i32,
    y: i32,
    down: bool,
    held: bool,
    right: bool,
) -> (bool, AppAction) {
    let AppState::PhotoViewer(v) = &mut app.state else {
        return (false, AppAction::None);
    };
    v.viewport = b;
    if !held {
        v.zoom_grab = false;
    }
    if v.menu == Some(6) && v.message.is_empty() && !v.delete_confirm && v.print.is_none() {
        let r = menu_bounds(b, v, 6);
        if (down && !right && r.contains(x, y)) || (held && v.zoom_grab) {
            slider_zoom(v, b, y);
            v.zoom_grab = held;
            return (true, AppAction::None);
        }
    }
    if v.message.is_empty() && !v.delete_confirm && v.menu.is_none() && v.print.is_none() {
        if !held {
            v.drag = None;
        }
        if held {
            if let Some((px, py)) = v.drag {
                v.pan.0 += x - px;
                v.pan.1 += y - py;
                v.drag = Some((x, y));
                v.clamp_pan(image_bounds(b, v.slideshow));
                return (true, AppAction::None);
            }
        }
        if down && image_bounds(b, v.slideshow).contains(x, y) && !right {
            if v.slideshow {
                return (true, AppAction::Photo(Request::Navigate(1)));
            }
            if !v.fit {
                v.drag = Some((x, y));
                return (true, AppAction::None);
            }
        }
        if right && v.slideshow {
            v.menu = Some(5);
            v.menu_point = (x, y);
            return (true, AppAction::None);
        }
    }
    let hot = buttons(app, b)
        .into_iter()
        .rev()
        .find(|a| a.bounds.contains(x, y));
    let changed = app.hot != hot.as_ref().map(|a| a.bounds);
    app.hot = hot.as_ref().map(|a| a.bounds);
    if down && !right {
        if let Some(Button {
            command: ViewCommand::Photo(c),
            ..
        }) = hot
        {
            return (true, execute(app, c, b));
        }
        if let AppState::PhotoViewer(v) = &mut app.state {
            return (v.menu.take().is_some(), AppAction::None);
        }
    }
    (changed, AppAction::None)
}
fn sample(image: &Surface, x: i32, y: i32, w: u32, h: u32) -> u32 {
    if image.width() == w && image.height() == h {
        return image.pixel(x as u32, y as u32).unwrap_or(0);
    }
    let fx = (((x as i64 * 2 + 1) * image.width() as i64 * 128 / w as i64) - 128)
        .clamp(0, (image.width() as i64 - 1) * 256);
    let fy = (((y as i64 * 2 + 1) * image.height() as i64 * 128 / h as i64) - 128)
        .clamp(0, (image.height() as i64 - 1) * 256);
    let (ix, iy) = (fx as u32 / 256, fy as u32 / 256);
    let (dx, dy) = (fx as u32 % 256, fy as u32 % 256);
    let mut a = 0u64;
    let mut rgb = [0u64; 3];
    for (px, py, weight) in [
        (ix, iy, (256 - dx) * (256 - dy)),
        ((ix + 1).min(image.width() - 1), iy, dx * (256 - dy)),
        (ix, (iy + 1).min(image.height() - 1), (256 - dx) * dy),
        (
            (ix + 1).min(image.width() - 1),
            (iy + 1).min(image.height() - 1),
            dx * dy,
        ),
    ] {
        let p = image.pixel(px, py).unwrap_or(0);
        let alpha = (p >> 24) as u64 * weight as u64;
        a += alpha;
        for (i, shift) in [16, 8, 0].into_iter().enumerate() {
            rgb[i] += ((p >> shift) & 255) as u64 * alpha;
        }
    }
    if a == 0 {
        0
    } else {
        ((a / 65536) as u32) << 24
            | ((rgb[0] / a) as u32) << 16
            | ((rgb[1] / a) as u32) << 8
            | (rgb[2] / a) as u32
    }
}
pub fn draw<F: Fn(&mut Surface, &str, Rect, u32)>(
    app: &ApplicationSession,
    s: &mut Surface,
    b: Rect,
    text: &F,
) {
    let raw_text = text;
    let text = &|s: &mut Surface, value: &str, r: Rect, color: u32| {
        super::system_text::paragraph(s, value, r, color, raw_text)
    };
    let AppState::PhotoViewer(v) = &app.state else {
        return;
    };
    let area = image_bounds(b, v.slideshow);
    s.fill_rect(
        area.x,
        area.y,
        area.width,
        area.height,
        if v.slideshow { 0xff000000 } else { 0xffeef3fa },
    );
    if let Some(image) = &v.image {
        let z = if v.fit { v.scale(area) } else { v.zoom };
        let (w, h) = (
            (image.width() as u64 * z as u64 / 100).max(1) as u32,
            (image.height() as u64 * z as u64 / 100).max(1) as u32,
        );
        let origin = (
            area.x
                + (area.width as i32 - w as i32) / 2
                + v.pan.0.clamp(
                    -((w.saturating_sub(area.width) / 2) as i32),
                    (w.saturating_sub(area.width) / 2) as i32,
                ),
            area.y
                + (area.height as i32 - h as i32) / 2
                + v.pan.1.clamp(
                    -((h.saturating_sub(area.height) / 2) as i32),
                    (h.saturating_sub(area.height) / 2) as i32,
                ),
        );
        let r = Rect::new(origin.0, origin.1, w, h);
        if let Some(clip) = r
            .intersection(&area)
            .and_then(|r| r.intersection(&s.drawing_bounds()))
        {
            for y in clip.y..clip.y + clip.height as i32 {
                for x in clip.x..clip.x + clip.width as i32 {
                    let p = sample(image, x - origin.0, y - origin.1, w, h);
                    s.blend_pixel(x as u32, y as u32, p);
                }
            }
        }
    } else {
        text(
            s,
            "Windows Photo Viewer can't display this picture because no picture is selected.",
            Rect::new(
                area.x + 20,
                area.y + area.height as i32 / 2,
                area.width.saturating_sub(40),
                50,
            ),
            0xff505050,
        );
    }
    if !v.slideshow {
        let top = Rect::new(b.x, b.y, b.width, 28);
        s.fill_rect(top.x, top.y, top.width, top.height, 0xffe4edf7);
        let t = toolbar(b);
        s.blend_rounded_rect(t, 17, 0x5579afd3);
        s.stroke_rounded_rect(t, 17, 1, 0xff779da9);
        s.fill_rect(t.x + 16, t.y + 1, t.width - 32, 1, 0x99ffffff);
        let all = buttons(app, b);
        for btn in &all {
            let ViewCommand::Photo(c) = &btn.command else {
                continue;
            };
            if matches!(c, Command::Menu(_)) {
                if app.hot == Some(btn.bounds) {
                    aero::menu_item(s, btn.bounds, true);
                }
                text(
                    s,
                    &btn.label,
                    Rect::new(
                        btn.bounds.x + 8,
                        btn.bounds.y + 6,
                        btn.bounds.width - 18,
                        20,
                    ),
                    0xff173747,
                );
                if !matches!(c, Command::Menu(2)) {
                    let x = btn.bounds.x + btn.bounds.width as i32 - 13;
                    for n in 0..3 {
                        s.fill_rect(
                            x + n,
                            btn.bounds.y + 11 + n,
                            (5 - n * 2) as u32,
                            1,
                            0xff294655,
                        );
                    }
                }
            }
        }
        for (x, w, g) in [
            (18, 33, Glyph::Zoom),
            (74, 28, Glyph::Fit),
            (120, 48, Glyph::Previous),
            (170, 48, Glyph::Slideshow),
            (220, 48, Glyph::Next),
            (282, 30, Glyph::RotateLeft),
            (316, 30, Glyph::RotateRight),
            (363, 28, Glyph::Delete),
        ] {
            let r = Rect::new(t.x + x, t.y, w, 34);
            if x == 170 {
                s.blend_rounded_rect(Rect::new(r.x - 1, r.y - 7, 48, 48), 24, 0xff507fb8);
                s.stroke_rounded_rect(Rect::new(r.x - 1, r.y - 7, 48, 48), 24, 1, 0xffe9f7ff);
                s.blend_rounded_rect(Rect::new(r.x + 3, r.y - 3, 40, 40), 20, 0xffacd2e8);
                s.blend_rounded_rect(Rect::new(r.x + 5, r.y - 2, 36, 18), 9, 0x99ffffff);
                s.stroke_rounded_rect(Rect::new(r.x + 3, r.y - 3, 40, 40), 20, 1, 0xff355c9e);
            } else if matches!(x, 120 | 220) {
                // The transport wings share the circular play button's glass rim.
                let wing = Rect::new(r.x, r.y + 3, r.width, 28);
                s.blend_rounded_rect(
                    wing,
                    5,
                    if app.hot == Some(r) {
                        0xaa6acfff
                    } else {
                        0x887dbfdf
                    },
                );
                s.stroke_rounded_rect(wing, 5, 1, 0xffd9f7ff);
                s.fill_rect(wing.x + 3, wing.y + 2, wing.width - 6, 1, 0xbbffffff);
            } else if app.hot == Some(r) {
                aero::glass_button(s, r, true);
            }
            let size = if x == 170 { 30 } else { 22 };
            system_icons::draw(
                s,
                Rect::new(
                    r.x + (w as i32 - size as i32) / 2,
                    r.y + (34 - size as i32) / 2,
                    size,
                    size,
                ),
                g,
            );
        }
        for x in [62, 354] {
            s.fill_rect(t.x + x, t.y + 8, 1, 18, 0x7795bacd);
            s.fill_rect(t.x + x + 1, t.y + 8, 1, 18, 0x99ffffff);
        }
        for n in 0..3 {
            s.fill_rect(
                t.x + 48 + n,
                t.y + 15 + n,
                (5 - n * 2) as u32,
                1,
                0xff244864,
            );
        }
        system_icons::draw(
            s,
            Rect::new(b.x + b.width as i32 - 29, b.y + 4, 18, 18),
            Glyph::Help,
        );
        if v.pages > 1 {
            text(
                s,
                &format!("<   {} / {}   >", v.page + 1, v.pages),
                Rect::new(b.x + 8, b.y + b.height as i32 - 78, 140, 20),
                0xff223344,
            );
        }
    }
    if let Some(menu) = v.menu.filter(|m| *m != 6) {
        let items = menu_items(menu);
        let menu_rect = menu_bounds(b, v, menu);
        let (x, y) = (menu_rect.x, menu_rect.y);
        aero::menu_surface(
            s,
            Rect::new(x - 2, y - 2, menu_rect.width + 4, menu_rect.height + 4),
        );
        for (i, (label, _)) in items.iter().enumerate() {
            let r = Rect::new(x, y + i as i32 * 26, menu_rect.width, 26);
            if app.hot == Some(r) || (app.hot.is_none() && i == v.menu_selection) {
                aero::menu_item(s, r, true);
            }
            text(
                s,
                label,
                Rect::new(x + 12, r.y + 5, r.width.saturating_sub(24), 20),
                0xff111111,
            );
        }
    }
    if v.menu == Some(6) {
        let r = menu_bounds(b, v, 6);
        aero::menu_surface(s, r);
        let track = Rect::new(r.x + 21, r.y + 20, 3, 137);
        s.fill_rect(track.x, track.y, track.width, track.height, 0xff91989e);
        s.fill_rect(track.x + 1, track.y, 1, track.height, 0xfffafafa);
        let zoom = if v.fit { v.scale(area) } else { v.zoom };
        let at = ZOOM_STOPS
            .iter()
            .enumerate()
            .min_by_key(|(_, value)| value.abs_diff(zoom))
            .map_or(0, |(i, _)| i);
        let thumb = Rect::new(
            r.x + 13,
            r.y + 151 - (at * 136 / (ZOOM_STOPS.len() - 1)) as i32,
            19,
            11,
        );
        aero::glass_button(s, thumb, v.zoom_grab);
        text(s, "+", Rect::new(r.x + 18, r.y + 2, 16, 16), 0xff303030);
        text(s, "-", Rect::new(r.x + 18, r.y + 157, 16, 16), 0xff303030);
    }
    if !v.message.is_empty() || v.delete_confirm {
        let r = Rect::new(
            b.x + (b.width as i32 - 430) / 2,
            b.y + (b.height as i32 - 205) / 2,
            430,
            205,
        );
        s.fill_rect(r.x, r.y, r.width, r.height, 0xfff0f0f0);
        s.stroke_rounded_rect(r, 0, 1, 0xff718492);
        text(
            s,
            if v.delete_confirm {
                "Are you sure you want to move this picture to the Recycle Bin?"
            } else {
                &v.message
            },
            Rect::new(r.x + 16, r.y + 16, 398, 140),
            0xff222222,
        );
        for btn in buttons(app, b) {
            aero::glass_button(s, btn.bounds, app.hot == Some(btn.bounds));
            text(
                s,
                &btn.label,
                Rect::new(btn.bounds.x + 20, btn.bounds.y + 5, 60, 20),
                0xff111111,
            );
        }
    }
    if v.print.is_some() && v.message.is_empty() {
        printing::draw(app, v, s, b, text);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn keyboard_menus_select_commands_instead_of_navigating_the_picture() {
        let mut app = ApplicationSession::new(1, super::super::AppId::PhotoViewer, 0);
        key(&mut app, input::ALT_FILE);
        key(&mut app, input::DOWN);
        let AppState::PhotoViewer(v) = &app.state else {
            panic!()
        };
        assert_eq!(v.menu, Some(0));
        assert_eq!(v.menu_selection, 1);
        key(&mut app, input::RIGHT);
        assert_eq!(key(&mut app, 13), AppAction::None);
        let AppState::PhotoViewer(v) = &app.state else {
            panic!()
        };
        // Enter invokes Print, not an image navigation request.
        assert!(v.menu.is_none());
        key(&mut app, input::ALT_FILE);
        key(&mut app, input::END);
        assert_eq!(key(&mut app, 13), AppAction::RequestClose);
    }

    #[test]
    fn zoom_slider_drag_captures_and_clamps_until_release() {
        let mut app = ApplicationSession::new(1, super::super::AppId::PhotoViewer, 0);
        let AppState::PhotoViewer(v) = &mut app.state else {
            panic!()
        };
        v.image = Some(Surface::new(900, 600));
        let b = Rect::new(10, 20, 900, 650);
        execute(&mut app, Command::ZoomMenu, b);
        let AppState::PhotoViewer(v) = &app.state else {
            panic!()
        };
        let r = menu_bounds(b, v, 6);
        pointer(&mut app, b, r.x + 22, r.y + 20, true, true, false);
        let AppState::PhotoViewer(v) = &app.state else {
            panic!()
        };
        assert!(v.captures_pointer());
        assert_eq!(v.zoom, 1600);
        pointer(
            &mut app,
            b,
            r.x + 22,
            b.y + b.height as i32 + 100,
            false,
            true,
            false,
        );
        let AppState::PhotoViewer(v) = &app.state else {
            panic!()
        };
        assert_eq!(v.zoom, 10);
        pointer(
            &mut app,
            b,
            r.x + 22,
            b.y + b.height as i32 + 100,
            false,
            false,
            false,
        );
        let AppState::PhotoViewer(v) = &app.state else {
            panic!()
        };
        assert!(!v.captures_pointer());
        key(&mut app, input::HOME);
        let AppState::PhotoViewer(v) = &app.state else {
            panic!()
        };
        assert_eq!(v.zoom, 1600);
        assert_eq!(v.menu, Some(6));
        key(&mut app, 27);
        let AppState::PhotoViewer(v) = &app.state else {
            panic!()
        };
        assert!(v.menu.is_none());
    }
    #[test]
    fn navigation_wraps_and_failed_load_preserves_the_picture() {
        let mut v = Viewer::default();
        let mut image = Surface::new(2, 1);
        image.fill_rect(0, 0, 2, 1, 0xff123456);
        v.load(
            2,
            "a.bmp".into(),
            &codec::encode("a.bmp", &image).unwrap(),
            alloc::vec![1, 2, 3],
            0,
        )
        .unwrap();
        assert_eq!(v.neighbor(-1), Some(1));
        assert_eq!(v.neighbor(2), Some(1));
        let old = v.clone();
        assert!(v
            .load(3, "bad.jpg".into(), b"broken", alloc::vec![3], 0)
            .is_err());
        assert!(v == old);
    }
    #[test]
    fn slideshow_uses_elapsed_time_instead_of_pointer_messages() {
        let mut v = Viewer {
            image: Some(Surface::new(1, 1)),
            slideshow: true,
            ..Viewer::default()
        };
        assert!(!v.tick(100));
        assert!(!v.tick(5099));
        assert!(v.tick(5100));
        assert!(!v.tick(5100));
    }
}
