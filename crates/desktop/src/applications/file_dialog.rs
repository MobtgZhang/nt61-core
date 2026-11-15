//! Common file picker backed by the same VFS as Explorer. No sample file list.
use super::{document::Encoding, explorer::input};
use crate::{
    file::{FileSystem, NodeKind},
    graphics::Rect,
};
use alloc::{format, string::String, vec::Vec};
mod layout;
mod render;
pub use layout::{bounds, list_bounds};
pub use render::draw;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Open,
    SaveAs,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    Text,
    RichText,
    Documents,
    Bitmap,
    All,
}
impl Filter {
    fn accepts(self, name: &str) -> bool {
        let name = name.to_ascii_lowercase();
        match self {
            Self::Text => name.ends_with(".txt"),
            Self::RichText => name.ends_with(".rtf"),
            Self::Documents => [".rtf", ".docx", ".doc", ".txt"]
                .iter()
                .any(|e| name.ends_with(e)),
            Self::Bitmap => name.ends_with(".bmp"),
            Self::All => true,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Text => "Text Documents (*.txt)",
            Self::RichText => "Rich Text (*.rtf)",
            Self::Documents => "Documents (*.rtf;*.docx;*.doc;*.txt)",
            Self::Bitmap => "Bitmap (*.bmp)",
            Self::All => "All Files (*.*)",
        }
    }
    fn extension(self) -> &'static str {
        match self {
            Self::Text => ".txt",
            Self::RichText | Self::Documents => ".rtf",
            Self::Bitmap => ".bmp",
            Self::All => "",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub id: u32,
    pub name: String,
    pub directory: bool,
    pub size: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    Name,
    Address,
    List,
    NewFolder,
    Encoding,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileDialog {
    pub mode: Mode,
    pub directory: u32,
    pub address: String,
    pub name: String,
    pub filter: Filter,
    default_filter: Filter,
    pub entries: Vec<Entry>,
    pub selected: Option<usize>,
    pub offset: usize,
    scroll_grab: Option<i32>,
    pub visible_rows: usize,
    pub focus: Focus,
    pub status: String,
    pub overwrite: Option<u32>,
    pub folder_name: String,
    select_all: bool,
    pub text_encoding: bool,
    pub encoding: Option<Encoding>,
    history: Vec<u32>,
    history_index: usize,
    /// Cached namespace targets, resolved outside the paint path.
    pub locations: Vec<(&'static str, u32)>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Result {
    None,
    Cancel,
    Open(u32),
    Save {
        parent: u32,
        name: String,
        existing: Option<u32>,
    },
}
impl FileDialog {
    pub fn new(
        files: &mut FileSystem,
        mode: Mode,
        current: Option<u32>,
        name: &str,
        filter: Filter,
    ) -> Self {
        let directory = current
            .and_then(|id| files.node(id).map(|n| n.parent))
            .filter(|id| *id != 0)
            .or_else(|| files.resolve("C:\\Users\\Administrator\\Documents"))
            .unwrap_or(files.root_id());
        let mut dialog = Self {
            mode,
            directory,
            address: String::new(),
            name: name.into(),
            filter,
            default_filter: filter,
            entries: Vec::new(),
            selected: None,
            offset: 0,
            scroll_grab: None,
            visible_rows: 6,
            focus: Focus::Name,
            status: String::new(),
            overwrite: None,
            folder_name: String::new(),
            select_all: true,
            text_encoding: false,
            encoding: None,
            history: alloc::vec![directory],
            history_index: 0,
            locations: Vec::new(),
        };
        for (label, path) in [
            ("Desktop", "C:\\Users\\Administrator\\Desktop"),
            ("Downloads", "C:\\Users\\Administrator\\Downloads"),
            ("Documents", "C:\\Users\\Administrator\\Documents"),
            ("Pictures", "C:\\Users\\Administrator\\Pictures"),
            ("Music", "C:\\Users\\Administrator\\Music"),
        ] {
            if let Some(id) = files.resolve(path) {
                dialog.locations.push((label, id));
            }
        }
        dialog.locations.push(("Local Disk (C:)", files.root_id()));
        dialog.refresh(files);
        dialog
    }
    pub fn refresh(&mut self, files: &mut FileSystem) {
        self.entries.clear();
        self.selected = None;
        self.offset = 0;
        self.address = files.path(self.directory).unwrap_or_default();
        if let Err(status) = files.refresh_directory(self.directory) {
            self.status = format!("Cannot read folder (0x{status:08X})");
            return;
        }
        self.entries = files
            .children(self.directory)
            .iter()
            .filter(|n| n.kind == NodeKind::Directory || self.filter.accepts(&n.name))
            .map(|n| Entry {
                id: n.id,
                name: n.name.clone(),
                directory: n.kind == NodeKind::Directory,
                size: n.size.unwrap_or(n.contents.len() as u64),
            })
            .collect();
        self.entries.sort_by(|a, b| {
            b.directory.cmp(&a.directory).then_with(|| {
                a.name
                    .to_ascii_lowercase()
                    .cmp(&b.name.to_ascii_lowercase())
            })
        });
        self.status.clear();
    }
    fn navigate(&mut self, files: &mut FileSystem, id: u32) {
        if files
            .node(id)
            .is_some_and(|n| n.kind == NodeKind::Directory)
        {
            if id != self.directory {
                self.history.truncate(self.history_index + 1);
                self.history.push(id);
                self.history_index = self.history.len() - 1;
            }
            self.directory = id;
            self.refresh(files);
        } else {
            self.status = "The folder does not exist.".into();
        }
    }
    pub fn can_back(&self) -> bool {
        self.history_index > 0
    }
    pub fn can_forward(&self) -> bool {
        self.history_index + 1 < self.history.len()
    }
    fn history_step(&mut self, files: &mut FileSystem, forward: bool) {
        let next = if forward {
            self.history_index
                .checked_add(1)
                .filter(|i| *i < self.history.len())
        } else {
            self.history_index.checked_sub(1)
        };
        if let Some(index) = next {
            self.history_index = index;
            self.directory = self.history[index];
            self.refresh(files);
        }
    }
    pub fn submit(&mut self, files: &mut FileSystem) -> Result {
        if self.focus == Focus::NewFolder {
            let id = files.mkdir(self.directory, self.folder_name.trim());
            if id == 0 {
                self.status = format!(
                    "Cannot create folder (0x{:08X})",
                    files.last_error.unwrap_or(0)
                );
            } else {
                self.focus = Focus::Name;
                self.refresh(files);
            }
            return Result::None;
        }
        if self.focus == Focus::Address {
            if let Some(id) = files.resolve(&if self.address.as_bytes().get(1) == Some(&b':') {
                self.address.clone()
            } else {
                format!(
                    "{}\\{}",
                    files.path(self.directory).unwrap_or_default(),
                    self.address.trim()
                )
            }) {
                self.navigate(files, id);
            } else {
                self.status = "The folder does not exist.".into();
            }
            return Result::None;
        }
        if self.focus == Focus::List {
            if let Some(entry) = self.selected.and_then(|i| self.entries.get(i)) {
                if entry.directory {
                    let id = entry.id;
                    self.navigate(files, id);
                    return Result::None;
                }
                self.name = entry.name.clone();
            }
        }
        let name = self.name.trim();
        if name.is_empty() {
            self.status = "Enter a file name.".into();
            return Result::None;
        }
        // Filename input is relative to the last successfully opened directory,
        // never an unsubmitted edit in the address field.
        let base = files.path(self.directory).unwrap_or_default();
        let mut path = if name.as_bytes().get(1) == Some(&b':') {
            name.into()
        } else if name.starts_with(['\\', '/']) {
            format!("C:{name}")
        } else {
            format!("{}\\{name}", base.trim_end_matches('\\'))
        };
        if let Some(id) = files.resolve(&path) {
            if files
                .node(id)
                .is_some_and(|n| n.kind == NodeKind::Directory)
            {
                self.navigate(files, id);
                return Result::None;
            }
            if self.mode == Mode::Open {
                return Result::Open(id);
            }
        }
        if self.mode == Mode::SaveAs
            && files.resolve(&path).is_none()
            && !name
                .rsplit(['\\', '/'])
                .next()
                .unwrap_or(name)
                .contains('.')
        {
            path.push_str(self.filter.extension());
        }
        let existing = files.resolve(&path);
        if self.mode == Mode::Open {
            self.status = "File not found. Check the file name.".into();
            return Result::None;
        }
        let Some((parent, name)) = path.rsplit_once(['\\', '/']) else {
            return Result::None;
        };
        let parent = if parent.ends_with(':') {
            format!("{parent}\\")
        } else {
            parent.into()
        };
        let Some(parent) = files.resolve(&parent).filter(|id| {
            files
                .node(*id)
                .is_some_and(|n| n.kind == NodeKind::Directory)
        }) else {
            self.status = "The destination folder does not exist.".into();
            return Result::None;
        };
        if !crate::file::valid_component(name) {
            self.status = "The file name is invalid.".into();
            return Result::None;
        }
        if let Some(id) = existing {
            if self.overwrite != Some(id) {
                self.overwrite = Some(id);
                self.status = format!("{name} already exists. Replace it?");
                return Result::None;
            }
        }
        Result::Save {
            parent,
            name: name.into(),
            existing,
        }
    }
    pub fn cycle_encoding(&mut self) {
        self.encoding = match self.encoding {
            None => Some(Encoding::Ansi),
            Some(Encoding::Ansi) => Some(Encoding::Utf16Le),
            Some(Encoding::Utf16Le) => Some(Encoding::Utf16Be),
            Some(Encoding::Utf16Be) => Some(Encoding::Utf8Bom),
            Some(Encoding::Utf8Bom) => Some(Encoding::Utf8),
            Some(Encoding::Utf8) => {
                if self.mode == Mode::Open {
                    None
                } else {
                    Some(Encoding::Ansi)
                }
            }
        };
    }
    pub fn key(&mut self, files: &mut FileSystem, key: u8) -> Result {
        if key == 27 {
            if self.overwrite.take().is_some() {
                self.status.clear();
                return Result::None;
            }
            return Result::Cancel;
        }
        if self.overwrite.is_some() {
            return match key {
                b'y' | b'Y' | 13 => self.submit(files),
                b'n' | b'N' => {
                    self.overwrite = None;
                    self.status.clear();
                    Result::None
                }
                _ => Result::None,
            };
        }
        match key {
            9 => {
                self.focus = match self.focus {
                    Focus::Name if self.text_encoding => Focus::Encoding,
                    Focus::Encoding => Focus::Address,
                    Focus::Name => Focus::Address,
                    Focus::Address => Focus::List,
                    _ => Focus::Name,
                };
                self.select_all = true;
            }
            1 => self.select_all = true,
            b' ' | input::LEFT | input::RIGHT if self.focus == Focus::Encoding => {
                self.cycle_encoding()
            }
            13 | 10 => return self.submit(files),
            input::UP | input::DOWN | input::PAGE_DOWN | input::PAGE_UP => {
                self.focus = Focus::List;
                let count = self.entries.len();
                if count != 0 {
                    let i = match key {
                        input::UP => self.selected.unwrap_or(0).saturating_sub(1),
                        input::PAGE_UP => {
                            self.selected.unwrap_or(0).saturating_sub(self.visible_rows)
                        }
                        input::PAGE_DOWN => self
                            .selected
                            .unwrap_or(0)
                            .saturating_add(self.visible_rows)
                            .min(count - 1),
                        _ => self.selected.map_or(0, |i| (i + 1).min(count - 1)),
                    };
                    self.selected = Some(i);
                    if i < self.offset {
                        self.offset = i;
                    }
                    if i >= self.offset + self.visible_rows {
                        self.offset = i + 1 - self.visible_rows;
                    }
                    if !self.entries[i].directory {
                        self.name = self.entries[i].name.clone();
                    }
                }
            }
            8 | 0x20..=0x7e if self.focus != Focus::List && self.focus != Focus::Encoding => {
                let field = match self.focus {
                    Focus::Address => &mut self.address,
                    Focus::NewFolder => &mut self.folder_name,
                    _ => &mut self.name,
                };
                if self.select_all {
                    field.clear();
                    self.select_all = false;
                }
                if key == 8 {
                    field.pop();
                } else if field.len() < 1000 {
                    field.push(key as char);
                }
                self.overwrite = None;
            }
            _ => {}
        }
        Result::None
    }
    pub fn captures_pointer(&self) -> bool {
        self.scroll_grab.is_some()
    }

    pub fn wheel(&mut self, body: Rect, delta: i32) -> bool {
        if self.overwrite.is_some() {
            return false;
        }
        let page = (layout::Layout::new(body).list.height / 24).max(1) as usize;
        let maximum = self.entries.len().saturating_sub(page);
        let old = self.offset;
        self.offset = (self.offset as i64 - i64::from(delta) * 3).clamp(0, maximum as i64) as usize;
        old != self.offset
    }

    pub fn pointer(
        &mut self,
        files: &mut FileSystem,
        body: Rect,
        x: i32,
        y: i32,
        down: bool,
        held: bool,
    ) -> (bool, Result) {
        let l = layout::Layout::new(body);
        let page = (l.list.height / 24).max(1) as usize;
        if self.overwrite.is_none() {
            if let Some(bar) = super::shared::scrollbar::Scrollbar::new(
                l.scroll,
                self.entries.len(),
                page,
                self.offset,
            ) {
                if let Some(grab) = self.scroll_grab {
                    if !held && !down {
                        self.scroll_grab = None;
                        return (true, Result::None);
                    }
                    let old = self.offset;
                    self.offset = bar.drag_offset(y, grab);
                    return (old != self.offset, Result::None);
                }
                if down && bar.bounds.contains(x, y) {
                    if bar.thumb.contains(x, y) {
                        self.scroll_grab = Some(y - bar.thumb.y);
                    } else {
                        let direction = bar.arrow(y).unwrap_or(y > bar.thumb.y);
                        let step = if bar.arrow(y).is_some() { 1 } else { page };
                        self.offset = if direction {
                            self.offset.saturating_add(step).min(bar.maximum)
                        } else {
                            self.offset.saturating_sub(step)
                        };
                    }
                    return (true, Result::None);
                }
            }
        }
        if !held {
            self.scroll_grab = None;
        }
        if down {
            (true, self.click(files, body, x, y))
        } else {
            (false, Result::None)
        }
    }

    pub fn click(&mut self, files: &mut FileSystem, body: Rect, x: i32, y: i32) -> Result {
        let l = layout::Layout::new(body);
        self.visible_rows = (l.list.height / 24).max(1) as usize;
        if self.overwrite.is_some() {
            if l.accept.contains(x, y) {
                return self.submit(files);
            }
            if l.cancel.contains(x, y) {
                self.overwrite = None;
                self.status.clear();
            }
            return Result::None;
        }
        if l.back.contains(x, y) || l.forward.contains(x, y) {
            self.history_step(files, l.forward.contains(x, y));
            return Result::None;
        }
        if l.address.contains(x, y) {
            self.focus = Focus::Address;
            self.select_all = true;
            return Result::None;
        }
        if l.new_folder.contains(x, y) {
            self.focus = Focus::NewFolder;
            self.folder_name = "New folder".into();
            self.select_all = true;
            return Result::None;
        }
        if l.navigation.contains(x, y) {
            if y >= l.navigation.y + 30 {
                if let Some((_, id)) = self
                    .locations
                    .get(((y - l.navigation.y - 30) / 26) as usize)
                {
                    self.navigate(files, *id);
                }
            }
            return Result::None;
        }
        if l.list.contains(x, y) {
            let index = self.offset + ((y - l.list.y) / 24) as usize;
            if let Some(entry) = self.entries.get(index) {
                let activate = self.selected == Some(index);
                self.selected = Some(index);
                self.focus = Focus::List;
                if !entry.directory {
                    self.name = entry.name.clone();
                }
                if activate {
                    return self.submit(files);
                }
            }
            return Result::None;
        }
        if l.name.contains(x, y) {
            self.focus = Focus::Name;
            self.select_all = true;
        }
        if l.filter.contains(x, y) {
            self.filter = if self.filter == Filter::All {
                self.default_filter
            } else {
                Filter::All
            };
            self.refresh(files);
        }
        if self.text_encoding && l.encoding.contains(x, y) {
            self.focus = Focus::Encoding;
            self.cycle_encoding();
        }
        if l.accept.contains(x, y) {
            self.focus = Focus::Name;
            return self.submit(files);
        }
        if l.cancel.contains(x, y) {
            return Result::Cancel;
        }
        Result::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dialog_wheel_and_drag_keep_filename_and_selection() {
        let mut fs = FileSystem::new();
        for i in 0..100 {
            fs.touch(1, &format!("file-{i:03}.txt"), b"x");
        }
        let mut d = FileDialog::new(&mut fs, Mode::Open, None, "keep.txt", Filter::All);
        let b = Rect::new(0, 0, 700, 460);
        let l = layout::Layout::new(b);
        assert!(d.wheel(b, -1));
        assert_eq!(d.offset, 3);
        let bar = super::super::shared::scrollbar::Scrollbar::new(
            l.scroll,
            d.entries.len(),
            (l.list.height / 24) as usize,
            d.offset,
        )
        .unwrap();
        d.pointer(&mut fs, b, bar.thumb.x + 2, bar.thumb.y + 2, true, true);
        assert!(d.captures_pointer());
        d.pointer(&mut fs, b, -30, 1000, false, true);
        assert_eq!(d.offset, bar.maximum);
        d.pointer(&mut fs, b, -30, 1000, false, false);
        assert!(!d.captures_pointer());
        assert_eq!(d.name, "keep.txt");
        assert_eq!(d.selected, None);
    }

    #[test]
    fn navigation_filters_folders_and_absolute_names_use_the_real_tree() {
        let mut fs = FileSystem::default();
        let dir = fs.mkdir(1, "pictures");
        let image = fs.touch(dir, "one.bmp", b"bmp");
        fs.touch(dir, "hidden.txt", b"text");
        let mut d = FileDialog::new(&mut fs, Mode::Open, None, "", Filter::Bitmap);
        d.name = "pictures".into();
        assert_eq!(d.submit(&mut fs), Result::None);
        assert_eq!(d.directory, dir);
        assert_eq!(d.entries.len(), 1);
        assert_eq!(d.entries[0].id, image);
        d.name = "one.bmp".into();
        assert_eq!(d.submit(&mut fs), Result::Open(image));
        d.focus = Focus::NewFolder;
        d.folder_name = "new folder".into();
        d.submit(&mut fs);
        assert!(fs.resolve("C:\\pictures\\new folder").is_some());
        d.focus = Focus::Name;
        d.name = "C:\\pictures\\missing.bmp".into();
        assert_eq!(d.submit(&mut fs), Result::None);
        assert!(d.status.contains("not found"));
    }
    #[test]
    fn filename_uses_navigated_folder_and_invalid_names_never_write() {
        let mut fs = FileSystem::default();
        let mut d = FileDialog::new(&mut fs, Mode::SaveAs, None, "", Filter::Text);
        d.address = "C:\\not-submitted".into();
        d.name = "new".into();
        assert_eq!(
            d.submit(&mut fs),
            Result::Save {
                parent: 1,
                name: "new.txt".into(),
                existing: None
            }
        );
        d.name = "bad?.txt".into();
        assert_eq!(d.submit(&mut fs), Result::None);
        assert!(!d.status.is_empty());
        assert!(fs.children(1).is_empty());
    }
}
