//! Desktop Background: installed pictures, live preview and profile settings.
use crate::{
    file::{FileSystem, NodeKind},
    graphics::{Rect, Surface},
    platform::services,
};
use alloc::{format, string::String, vec::Vec};
use zune_jpeg::{
    zune_core::{colorspace::ColorSpace, options::DecoderOptions},
    JpegDecoder,
};

pub const DEFAULT_PATH: &str = "C:\\Windows\\Web\\Wallpaper\\Windows\\img0.jpg";
pub const POSITIONS: &[&str] = &["Fill", "Fit", "Stretch", "Tile", "Center"];
const PAGE_SIZE: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Preference {
    pub path: String,
    pub position: u8,
}
impl Default for Preference {
    fn default() -> Self {
        Self {
            path: DEFAULT_PATH.into(),
            position: 0,
        }
    }
}
impl Preference {
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let (&position, path) = bytes.split_first()?;
        let path = core::str::from_utf8(path).ok()?;
        if position > 4 || path.is_empty() || path.len() > 260 || path.contains('\0') {
            return None;
        }
        Some(Self {
            path: path.into(),
            position,
        })
    }
    pub fn encode(&self) -> Vec<u8> {
        let mut b = alloc::vec![self.position];
        b.extend_from_slice(self.path.as_bytes());
        b
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picture {
    pub path: String,
    pub group: String,
    pub thumbnail: Option<Surface>,
    pub failed: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Settings {
    pub current: Preference,
    pub saved: Preference,
    pub pictures: Vec<Picture>,
    pub background_page: bool,
    pub first: usize,
    pub position_open: bool,
    pub revision: u64,
    pub message: String,
    pub theme_thumbnail: Option<Surface>,
}
impl Settings {
    pub fn load(&mut self) {
        match services::session("wallpaper-get", &[]) {
            Ok(bytes) => match Preference::decode(&bytes) {
                Some(p) => {
                    self.saved = p.clone();
                    self.current = p;
                    self.revision += 1;
                    self.message.clear();
                }
                None => self.message = "Cannot read the desktop background setting.".into(),
            },
            Err(s) => self.message = format!("Cannot load desktop background (0x{s:08X})."),
        }
    }
    pub fn open(&mut self, files: &mut FileSystem) {
        self.background_page = false;
        self.position_open = false;
        self.current = self.saved.clone();
        self.theme_thumbnail = files
            .read_bytes(&self.current.path)
            .ok()
            .and_then(|b| decode_jpeg(&b))
            .map(|image| {
                let mut thumb = Surface::new(150, 86);
                render(&image, &mut thumb, self.current.position);
                thumb
            });
        if self.pictures.is_empty() {
            self.catalog(files, "C:\\Windows\\Web\\Wallpaper", 0);
            self.catalog(files, "C:\\Windows\\Globalization\\MCT", 0);
            self.pictures.sort_by(|a, b| a.path.cmp(&b.path));
        }
        self.first = self
            .pictures
            .iter()
            .position(|p| p.path.eq_ignore_ascii_case(&self.current.path))
            .unwrap_or(0)
            / PAGE_SIZE
            * PAGE_SIZE;
    }
    fn catalog(&mut self, files: &mut FileSystem, path: &str, depth: u8) {
        if depth > 5 || self.pictures.len() >= 512 {
            return;
        }
        let Some(id) = files.resolve(path) else {
            return;
        };
        if files.load_directory(id).is_err() {
            return;
        }
        let children: Vec<_> = files
            .iter_children(id)
            .map(|n| (n.name.clone(), n.kind))
            .collect();
        for (name, kind) in children {
            let full = format!("{path}\\{name}");
            if kind == NodeKind::Directory {
                self.catalog(files, &full, depth + 1);
            } else if name.to_ascii_lowercase().ends_with(".jpg") && self.pictures.len() < 512 {
                let group = path.rsplit('\\').next().unwrap_or(path).into();
                self.pictures.push(Picture {
                    path: full,
                    group,
                    thumbnail: None,
                    failed: false,
                });
            }
        }
    }
    /// Decode just one visible thumbnail per event-loop turn. Keep at most one
    /// page in memory instead of retaining every full-size installed JPEG.
    pub fn work(&mut self, files: &mut FileSystem) -> bool {
        if !self.background_page {
            return false;
        }
        let end = (self.first + PAGE_SIZE).min(self.pictures.len());
        let Some(index) = (self.first..end)
            .find(|&i| self.pictures[i].thumbnail.is_none() && !self.pictures[i].failed)
        else {
            return false;
        };
        let picture = &mut self.pictures[index];
        match files
            .read_bytes(&picture.path)
            .ok()
            .and_then(|b| decode_jpeg(&b))
        {
            Some(image) => {
                let mut thumb = Surface::new(112, 70);
                render(&image, &mut thumb, 0);
                picture.thumbnail = Some(thumb);
            }
            None => picture.failed = true,
        }
        true
    }
    pub fn cancel(&mut self) {
        if self.current != self.saved {
            self.current = self.saved.clone();
            self.revision += 1;
        }
        self.position_open = false;
        self.background_page = false;
    }
    fn page(&mut self, next: bool) {
        self.first = if next {
            (self.first + PAGE_SIZE)
                .min(self.pictures.len().saturating_sub(1) / PAGE_SIZE * PAGE_SIZE)
        } else {
            self.first.saturating_sub(PAGE_SIZE)
        };
        for p in &mut self.pictures {
            p.thumbnail = None;
        }
    }
    /// Returns true when Save or Cancel asks the shell to close the window.
    pub fn click(&mut self, b: Rect, x: i32, y: i32) -> bool {
        if !self.background_page {
            if background_link(b).contains(x, y) {
                self.background_page = true;
            }
            return false;
        }
        if self.position_open {
            self.position_open = false;
            for i in 0..5 {
                if position_item(b, i).contains(x, y) {
                    self.current.position = i as u8;
                    self.revision += 1;
                    return false;
                }
            }
            return false;
        }
        if cancel_button(b).contains(x, y) {
            self.cancel();
            return true;
        }
        if save_button(b).contains(x, y) {
            match services::session("wallpaper-set", &self.current.encode()) {
                Ok(_) => {
                    self.saved = self.current.clone();
                    self.message.clear();
                    return true;
                }
                Err(s) => self.message = format!("Cannot save desktop background (0x{s:08X})."),
            }
            return false;
        }
        if position_box(b).contains(x, y) {
            self.position_open = true;
            return false;
        }
        for i in 0..PAGE_SIZE {
            if picture_box(b, i).contains(x, y) {
                if let Some(p) = self.pictures.get(self.first + i) {
                    self.current.path = p.path.clone();
                    self.revision += 1;
                    self.message.clear();
                }
                return false;
            }
        }
        if previous_button(b).contains(x, y) {
            self.page(false);
        }
        if next_button(b).contains(x, y) {
            self.page(true);
        }
        false
    }
}

pub fn decode_jpeg(bytes: &[u8]) -> Option<crate::png::Image> {
    if bytes.len() > 4 * 1024 * 1024 {
        return None;
    }
    let options = DecoderOptions::default()
        .set_max_width(4096)
        .set_max_height(4096)
        .jpeg_set_max_scans(64)
        .set_use_unsafe(false)
        .jpeg_set_out_colorspace(ColorSpace::RGBA);
    let mut decoder = JpegDecoder::new_with_options(bytes, options);
    decoder.decode_headers().ok()?;
    let (w, h) = decoder.dimensions()?;
    if w.checked_mul(h)? > 8 * 1024 * 1024 {
        return None;
    }
    let pixels = decoder.decode().ok()?;
    if pixels.len() != w.checked_mul(h)?.checked_mul(4)? {
        return None;
    }
    Some(crate::png::Image {
        width: w as u32,
        height: h as u32,
        pixels,
    })
}

/// Windows 7 WallpaperStyle/TileWallpaper placement, centered on the monitor.
pub fn render(image: &crate::png::Image, target: &mut Surface, position: u8) -> bool {
    let (sw, sh, tw, th) = (
        image.width as i64,
        image.height as i64,
        target.width() as i64,
        target.height() as i64,
    );
    if sw == 0
        || sh == 0
        || tw == 0
        || th == 0
        || image.pixels.len() != sw as usize * sh as usize * 4
    {
        return false;
    }
    target.fill_rect(0, 0, tw as u32, th as u32, 0xFF000000);
    let (dw, dh) = match position {
        0 if sw * th > tw * sh => (sw * th / sh, th),
        0 => (tw, sh * tw / sw),
        1 if sw * th > tw * sh => (tw, (sh * tw / sw).max(1)),
        1 => ((sw * th / sh).max(1), th),
        2 => (tw, th),
        _ => (sw, sh),
    };
    let (ox, oy) = ((tw - dw) / 2, (th - dh) / 2);
    for y in 0..th {
        for x in 0..tw {
            let (sx, sy) = if position == 3 {
                (x % sw, y % sh)
            } else {
                if x < ox || y < oy || x >= ox + dw || y >= oy + dh {
                    continue;
                }
                ((x - ox) * sw / dw, (y - oy) * sh / dh)
            };
            let i = (sy as usize * sw as usize + sx as usize) * 4;
            target.set_pixel(
                x as u32,
                y as u32,
                0xFF000000
                    | u32::from(image.pixels[i]) << 16
                    | u32::from(image.pixels[i + 1]) << 8
                    | u32::from(image.pixels[i + 2]),
            );
        }
    }
    true
}
fn background_link(b: Rect) -> Rect {
    Rect::new(b.x + 24, b.y + b.height as i32 - 68, 180, 44)
}
fn position_box(b: Rect) -> Rect {
    Rect::new(b.x + 24, b.y + b.height as i32 - 94, 144, 26)
}
fn position_item(b: Rect, i: usize) -> Rect {
    let p = position_box(b);
    Rect::new(p.x, p.y - 5 * 24 + i as i32 * 24, p.width, 24)
}
fn save_button(b: Rect) -> Rect {
    Rect::new(
        b.x + b.width as i32 - 246,
        b.y + b.height as i32 - 38,
        130,
        26,
    )
}
fn cancel_button(b: Rect) -> Rect {
    Rect::new(
        b.x + b.width as i32 - 104,
        b.y + b.height as i32 - 38,
        80,
        26,
    )
}
fn previous_button(b: Rect) -> Rect {
    Rect::new(b.x + b.width as i32 - 98, b.y + 64, 30, 24)
}
fn next_button(b: Rect) -> Rect {
    Rect::new(b.x + b.width as i32 - 60, b.y + 64, 30, 24)
}
fn picture_box(b: Rect, i: usize) -> Rect {
    Rect::new(
        b.x + 24 + (i % 4) as i32 * 132,
        b.y + 106 + (i / 4) as i32 * 108,
        124,
        100,
    )
}
pub fn draw(
    s: &Settings,
    f: &mut Surface,
    b: Rect,
    mut text: impl FnMut(&mut Surface, &str, Rect, u32),
) {
    f.fill_rect(b.x, b.y, b.width, b.height, 0xFFFFFFFF);
    let blue = 0xFF003399;
    let black = 0xFF111111;
    if !s.background_page {
        text(
            f,
            "Change the visuals and sounds on your computer",
            Rect::new(b.x + 24, b.y + 18, b.width - 48, 28),
            blue,
        );
        text(
            f,
            "Aero Themes",
            Rect::new(b.x + 24, b.y + 62, 220, 22),
            black,
        );
        let card = Rect::new(b.x + 24, b.y + 94, 170, 128);
        f.fill_rect(card.x, card.y, card.width, card.height, 0xFFDCEBFC);
        if let Some(t) = &s.theme_thumbnail {
            f.copy_region(t, Rect::new(0, 0, 150, 86), card.x + 10, card.y + 6);
        }
        text(
            f,
            "Windows 7",
            Rect::new(card.x + 12, card.y + 98, 145, 24),
            black,
        );
        text(f, "Desktop Background", background_link(b), blue);
        text(
            f,
            "Windows 7",
            Rect::new(b.x + 24, b.y + b.height as i32 - 42, 180, 22),
            black,
        );
        text(
            f,
            &s.message,
            Rect::new(b.x + 24, b.y + 238, b.width - 48, 44),
            0xFF990000,
        );
        return;
    }
    text(
        f,
        "Choose your desktop background",
        Rect::new(b.x + 24, b.y + 16, b.width - 48, 26),
        blue,
    );
    text(
        f,
        "Picture location:   Windows Desktop Backgrounds",
        Rect::new(b.x + 24, b.y + 65, b.width - 138, 24),
        black,
    );
    for (r, label) in [(previous_button(b), "<"), (next_button(b), ">")] {
        button(f, r);
        text(f, label, Rect::new(r.x + 10, r.y + 4, 20, 20), black);
    }
    for i in 0..PAGE_SIZE {
        let Some(p) = s.pictures.get(s.first + i) else {
            break;
        };
        let r = picture_box(b, i);
        if p.path.eq_ignore_ascii_case(&s.current.path) {
            f.fill_rect(r.x, r.y, r.width, r.height, 0xFFC1DDFC);
        }
        if let Some(t) = &p.thumbnail {
            f.copy_region(t, Rect::new(0, 0, 112, 70), r.x + 6, r.y + 4);
        }
        text(
            f,
            &format!("{} / {}", p.group, p.path.rsplit('\\').next().unwrap_or("")),
            Rect::new(r.x + 5, r.y + 78, 114, 20),
            black,
        );
    }
    text(
        f,
        "Picture position:",
        Rect::new(b.x + 24, b.y + b.height as i32 - 118, 180, 22),
        black,
    );
    let p = position_box(b);
    button(f, p);
    text(
        f,
        POSITIONS
            .get(s.current.position as usize)
            .copied()
            .unwrap_or("Fill"),
        Rect::new(p.x + 8, p.y + 5, 110, 20),
        black,
    );
    text(f, "v", Rect::new(p.x + 124, p.y + 5, 16, 20), black);
    text(
        f,
        &s.message,
        Rect::new(
            b.x + 184,
            b.y + b.height as i32 - 112,
            b.width.saturating_sub(208),
            54,
        ),
        0xFF990000,
    );
    for (r, label) in [
        (save_button(b), "Save changes"),
        (cancel_button(b), "Cancel"),
    ] {
        button(f, r);
        text(
            f,
            label,
            Rect::new(r.x + 10, r.y + 5, r.width - 16, 20),
            black,
        );
    }
    if s.position_open {
        for i in 0..5 {
            let r = position_item(b, i);
            button(f, r);
            text(
                f,
                POSITIONS[i],
                Rect::new(r.x + 8, r.y + 4, r.width - 16, 20),
                black,
            );
        }
    }
}
fn button(f: &mut Surface, r: Rect) {
    f.fill_rect(r.x, r.y, r.width, r.height, 0xFF8A99A8);
    f.fill_rect(r.x + 1, r.y + 1, r.width - 2, r.height - 2, 0xFFF0F0F0);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn placement_handles_letterboxing_tiling_and_center_cropping() {
        let image = crate::png::Image {
            width: 2,
            height: 1,
            pixels: alloc::vec![255, 0, 0, 255, 0, 0, 255, 255],
        };
        let mut f = Surface::new(4, 4);
        assert!(render(&image, &mut f, 1));
        assert_eq!(f.pixel(0, 0), Some(0xFF000000));
        assert_eq!(f.pixel(0, 1), Some(0xFFFF0000));
        assert_eq!(f.pixel(3, 2), Some(0xFF0000FF));
        assert_eq!(f.pixel(0, 3), Some(0xFF000000));
        render(&image, &mut f, 3);
        assert_eq!(f.pixel(2, 3), Some(0xFFFF0000));
        assert_eq!(f.pixel(3, 3), Some(0xFF0000FF));
        render(&image, &mut f, 4);
        assert_eq!(f.pixel(1, 1), Some(0xFFFF0000));
        assert_eq!(f.pixel(2, 1), Some(0xFF0000FF));
        render(&image, &mut f, 2);
        assert_eq!(f.pixel(0, 3), Some(0xFFFF0000));
        assert_eq!(f.pixel(3, 3), Some(0xFF0000FF));
        render(&image, &mut f, 0);
        assert_ne!(f.pixel(0, 0), Some(0xFF000000));
    }
    #[test]
    fn cancel_restores_live_preview_and_leaves_saved_setting_unchanged() {
        let mut s = Settings::default();
        let saved = s.saved.clone();
        s.current.path = "C:\\Pictures\\other.jpg".into();
        s.current.position = 3;
        s.revision = 8;
        s.background_page = true;
        s.cancel();
        assert_eq!(s.current, saved);
        assert_eq!(s.saved, saved);
        assert_eq!(s.revision, 9);
        assert!(!s.background_page);
        assert!(Preference::decode(&[9, b'x']).is_none());
        assert!(Preference::decode(b"\0bad\0path").is_none());
        assert_eq!(Preference::decode(&saved.encode()), Some(saved));
    }
    #[test]
    fn installed_jpeg_catalog_decodes_with_bounded_memory() {
        fn visit(path: &std::path::Path, count: &mut usize) {
            for entry in std::fs::read_dir(path).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    visit(&path, count);
                } else if path
                    .extension()
                    .is_some_and(|v| v.eq_ignore_ascii_case("jpg"))
                {
                    let bytes = std::fs::read(&path).unwrap();
                    let image = decode_jpeg(&bytes)
                        .unwrap_or_else(|| panic!("failed to decode {}", path.display()));
                    assert!(image.pixels.chunks_exact(4).all(|p| p[3] == 255));
                    *count += 1;
                }
            }
        }
        let root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/windows7/wallpapers");
        let mut count = 0;
        visit(&root.join("Desktop"), &mut count);
        visit(&root.join("Desktop Regional"), &mut count);
        assert_eq!(count, 139);
        for bad in [b"".as_slice(), b"\xff\xd8\xff", b"\xff\xd8\xff\xd9"] {
            assert!(decode_jpeg(bad).is_none());
        }
    }
}
