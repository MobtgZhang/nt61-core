//! Windows Media Player 12 library / Now Playing shell with bounded Rust audio playback.
//! WAV PCM and MPEG Layer III are supported; unavailable codecs and device features report errors.
pub mod audio;
pub mod icon;
pub mod playlist;
use super::{
    view::{Button, Command as ViewCommand},
    AppAction, AppId, AppState, ApplicationSession,
};
use crate::{
    graphics::{Rect, Surface},
    platform::services,
};
use alloc::{boxed::Box, format, string::String, vec::Vec};
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Track {
    pub id: u32,
    pub name: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    Load(u32),
    Refresh,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Open,
    Play,
    Stop,
    Previous,
    Next,
    Shuffle,
    Repeat,
    Mute,
    Volume(u8),
    Seek(u16),
    Mode,
    Select(u32),
    Queue(u32),
    Remove,
    Clear,
    Search,
    Library,
    Tab(u8),
    Menu,
    Refresh,
    NewPlaylist,
    SavePlaylist,
    Stream,
    CloseNotice,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Player {
    pub library: Vec<Track>,
    pub playlist: Vec<Track>,
    pub playlist_name: String,
    pub playlist_file: Option<u32>,
    pub selected: Option<u32>,
    pub current: Option<u32>,
    pub stream: Option<Box<audio::Stream>>,
    pub playing: bool,
    pub now_playing: bool,
    pub show_list: bool,
    pub shuffle: bool,
    pub repeat: bool,
    pub volume: u8,
    pub muted: bool,
    pub search: String,
    pub search_focus: bool,
    pub menu: bool,
    pub tab: u8,
    pub error: String,
    pub position: u64,
    next_poll: u64,
    last_draw: u64,
    pub pending: Option<Request>,
    random: u64,
}
impl Default for Player {
    fn default() -> Self {
        Self {
            library: Vec::new(),
            playlist: Vec::new(),
            playlist_name: "Untitled playlist.wpl".into(),
            playlist_file: None,
            selected: None,
            current: None,
            stream: None,
            playing: false,
            now_playing: false,
            show_list: false,
            shuffle: false,
            repeat: false,
            volume: 50,
            muted: false,
            search: String::new(),
            search_focus: false,
            menu: false,
            tab: 0,
            error: String::new(),
            position: 0,
            next_poll: 0,
            last_draw: 0,
            pending: None,
            random: 0x61_7601,
        }
    }
}
impl Player {
    pub fn load(&mut self, id: u32, stream: audio::Stream) {
        self.current = Some(id);
        self.selected = Some(id);
        self.stream = Some(Box::new(stream));
        self.position = 0;
        self.playing = true;
        self.error.clear();
        self.next_poll = 0;
        if !self.playlist.iter().any(|t| t.id == id) {
            if let Some(t) = self.library.iter().find(|t| t.id == id) {
                self.playlist.push(t.clone());
            }
        }
    }
    fn control(&mut self, action: &str) {
        if let Err(status) = services::media(action, &[]) {
            self.fail(status);
        }
    }
    fn fail(&mut self, status: u32) {
        self.playing = false;
        self.error = if status == nt61_abi::desktop_bridge::STATUS_NOT_SUPPORTED {
            "Windows Media Player cannot play the file because there is a problem with your sound device. No compatible playback device is available.".into()
        } else {
            format!("Windows Media Player encountered an audio output error (0x{status:08X}).")
        };
    }
    fn output_volume(&mut self) {
        if let Err(s) = services::media("volume", &[self.volume, u8::from(self.muted)]) {
            self.fail(s);
        }
    }
    pub fn start_output(&mut self) {
        self.control("reset");
        self.output_volume();
    }
    pub fn seek(&mut self, permille: u16) {
        if let Some(s) = &mut self.stream {
            let frame = s.length * u64::from(permille.min(1000)) / 1000;
            s.seek(frame);
            self.position = frame;
            self.control("reset");
            if !self.playing {
                self.control("pause");
            }
        }
    }
    pub fn adjacent(&mut self, forward: bool, automatic: bool) -> Option<u32> {
        if self.playlist.is_empty() {
            return None;
        }
        let current = self
            .playlist
            .iter()
            .position(|t| Some(t.id) == self.current)
            .unwrap_or(0);
        let n = if self.shuffle && forward && self.playlist.len() > 1 {
            self.random ^= self.random << 13;
            self.random ^= self.random >> 7;
            self.random ^= self.random << 17;
            (current + 1 + self.random as usize % (self.playlist.len() - 1)) % self.playlist.len()
        } else if forward {
            if automatic && current + 1 == self.playlist.len() && !self.repeat {
                return None;
            }
            (current + 1) % self.playlist.len()
        } else {
            (current + self.playlist.len() - 1) % self.playlist.len()
        };
        Some(self.playlist[n].id)
    }
    /// Queue at most 100 ms per poll. Repaint only at 4 Hz while playback continues.
    pub fn poll(&mut self, now: u64) -> bool {
        if !self.playing || self.stream.is_none() || now < self.next_poll {
            return false;
        }
        self.next_poll = now.saturating_add(20);
        let status = match services::media("status", &[]) {
            Ok(s) => s,
            Err(s) => {
                self.fail(s);
                return true;
            }
        };
        let queued = u64::from(status[0]) / 2;
        self.position = self
            .stream
            .as_ref()
            .map_or(0, |s| s.position.saturating_sub(queued));
        if queued < 9600 {
            let s = self.stream.as_mut().expect("checked stream");
            let previous = s.position;
            let pcm = s.read(4800);
            if pcm.is_empty() {
                if queued == 0 {
                    self.playing = false;
                    if let Some(id) = self.adjacent(true, true) {
                        self.pending = Some(Request::Load(id));
                    }
                    return true;
                }
            } else {
                let bytes: Vec<_> = pcm.into_iter().flat_map(i16::to_le_bytes).collect();
                if let Err(status) = services::media("write", &bytes) {
                    if let Some(s) = &mut self.stream {
                        s.seek(previous);
                    }
                    self.fail(status);
                    return true;
                }
            }
        }
        if now.saturating_sub(self.last_draw) >= 250 {
            self.last_draw = now;
            true
        } else {
            false
        }
    }
    pub fn name(&self) -> &str {
        self.library
            .iter()
            .chain(self.playlist.iter())
            .find(|t| Some(t.id) == self.current)
            .map_or("", |t| t.name.as_str())
    }
    fn visible(&self) -> impl Iterator<Item = &Track> {
        self.library.iter().filter(|t| {
            self.search.is_empty()
                || t.name
                    .to_ascii_lowercase()
                    .contains(&self.search.to_ascii_lowercase())
        })
    }
}
fn player(app: &ApplicationSession) -> &Player {
    let AppState::MediaPlayer(p) = &app.state else {
        unreachable!()
    };
    p
}
fn player_mut(app: &mut ApplicationSession) -> &mut Player {
    let AppState::MediaPlayer(p) = &mut app.state else {
        unreachable!()
    };
    p
}
fn add(out: &mut Vec<Button>, r: Rect, label: impl Into<String>, c: Command) {
    out.push(Button {
        bounds: r,
        label: label.into(),
        command: ViewCommand::Media(c),
    });
}
fn seek_rect(b: Rect) -> Rect {
    Rect::new(
        b.x + 16,
        b.y + b.height as i32 - 68,
        b.width.saturating_sub(32),
        14,
    )
}
fn controls(b: Rect) -> (i32, i32) {
    (b.x + (b.width as i32 - 310) / 2, b.y + b.height as i32 - 49)
}
pub fn buttons(app: &ApplicationSession, b: Rect) -> Vec<Button> {
    let p = player(app);
    let mut out = Vec::new();
    let (x, y) = controls(b);
    for (dx, w, label, c) in [
        (0, 30, "", Command::Shuffle),
        (31, 30, "", Command::Repeat),
        (68, 28, "", Command::Stop),
        (99, 32, "", Command::Previous),
        (136, 44, "", Command::Play),
        (185, 32, "", Command::Next),
        (225, 28, "", Command::Mute),
    ] {
        add(&mut out, Rect::new(x + dx, y, w, 38), label, c);
    }
    for i in 0..=10 {
        add(
            &mut out,
            Rect::new(x + 260 + i * 5, y + 10, 5, 20),
            "",
            Command::Volume(i as u8 * 10),
        );
    }
    let r = seek_rect(b);
    for i in 0..100 {
        add(
            &mut out,
            Rect::new(
                r.x + (r.width * i / 100) as i32,
                r.y,
                (r.width / 100 + 1).max(1),
                r.height,
            ),
            "",
            Command::Seek(i as u16 * 10),
        );
    }
    add(
        &mut out,
        Rect::new(b.x + b.width as i32 - 32, y + 8, 24, 24),
        "",
        Command::Mode,
    );
    if !p.now_playing {
        add(
            &mut out,
            Rect::new(b.x + 10, b.y + 4, 30, 26),
            "<",
            Command::Library,
        );
        for (i, name) in ["Play", "Burn", "Sync"].iter().enumerate() {
            add(
                &mut out,
                Rect::new(b.x + b.width as i32 - 225 + i as i32 * 73, b.y + 3, 70, 29),
                *name,
                Command::Tab(i as u8),
            );
        }
        add(
            &mut out,
            Rect::new(b.x + 8, b.y + 36, 86, 25),
            "Organize",
            Command::Menu,
        );
        add(
            &mut out,
            Rect::new(b.x + 100, b.y + 36, 70, 25),
            "Stream",
            Command::Stream,
        );
        add(
            &mut out,
            Rect::new(b.x + 176, b.y + 36, 130, 25),
            "Create playlist",
            Command::NewPlaylist,
        );
        add(
            &mut out,
            Rect::new(b.x + b.width as i32 - 216, b.y + 37, 205, 24),
            format!(
                "{}",
                if p.search.is_empty() {
                    "Search"
                } else {
                    &p.search
                }
            ),
            Command::Search,
        );
        for (i, name) in [
            "Library",
            "Playlists",
            "Music",
            "    Artist",
            "    Album",
            "    Genre",
            "Videos",
            "Pictures",
            "Other Libraries",
        ]
        .iter()
        .enumerate()
        {
            add(
                &mut out,
                Rect::new(b.x + 9, b.y + 78 + i as i32 * 27, 130, 25),
                *name,
                if i <= 5 {
                    Command::Library
                } else {
                    Command::Tab(3)
                },
            );
        }
        let center = b.width.saturating_sub(if p.show_list { 330 } else { 175 });
        for (i, t) in p
            .visible()
            .take((b.height.saturating_sub(205) / 27) as usize)
            .enumerate()
        {
            add(
                &mut out,
                Rect::new(b.x + 155, b.y + 109 + i as i32 * 27, center, 26),
                &t.name,
                Command::Select(t.id),
            );
        }
        if p.show_list {
            add(
                &mut out,
                Rect::new(b.x + b.width as i32 - 172, b.y + 72, 82, 26),
                "Save list",
                Command::SavePlaylist,
            );
            add(
                &mut out,
                Rect::new(b.x + b.width as i32 - 88, b.y + 72, 82, 26),
                "Clear list",
                Command::Clear,
            );
            for (i, t) in p
                .playlist
                .iter()
                .take((b.height.saturating_sub(220) / 25) as usize)
                .enumerate()
            {
                add(
                    &mut out,
                    Rect::new(
                        b.x + b.width as i32 - 165,
                        b.y + 115 + i as i32 * 25,
                        154,
                        24,
                    ),
                    &t.name,
                    Command::Queue(t.id),
                );
            }
            add(
                &mut out,
                Rect::new(
                    b.x + b.width as i32 - 165,
                    b.y + b.height as i32 - 112,
                    154,
                    24,
                ),
                "Remove selected",
                Command::Remove,
            );
        }
    } else {
        add(
            &mut out,
            Rect::new(b.x + 12, b.y + 8, 135, 28),
            "Switch to Library",
            Command::Mode,
        );
        add(
            &mut out,
            Rect::new(b.x + 12, b.y + 42, 92, 24),
            "Open...",
            Command::Open,
        );
    }
    if p.menu {
        add(
            &mut out,
            Rect::new(b.x + 8, b.y + 64, 190, 27),
            "Open...    Ctrl+O",
            Command::Open,
        );
        add(
            &mut out,
            Rect::new(b.x + 8, b.y + 91, 190, 27),
            "Refresh music library",
            Command::Refresh,
        );
        add(
            &mut out,
            Rect::new(b.x + 8, b.y + 118, 190, 27),
            "Switch to Now Playing",
            Command::Mode,
        );
    }
    if !p.error.is_empty() {
        add(
            &mut out,
            Rect::new(
                b.x + b.width as i32 - 110,
                b.y + b.height as i32 / 2 + 38,
                80,
                26,
            ),
            "OK",
            Command::CloseNotice,
        );
    }
    out
}
pub fn execute(app: &mut ApplicationSession, c: Command) -> AppAction {
    let p = player_mut(app);
    if !p.error.is_empty() && c != Command::CloseNotice {
        return AppAction::None;
    }
    if c != Command::Menu {
        p.menu = false;
    }
    p.search_focus = false;
    match c {
        Command::Open=>return AppAction::Open,
        Command::SavePlaylist=>return AppAction::SaveAs,
        Command::NewPlaylist=>{p.show_list=true;p.playlist.clear();p.playlist_file=None;p.playlist_name="Untitled playlist.wpl".into();p.selected=None;},
        Command::Stream=>p.error="Media streaming and home media sharing are not available. Use Open to play a local WAV or MP3 file.".into(),
        Command::Refresh=>return AppAction::Media(Request::Refresh),
        Command::Play=>{if p.stream.is_some(){
            if !p.playing && p.stream.as_ref().is_some_and(|s| s.position >= s.length) {
                if let Some(s)=&mut p.stream{s.seek(0);}p.position=0;p.control("reset");
            }
            p.playing=!p.playing;p.control(if p.playing{"resume"}else{"pause"});}else if let Some(id)=p.selected.or_else(||p.library.first().map(|t|t.id)){return AppAction::Media(Request::Load(id));}else{return AppAction::Open;}},
        Command::Stop=>{p.playing=false;p.control("reset");if let Some(s)=&mut p.stream{s.seek(0);}p.position=0;},
        Command::Previous=>{if p.position>3*48000{p.seek(0);}else if let Some(id)=p.adjacent(false,false){return AppAction::Media(Request::Load(id));}},
        Command::Next=>if let Some(id)=p.adjacent(true,false){return AppAction::Media(Request::Load(id));},
        Command::Select(id)=>{p.selected=Some(id);if !p.playlist.iter().any(|t|t.id==id){if let Some(t)=p.library.iter().find(|t|t.id==id){p.playlist.push(t.clone());}}return AppAction::Media(Request::Load(id));},
        Command::Queue(id)=>{p.selected=Some(id);return AppAction::Media(Request::Load(id));},
        Command::Remove=>{if let Some(id)=p.selected{p.playlist.retain(|t|t.id!=id);}},
        Command::Clear=>p.playlist.clear(),
        Command::Shuffle=>p.shuffle=!p.shuffle,Command::Repeat=>p.repeat=!p.repeat,
        Command::Mute=>{p.muted=!p.muted;p.output_volume();},Command::Volume(v)=>{p.volume=v.min(100);p.output_volume();},
        Command::Seek(v)=>p.seek(v),Command::Mode=>p.now_playing=!p.now_playing,
        Command::Search=>p.search_focus=true,Command::Library=>{p.tab=0;p.search_focus=false;},
        Command::Tab(t)=>{if t!=0{p.error=match t{1=>"Burning audio CDs requires optical recording support, which is not available.",2=>"No supported portable synchronization device is connected.",_=>"This media type is not supported by the installed Rust codecs. WAV PCM and MP3 audio are supported."}.into();}else{p.tab=0;p.show_list=!p.show_list;}},
        Command::Menu=>p.menu=!p.menu,Command::CloseNotice=>p.error.clear(),
    }
    AppAction::None
}
pub fn key(app: &mut ApplicationSession, k: u8) -> AppAction {
    let p = player_mut(app);
    if p.search_focus {
        match k {
            27 | 13 => p.search_focus = false,
            8 | 127 => {
                p.search.pop();
            }
            32..=126 => p.search.push(k as char),
            _ => {}
        }
        return AppAction::None;
    }
    let c = match k {
        15 => Command::Open,
        16 | b' ' => Command::Play,
        19 => Command::Stop,
        6 => Command::Next,
        2 => Command::Previous,
        20 => Command::Repeat,
        8 => Command::Shuffle,
        27 => {
            p.menu = false;
            p.error.clear();
            return AppAction::None;
        }
        _ => return AppAction::None,
    };
    execute(app, c)
}
pub fn click(app: &mut ApplicationSession, b: Rect, x: i32, y: i32) -> AppAction {
    if let Some(Button {
        command: ViewCommand::Media(c),
        ..
    }) = buttons(app, b)
        .into_iter()
        .rev()
        .find(|v| v.bounds.contains(x, y))
    {
        execute(app, c)
    } else {
        player_mut(app).search_focus = false;
        AppAction::None
    }
}
fn rgba(f: &mut Surface, bytes: &[u8], size: u32, x: i32, y: i32) {
    for dy in 0..size {
        for dx in 0..size {
            let i = ((dy * size + dx) * 4) as usize;
            let p = &bytes[i..i + 4];
            f.blend_pixel(
                (x + dx as i32) as u32,
                (y + dy as i32) as u32,
                u32::from_be_bytes([p[3], p[0], p[1], p[2]]),
            );
        }
    }
}
fn triangle(f: &mut Surface, x: i32, y: i32, size: u32, color: u32) {
    for i in 0..size {
        let half = i / 2;
        f.fill_rect(x + i as i32, y + half as i32, 1, size - half * 2, color);
    }
}
fn circle(f: &mut Surface, x: i32, y: i32, r: i32, color: u32) {
    for dy in -r..=r {
        for dx in -r..=r {
            if dx * dx + dy * dy <= r * r {
                f.blend_pixel((x + dx) as u32, (y + dy) as u32, color);
            }
        }
    }
}
fn time(frame: u64) -> String {
    let s = frame / 48000;
    format!("{}:{:02}", s / 60, s % 60)
}
pub fn draw(
    app: &ApplicationSession,
    f: &mut Surface,
    b: Rect,
    text: impl Fn(&mut Surface, &str, Rect, u32),
) {
    let p = player(app);
    let fg = if p.now_playing {
        0xffeeeeee
    } else {
        0xff1b2a3b
    };
    f.fill_rect(
        b.x,
        b.y,
        b.width,
        b.height,
        if p.now_playing {
            0xff101418
        } else {
            0xffffffff
        },
    );
    if !p.now_playing {
        for y in 0..66 {
            let color = crate::graphics::lerp_color(0xfff8fcff, 0xffd7e6f6, y * 255 / 66);
            f.fill_rect(b.x, b.y + y as i32, b.width, 1, color);
        }
        f.fill_rect(
            b.x + 143,
            b.y + 68,
            1,
            b.height.saturating_sub(140),
            0xffdce5ed,
        );
        if p.show_list {
            f.fill_rect(
                b.x + b.width as i32 - 178,
                b.y + 68,
                1,
                b.height.saturating_sub(140),
                0xffdce5ed,
            );
        }
        text(
            f,
            "Library  >  Music  >  All music",
            Rect::new(b.x + 50, b.y + 8, b.width.saturating_sub(285), 24),
            0xff24394b,
        );
        text(
            f,
            "Title",
            Rect::new(
                b.x + 160,
                b.y + 78,
                b.width.saturating_sub(if p.show_list { 340 } else { 185 }),
                24,
            ),
            0xff345267,
        );
        if p.show_list {
            text(
                f,
                "Play list",
                Rect::new(b.x + b.width as i32 - 163, b.y + 100, 145, 20),
                0xff65778b,
            );
        }
        if p.library.is_empty() {
            text(f,"There are no items in your Music library.\nPress Ctrl+O to play a WAV or MP3 file.",Rect::new(b.x+166,b.y+145,b.width.saturating_sub(if p.show_list {355}else{195}),105),0xff67798c);
        }
    } else {
        let cx = b.x + b.width as i32 / 2;
        let cy = b.y + b.height as i32 / 2 - 25;
        f.stroke_rounded_rect(Rect::new(cx - 70, cy - 70, 140, 140), 4, 2, 0xff344352);
        super::icons::draw(f, AppId::MediaPlayer, Rect::new(cx - 32, cy - 32, 64, 64));
        text(
            f,
            p.name(),
            Rect::new(b.x + 30, cy + 90, b.width.saturating_sub(60), 40),
            fg,
        );
    }
    let bottom = b.y + b.height as i32 - 76;
    for dy in 0..76 {
        f.fill_rect(
            b.x,
            bottom + dy,
            b.width,
            1,
            crate::graphics::lerp_color(0xffeff5fb, 0xffbacbe0, dy as u32 * 255 / 76),
        );
    }
    let r = seek_rect(b);
    f.fill_rect(r.x, r.y + 5, r.width, 4, 0xff899daf);
    let length = p.stream.as_ref().map_or(0, |s| s.length);
    let progress = if length == 0 {
        0
    } else {
        (p.position.min(length) * r.width as u64 / length) as u32
    };
    f.fill_rect(r.x, r.y + 5, progress, 4, 0xff178ade);
    circle(f, r.x + progress as i32, r.y + 7, 5, 0xffe8f5ff);
    let (x, y) = controls(b);
    f.stroke_rounded_rect(Rect::new(x - 8, y + 3, 334, 31), 15, 1, 0xff9baabc);
    for dx in [98, 182] {
        let r = Rect::new(x + dx, y + 6, 39, 25);
        f.rounded_rect(r, 5, 0xffe7f0f8);
        f.stroke_rounded_rect(r, 5, 1, 0xff9aafc1);
    }
    circle(f, x + 158, y + 18, 21, 0xff618bb5);
    circle(f, x + 158, y + 17, 18, 0xff1678ca);
    circle(f, x + 158, y + 13, 15, 0x504ec4ff);
    if p.playing {
        f.fill_rect(x + 151, y + 9, 5, 17, 0xffffffff);
        f.fill_rect(x + 161, y + 9, 5, 17, 0xffffffff);
    } else {
        triangle(f, x + 152, y + 8, 17, 0xffffffff);
    }
    f.fill_rect(x + 78, y + 13, 10, 10, 0xff3f5c7b);
    for px in [x + 195] {
        triangle(f, px, y + 11, 13, 0xff466887);
        f.fill_rect(px + 14, y + 11, 3, 13, 0xff466887);
    }
    // Reverse the previous-track glyph.
    for i in 0..13 {
        let half = i / 2;
        f.fill_rect(
            x + 119 - i,
            y + 11 + half,
            1,
            (13 - half * 2) as u32,
            0xff466887,
        );
    }
    f.fill_rect(x + 103, y + 11, 3, 13, 0xff466887);
    let shuffle = if p.shuffle { 0xff006bcc } else { 0xff66788a };
    for i in 0..11 {
        f.fill_rect(x + 5 + i, y + 12 + i, 2, 2, shuffle);
        f.fill_rect(x + 5 + i, y + 23 - i, 2, 2, shuffle);
    }
    triangle(f, x + 15, y + 9, 7, shuffle);
    triangle(f, x + 15, y + 19, 7, shuffle);
    let repeat = if p.repeat { 0xff006bcc } else { 0xff66788a };
    f.stroke_rounded_rect(Rect::new(x + 38, y + 12, 15, 13), 5, 2, repeat);
    f.fill_rect(x + 40, y + 10, 6, 5, 0xffdbe6f2);
    triangle(f, x + 41, y + 9, 7, repeat);
    // Speaker and sound waves use fixed geometry, independent of font glyph coverage.
    f.fill_rect(x + 230, y + 15, 4, 7, 0xff345773);
    for i in 0..5 {
        f.fill_rect(x + 234 + i, y + 14 - i, 1, (9 + i * 2) as u32, 0xff345773);
    }
    if p.muted {
        for i in 0..7 {
            f.fill_rect(x + 242 + i, y + 15 + i, 1, 1, 0xffae3232);
            f.fill_rect(x + 242 + i, y + 21 - i, 1, 1, 0xffae3232);
        }
    } else {
        for i in 0..9 {
            let dx = if i < 4 { i / 2 } else { (8 - i) / 2 };
            f.fill_rect(x + 243 + dx, y + 14 + i, 1, 1, 0xff345773);
        }
    }
    f.fill_rect(x + 260, y + 19, 55, 3, 0xff7a91aa);
    circle(
        f,
        x + 260 + i32::from(p.volume) * 55 / 100,
        y + 20,
        5,
        0xff3185bd,
    );
    text(
        f,
        "▣",
        Rect::new(b.x + b.width as i32 - 30, y + 8, 24, 25),
        0xff355a7c,
    );
    text(
        f,
        &time(p.position),
        Rect::new(b.x + 12, y + 11, 60, 22),
        0xff3e536b,
    );
    text(
        f,
        &time(length),
        Rect::new(b.x + b.width as i32 - 90, y + 11, 56, 22),
        0xff3e536b,
    );
    for btn in buttons(app, b) {
        let ViewCommand::Media(ref c) = btn.command else {
            continue;
        };
        if matches!(c,Command::Play|Command::Stop|Command::Previous|Command::Next|Command::Shuffle|Command::Repeat|Command::Mute|Command::Volume(_)|Command::Seek(_)|Command::Mode if btn.label.is_empty())
        {
            continue;
        }
        if p.menu && btn.bounds.y >= b.y + 64 && btn.bounds.x == b.x + 8 {
            crate::compositor::aero::menu_surface(f, btn.bounds);
        }
        if app.hot == Some(btn.bounds)
            || matches!(c,Command::Select(id)|Command::Queue(id) if Some(*id)==p.current)
        {
            crate::compositor::aero::menu_item(f, btn.bounds, true);
        }
        if matches!(c, Command::Search) {
            f.stroke_rounded_rect(
                btn.bounds,
                2,
                1,
                if p.search_focus {
                    0xff4488b9
                } else {
                    0xffa5b6c8
                },
            );
        }
        if matches!(c, Command::Library) && btn.bounds.y >= b.y + 78 {
            let index = (btn.bounds.y - b.y - 78) / 27;
            let icon: &[u8] = match index {
                0 => include_bytes!("../explorer/resources/shell/libraries-16.rgba"),
                2..=5 => include_bytes!("../explorer/resources/shell/music-16.rgba"),
                6 => include_bytes!("../explorer/resources/shell/videos-16.rgba"),
                7 => include_bytes!("../explorer/resources/shell/pictures-16.rgba"),
                _ => include_bytes!("../explorer/resources/shell/folder-16.rgba"),
            };
            rgba(f, icon, 16, btn.bounds.x, btn.bounds.y + 4);
        }
        text(
            f,
            &btn.label,
            Rect::new(
                btn.bounds.x
                    + if matches!(c, Command::Library) && btn.bounds.y >= b.y + 78 {
                        22
                    } else {
                        5
                    },
                btn.bounds.y + 4,
                btn.bounds.width.saturating_sub(10),
                btn.bounds.height.saturating_sub(4),
            ),
            fg,
        );
    }
    if !p.error.is_empty() {
        let r = Rect::new(
            b.x + 20,
            b.y + b.height as i32 / 2 - 65,
            b.width.saturating_sub(40),
            145,
        );
        crate::compositor::aero::shadow(f, r, 8);
        f.fill_rect(r.x, r.y, r.width, r.height, 0xfff0f0f0);
        text(
            f,
            "Windows Media Player",
            Rect::new(r.x + 14, r.y + 8, r.width - 28, 24),
            0xff173e64,
        );
        text(
            f,
            &p.error,
            Rect::new(r.x + 14, r.y + 37, r.width - 28, 67),
            0xff111111,
        );
        let r = Rect::new(
            b.x + b.width as i32 - 110,
            b.y + b.height as i32 / 2 + 38,
            80,
            26,
        );
        f.stroke_rounded_rect(r, 2, 1, 0xff707070);
        text(f, "OK", Rect::new(r.x + 29, r.y + 4, 45, 20), 0xff111111);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn queue_navigation_repeat_and_shuffle() {
        let mut p = Player::default();
        p.playlist = (1..=3)
            .map(|id| Track {
                id,
                name: format!("{id}.mp3"),
            })
            .collect();
        p.current = Some(3);
        assert_eq!(p.adjacent(true, true), None);
        p.repeat = true;
        assert_eq!(p.adjacent(true, true), Some(1));
        p.shuffle = true;
        for _ in 0..20 {
            assert_ne!(p.adjacent(true, false), Some(3));
        }
    }
    #[test]
    fn media_identity_and_controls() {
        let mut a = ApplicationSession::new(1, AppId::MediaPlayer, 0);
        assert_eq!(key(&mut a, 15), AppAction::Open);
        execute(&mut a, Command::Repeat);
        assert!(player(&a).repeat);
        execute(&mut a, Command::Tab(1));
        assert!(player(&a).error.contains("optical"));
        assert_eq!(execute(&mut a, Command::Play), AppAction::None);
        execute(&mut a, Command::CloseNotice);
        assert_eq!(execute(&mut a, Command::Play), AppAction::Open);
    }
}
