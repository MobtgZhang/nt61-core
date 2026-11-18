//! Common file picker backed by the same VFS as Explorer. No sample file list.
use super::{document::Encoding, explorer::input};
use crate::{
    controls::{CommonKey, ListView, Modifiers},
    file::{FileSystem, NodeKind},
    graphics::Rect,
    widgets::popup_menu::{PopupItem, PopupKey, PopupMenu, PopupResult},
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
    Docx,
    Bitmap,
    Images,
    Media,
    Playlist,
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
            Self::Docx => name.ends_with(".docx"),
            Self::Images => super::associations::resolve(&name)==Some(super::AppId::PhotoViewer),
            Self::Bitmap => name.ends_with(".bmp"),
            Self::Media => name.ends_with(".wav") || name.ends_with(".mp3") || name.ends_with(".wpl"),
            Self::Playlist => name.ends_with(".wpl"),
            Self::All => true,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Text => "Text Documents (*.txt)",
            Self::RichText => "Rich Text (*.rtf)",
            Self::Documents => "Documents (*.rtf;*.docx;*.doc;*.txt)",
            Self::Docx => "Office Open XML Document (*.docx)",
            Self::Images => "Pictures (*.bmp;*.jpg;*.png;*.gif;*.tif;*.ico)",
            Self::Bitmap => "Bitmap (*.bmp)",
            Self::Media => "Media Files (*.wav;*.mp3;*.wpl)",
            Self::Playlist => "Windows Media Playlist (*.wpl)",
            Self::All => "All Files (*.*)",
        }
    }
    fn extension(self) -> &'static str {
        match self {
            Self::Text => ".txt",
            Self::RichText | Self::Documents => ".rtf",
            Self::Docx => ".docx",
            Self::Images => ".png",
            Self::Bitmap => ".bmp",
            Self::Media => ".wav",
            Self::Playlist => ".wpl",
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
    Filter,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PopupKind {
    Filter,
    Encoding,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileDialog {
    pub mode: Mode,
    pub directory: u32,
    pub address: String,
    pub name: String,
    pub filter: Filter,
    pub guest_filters: Vec<(String, String)>,
    pub guest_filter_index: usize,
    pub guest_extension: String,
    default_filter: Filter,
    pub entries: Vec<Entry>,
    list: ListView,
    /// Monotonic milliseconds, supplied by the application input loop.
    pub input_time: u64,
    last_click: Option<(u32, u64, i32, i32)>,
    sort_descending: bool,
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
    popup: PopupMenu,
    popup_kind: Option<PopupKind>,
    /// A filename wildcard temporarily narrows the current folder's view.
    pattern: String,
    overwrite_path: Option<String>,
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
            guest_filters: Vec::new(),
            guest_filter_index: 0,
            guest_extension: String::new(),
            default_filter: filter,
            entries: Vec::new(),
            list: ListView::new(1),
            input_time: 0,
            last_click: None,
            sort_descending: false,
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
            popup: PopupMenu::new(Vec::new()),
            popup_kind: None,
            pattern: String::new(),
            overwrite_path: None,
        };
        if mode == Mode::SaveAs && filter == Filter::Documents {
            dialog.filter = if name.to_ascii_lowercase().ends_with(".docx") {
                Filter::Docx
            } else if name.to_ascii_lowercase().ends_with(".txt") {
                Filter::Text
            } else {
                Filter::RichText
            };
        }
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
        self.list.clear();
        self.last_click = None;
        self.address = files.path(self.directory).unwrap_or_default();
        if let Err(status) = files.refresh_directory(self.directory) {
            self.status = format!("Cannot read folder (0x{status:08X})");
            return;
        }
        self.entries = files
            .children(self.directory)
            .iter()
            .filter(|n| {
                n.kind == NodeKind::Directory
                    || if self.pattern.is_empty() {
                        if let Some((_, pattern)) = self.guest_filters.get(self.guest_filter_index) {
                            pattern.split(';').any(|p|p=="*.*" || wildcard_matches(p, &n.name))
                        } else { self.filter.accepts(&n.name) }
                    } else {
                        wildcard_matches(&self.pattern, &n.name)
                    }
            })
            .map(|n| Entry {
                id: n.id,
                name: n.name.clone(),
                directory: n.kind == NodeKind::Directory,
                size: n.size.unwrap_or(n.contents.len() as u64),
            })
            .collect();
        self.entries.sort_by(|a, b| {
            b.directory.cmp(&a.directory).then_with(|| {
                let order = a.name.to_lowercase().cmp(&b.name.to_lowercase());
                if self.sort_descending {
                    order.reverse()
                } else {
                    order
                }
            })
        });
        for (i, entry) in self.entries.iter().enumerate() {
            self.list.insert(
                i,
                alloc::vec![
                    entry.name.clone(),
                    if entry.directory {
                        "D".into()
                    } else {
                        "F".into()
                    }
                ],
                entry.id as isize,
            );
        }
        self.status.clear();
    }
    fn sort_by_name(&mut self) {
        self.sort_descending = !self.sort_descending;
        let descending = self.sort_descending;
        self.entries.sort_by(|a, b| {
            b.directory.cmp(&a.directory).then_with(|| {
                let order = a.name.to_lowercase().cmp(&b.name.to_lowercase());
                if descending {
                    order.reverse()
                } else {
                    order
                }
            })
        });
        self.list.sort_by(|a, b| {
            a.cells[1].cmp(&b.cells[1]).then_with(|| {
                let order = a.cells[0].to_lowercase().cmp(&b.cells[0].to_lowercase());
                if descending {
                    order.reverse()
                } else {
                    order
                }
            })
        });
        if let Some(id) = self.list.focused_item() {
            self.list.ensure_visible(id, self.visible_rows);
        }
        self.last_click = None;
    }
    pub fn selected(&self) -> Option<usize> {
        let row = self.list.selected().next()?;
        self.entries
            .iter()
            .position(|entry| entry.id as isize == row.data)
    }
    pub fn offset(&self) -> usize {
        self.list.offset()
    }
    fn set_offset(&mut self, offset: usize, page: usize) {
        self.list.set_offset(offset, page);
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
            self.name.clear();
            self.pattern.clear();
            self.overwrite = None;
            self.overwrite_path = None;
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
            self.name.clear();
            self.pattern.clear();
            self.overwrite = None;
            self.overwrite_path = None;
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
            if let Some(entry) = self.selected().and_then(|i| self.entries.get(i)) {
                if entry.directory {
                    let id = entry.id;
                    self.navigate(files, id);
                    return Result::None;
                }
                self.name = entry.name.clone();
            }
        }
        // Explorer permits a quoted path (including spaces), but a quote in
        // the middle of a component is still invalid.
        let name = self.name.trim();
        let name = name
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .unwrap_or(name);
        if name.is_empty() {
            self.status = "Enter a file name.".into();
            return Result::None;
        }
        if name.chars().any(|c| matches!(c, '*' | '?')) && !name.contains(['\\', '/', ':']) {
            self.pattern = name.into();
            self.refresh(files);
            if self.entries.is_empty() {
                self.status = "No items match your search.".into();
            }
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
        if files.resolve(&path).is_none()
            && !name
                .rsplit(['\\', '/'])
                .next()
                .unwrap_or(name)
                .contains('.')
        {
            if self.guest_extension.is_empty() { path.push_str(self.filter.extension()); }
            else { path.push('.'); path.push_str(&self.guest_extension); }
        }
        let existing = files.resolve(&path);
        if let Some(id) = existing {
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
            if self.overwrite != Some(id) || self.overwrite_path.as_deref() != Some(path.as_str()) {
                self.overwrite = Some(id);
                self.overwrite_path = Some(path.clone());
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
    pub fn filter_label(&self) -> &str {
        self.guest_filters.get(self.guest_filter_index).map(|v|v.0.as_str()).unwrap_or(self.filter.label())
    }
    pub fn guest_directory(&mut self, files: &mut FileSystem, directory: u32) { self.navigate(files, directory); }
    fn filters(&self) -> Vec<Filter> {
        match self.default_filter {
            Filter::Documents if self.mode == Mode::SaveAs => {
                alloc::vec![Filter::RichText, Filter::Docx, Filter::Text, Filter::All]
            }
            Filter::Documents => alloc::vec![
                Filter::Documents,
                Filter::RichText,
                Filter::Docx,
                Filter::Text,
                Filter::All
            ],
            Filter::All => alloc::vec![Filter::All],
            value => alloc::vec![value, Filter::All],
        }
    }
    fn encodings(&self) -> Vec<Option<Encoding>> {
        let mut values = alloc::vec![
            Some(Encoding::Ansi),
            Some(Encoding::Utf16Le),
            Some(Encoding::Utf16Be),
            Some(Encoding::Utf8Bom),
            Some(Encoding::Utf8)
        ];
        if self.mode == Mode::Open {
            values.insert(0, None);
        }
        values
    }
    fn open_popup(&mut self, body: Rect, kind: PopupKind) {
        let layout = layout::Layout::new(body);
        let (items, selected, anchor) = match kind {
            PopupKind::Filter if !self.guest_filters.is_empty() => (
                self.guest_filters.iter().enumerate().map(|(i,v)|PopupItem::new(i as u32,&v.0)).collect(),
                self.guest_filter_index as u32, layout.filter,
            ),
            PopupKind::Filter => {
                let values = self.filters();
                let selected = values.iter().position(|v| *v == self.filter).unwrap_or(0) as u32;
                (
                    values
                        .iter()
                        .enumerate()
                        .map(|(i, v)| PopupItem::new(i as u32, v.label()))
                        .collect(),
                    selected,
                    layout.filter,
                )
            }
            PopupKind::Encoding => {
                let values = self.encodings();
                let selected = values.iter().position(|v| *v == self.encoding).unwrap_or(0) as u32;
                (
                    values
                        .iter()
                        .enumerate()
                        .map(|(i, v)| PopupItem::new(i as u32, encoding_label(*v)))
                        .collect(),
                    selected,
                    layout.encoding,
                )
            }
        };
        self.popup = PopupMenu::new(items);
        self.popup.open(anchor, body, Some(selected));
        self.popup_kind = Some(kind);
    }
    fn popup_result(&mut self, files: &mut FileSystem, result: PopupResult) {
        match result {
            PopupResult::Selected(id) => {
                match self.popup_kind {
                    Some(PopupKind::Filter) if !self.guest_filters.is_empty() => {
                        if (id as usize) < self.guest_filters.len() {
                            self.guest_filter_index=id as usize;self.pattern.clear();self.refresh(files);
                        }
                    }
                    Some(PopupKind::Filter) => {
                        if let Some(value) = self.filters().get(id as usize) {
                            if self.mode == Mode::SaveAs
                                && *value != self.filter
                                && *value != Filter::All
                            {
                                let old_extension = self.filter.extension();
                                if !old_extension.is_empty()
                                    && self.name.to_ascii_lowercase().ends_with(old_extension)
                                {
                                    self.name.truncate(self.name.len() - old_extension.len());
                                    self.name.push_str(value.extension());
                                }
                            }
                            self.filter = *value;
                            self.pattern.clear();
                            self.refresh(files);
                            self.overwrite = None;
                            self.overwrite_path = None;
                        }
                    }
                    Some(PopupKind::Encoding) => {
                        if let Some(value) = self.encodings().get(id as usize) {
                            self.encoding = *value;
                        }
                    }
                    None => {}
                }
                self.popup_kind = None;
            }
            PopupResult::Cancelled => self.popup_kind = None,
            PopupResult::None => {}
        }
    }
    /// Open an option list with the same geometry used by pointer input.
    pub fn key_at(&mut self, files: &mut FileSystem, key: u8, body: Rect) -> Result {
        if !self.popup.is_open() && matches!(key, b' ' | 13 | input::DOWN) {
            let kind = match self.focus {
                Focus::Filter => Some(PopupKind::Filter),
                Focus::Encoding => Some(PopupKind::Encoding),
                _ => None,
            };
            if let Some(kind) = kind {
                self.open_popup(body, kind);
                return Result::None;
            }
        }
        self.key(files, key)
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
        if self.popup.is_open() {
            let action = match key {
                input::UP => Some(PopupKey::Up),
                input::DOWN => Some(PopupKey::Down),
                input::HOME => Some(PopupKey::Home),
                input::END => Some(PopupKey::End),
                input::PAGE_UP => Some(PopupKey::PageUp),
                input::PAGE_DOWN => Some(PopupKey::PageDown),
                13 | 10 | b' ' => Some(PopupKey::Enter),
                27 | 9 => Some(PopupKey::Escape),
                _ => None,
            };
            if let Some(action) = action {
                let result = self.popup.key(action);
                self.popup_result(files, result);
            }
            return Result::None;
        }
        if key == 27 {
            if self.overwrite.take().is_some() {
                self.status.clear();
                self.overwrite_path = None;
                return Result::None;
            }
            return Result::Cancel;
        }
        if self.overwrite.is_some() {
            return match key {
                b'y' | b'Y' | 13 => self.submit(files),
                b'n' | b'N' => {
                    self.overwrite = None;
                    self.overwrite_path = None;
                    self.status.clear();
                    Result::None
                }
                _ => Result::None,
            };
        }
        match key {
            input::REFRESH => {
                let selected = self.selected().map(|i| self.entries[i].id);
                let offset = self.offset();
                self.refresh(files);
                if let Some(id) = selected {
                    if let Some(row) = self.list.rows().iter().find(|row| row.data == id as isize) {
                        self.list.select(row.id, Modifiers::default());
                    }
                }
                self.set_offset(offset, self.visible_rows);
            }
            8 if self.focus == Focus::List => {
                if let Some(parent) = files
                    .node(self.directory)
                    .map(|node| node.parent)
                    .filter(|id| *id != 0 && *id != self.directory)
                {
                    self.navigate(files, parent);
                }
            }
            9 => {
                self.focus = match self.focus {
                    Focus::Name => Focus::Filter,
                    Focus::Filter if self.text_encoding => Focus::Encoding,
                    Focus::Filter => Focus::Address,
                    Focus::Encoding => Focus::Address,
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
            input::UP
            | input::DOWN
            | input::PAGE_DOWN
            | input::PAGE_UP
            | input::HOME
            | input::END
                if !matches!(self.focus, Focus::Filter | Focus::Encoding) =>
            {
                self.focus = Focus::List;
                let action = match key {
                    input::UP => CommonKey::Up,
                    input::HOME => CommonKey::Home,
                    input::END => CommonKey::End,
                    input::PAGE_UP => CommonKey::PageUp,
                    input::PAGE_DOWN => CommonKey::PageDown,
                    _ => CommonKey::Down,
                };
                self.list
                    .key(action, Modifiers::default(), self.visible_rows);
                self.last_click = None;
                if let Some(entry) = self.selected().and_then(|i| self.entries.get(i)) {
                    if !entry.directory {
                        self.name = entry.name.clone();
                    }
                }
            }
            8 | 0x20..=0x7e
                if !matches!(self.focus, Focus::List | Focus::Encoding | Focus::Filter) =>
            {
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
                self.overwrite_path = None;
            }
            _ => {}
        }
        Result::None
    }
    pub fn captures_pointer(&self) -> bool {
        self.scroll_grab.is_some() || self.popup.is_open()
    }

    pub fn wheel(&mut self, body: Rect, delta: i32) -> bool {
        if self.popup.is_open() {
            self.popup.wheel(delta);
            return true;
        }
        if self.overwrite.is_some() {
            return false;
        }
        let page = (layout::Layout::new(body).list.height / 24).max(1) as usize;
        let old = self.offset();
        self.list.scroll(delta.saturating_mul(-3), page);
        old != self.offset()
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
        if self.popup.is_open() {
            let result = self.popup.pointer(x, y, down);
            self.popup_result(files, result);
            return (true, Result::None);
        }
        let l = layout::Layout::new(body);
        let page = (l.list.height / 24).max(1) as usize;
        if self.overwrite.is_none() {
            if let Some(bar) = super::shared::scrollbar::Scrollbar::new(
                l.scroll,
                self.entries.len(),
                page,
                self.offset(),
            ) {
                if let Some(grab) = self.scroll_grab {
                    if !held && !down {
                        self.scroll_grab = None;
                        return (true, Result::None);
                    }
                    let old = self.offset();
                    self.set_offset(bar.drag_offset(y, grab), page);
                    return (old != self.offset(), Result::None);
                }
                if down && bar.bounds.contains(x, y) {
                    if bar.thumb.contains(x, y) {
                        self.scroll_grab = Some(y - bar.thumb.y);
                    } else {
                        let direction = bar.arrow(y).unwrap_or(y > bar.thumb.y);
                        let step = if bar.arrow(y).is_some() { 1 } else { page };
                        let offset = if direction {
                            self.offset().saturating_add(step).min(bar.maximum)
                        } else {
                            self.offset().saturating_sub(step)
                        };
                        self.set_offset(offset, page);
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
        if self.popup.is_open() {
            let result = self.popup.pointer(x, y, true);
            self.popup_result(files, result);
            return Result::None;
        }
        let l = layout::Layout::new(body);
        self.visible_rows = (l.list.height / 24).max(1) as usize;
        if self.overwrite.is_some() {
            if l.accept.contains(x, y) {
                return self.submit(files);
            }
            if l.cancel.contains(x, y) {
                self.overwrite = None;
                self.overwrite_path = None;
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
        if l.header.contains(x, y)
            && (l.header.width < 340 || x < l.header.x + l.header.width as i32 * 2 / 3)
        {
            self.sort_by_name();
            return Result::None;
        }
        if l.list.contains(x, y) {
            let index = self.offset() + ((y - l.list.y) / 24) as usize;
            if let Some(entry) = self.entries.get(index) {
                let activate = self.last_click.is_some_and(|(id, time, px, py)| {
                    id == entry.id
                        && self.input_time >= time
                        && self.input_time - time <= 500
                        && x.abs_diff(px) <= 4
                        && y.abs_diff(py) <= 4
                });
                self.last_click = if activate {
                    None
                } else {
                    Some((entry.id, self.input_time, x, y))
                };
                if let Some(row) = self.list.rows().get(index) {
                    self.list.select(row.id, Modifiers::default());
                }
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
        self.last_click = None;
        if l.name.contains(x, y) {
            self.focus = Focus::Name;
            self.select_all = true;
        }
        if l.filter.contains(x, y) {
            self.focus = Focus::Filter;
            self.open_popup(body, PopupKind::Filter);
        }
        if self.text_encoding && l.encoding.contains(x, y) {
            self.focus = Focus::Encoding;
            self.open_popup(body, PopupKind::Encoding);
        }
        if l.accept.contains(x, y) {
            if !matches!(self.focus, Focus::List) {
                self.focus = Focus::Name;
            }
            return self.submit(files);
        }
        if l.cancel.contains(x, y) {
            return Result::Cancel;
        }
        Result::None
    }
}

fn encoding_label(value: Option<Encoding>) -> &'static str {
    match value {
        None => "Auto-detect",
        Some(Encoding::Ansi) => "ANSI",
        Some(Encoding::Utf16Le) => "Unicode",
        Some(Encoding::Utf16Be) => "Unicode big endian",
        Some(Encoding::Utf8Bom) => "UTF-8",
        Some(Encoding::Utf8) => "UTF-8 (no BOM)",
    }
}

/// DOS common-dialog wildcards are case insensitive; *.* includes extensionless names.
fn wildcard_matches(patterns: &str, name: &str) -> bool {
    let name = name.to_lowercase();
    patterns.split(';').any(|pattern| {
        let pattern = pattern.trim().to_lowercase();
        if matches!(pattern.as_str(), "*.*" | "*") {
            return true;
        }
        let pattern: Vec<_> = pattern.chars().collect();
        let name: Vec<_> = name.chars().collect();
        let (mut p, mut n, mut star, mut retry) = (0, 0, None, 0);
        while n < name.len() {
            if p < pattern.len() && (pattern[p] == '?' || pattern[p] == name[n]) {
                p += 1;
                n += 1;
            } else if p < pattern.len() && pattern[p] == '*' {
                star = Some(p);
                p += 1;
                retry = n;
            } else if let Some(index) = star {
                p = index + 1;
                retry += 1;
                n = retry;
            } else {
                return false;
            }
        }
        while p < pattern.len() && pattern[p] == '*' {
            p += 1;
        }
        p == pattern.len()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sorting_and_refresh_preserve_selected_file_identity_with_folders_first() {
        let mut fs = FileSystem::new();
        fs.mkdir(1, "z-folder");
        let first = fs.touch(1, "a.txt", b"a");
        fs.touch(1, "b.txt", b"b");
        let mut d = FileDialog::new(&mut fs, Mode::Open, None, "", Filter::Text);
        d.focus = Focus::List;
        d.key(&mut fs, input::HOME);
        d.key(&mut fs, input::DOWN);
        assert_eq!(d.entries[d.selected().unwrap()].id, first);
        d.sort_by_name();
        assert!(d.entries[0].directory);
        assert_eq!(d.entries[d.selected().unwrap()].id, first);
        assert_eq!(d.selected(), Some(2));
        d.key(&mut fs, input::REFRESH);
        assert_eq!(d.entries[d.selected().unwrap()].id, first);
        assert_eq!(d.key(&mut fs, 13), Result::Open(first));
    }
    #[test]
    fn only_nearby_timely_double_click_activates_a_list_item() {
        let mut fs = FileSystem::new();
        let file = fs.touch(1, "one.txt", b"text");
        let mut d = FileDialog::new(&mut fs, Mode::Open, None, "", Filter::Text);
        let body = Rect::new(0, 0, 640, 480);
        let list = list_bounds(body);
        d.input_time = 100;
        assert_eq!(
            d.click(&mut fs, body, list.x + 10, list.y + 10),
            Result::None
        );
        d.input_time = 900;
        assert_eq!(
            d.click(&mut fs, body, list.x + 10, list.y + 10),
            Result::None
        );
        d.input_time = 1000;
        assert_eq!(
            d.click(&mut fs, body, list.x + 30, list.y + 10),
            Result::None
        );
        d.input_time = 1100;
        assert_eq!(
            d.click(&mut fs, body, list.x + 31, list.y + 10),
            Result::Open(file)
        );
    }
    #[test]
    fn filter_popup_commits_only_on_accept_and_escape_keeps_dialog_open() {
        let mut fs = FileSystem::new();
        fs.touch(1, "one.txt", b"text");
        fs.touch(1, "two.bmp", b"bitmap");
        let mut d = FileDialog::new(&mut fs, Mode::Open, None, "keep", Filter::Text);
        let body = Rect::new(0, 0, 700, 460);
        let field = layout::Layout::new(body).filter;
        d.click(&mut fs, body, field.x + 3, field.y + 3);
        assert!(d.popup.is_open());
        assert_eq!(d.filter, Filter::Text);
        d.key(&mut fs, input::DOWN);
        assert_eq!(d.filter, Filter::Text);
        assert_eq!(d.key(&mut fs, 27), Result::None);
        assert_eq!(d.filter, Filter::Text);
        assert!(!d.popup.is_open());
        d.key_at(&mut fs, b' ', body);
        d.key(&mut fs, input::END);
        d.key(&mut fs, 13);
        assert_eq!(d.filter, Filter::All);
        assert_eq!(d.entries.len(), 2);
        assert_eq!(d.name, "keep");
    }

    #[test]
    fn encoding_popup_does_not_change_encoding_until_selection_and_consumes_outside_click() {
        let mut fs = FileSystem::new();
        let mut d = FileDialog::new(&mut fs, Mode::SaveAs, None, "new", Filter::Text);
        d.text_encoding = true;
        d.encoding = Some(Encoding::Ansi);
        let body = Rect::new(0, 0, 700, 460);
        let l = layout::Layout::new(body);
        d.click(&mut fs, body, l.encoding.x + 3, l.encoding.y + 3);
        assert!(d.popup.is_open());
        assert_eq!(d.encoding, Some(Encoding::Ansi));
        assert_eq!(
            d.click(&mut fs, body, l.accept.x + 3, l.accept.y + 3),
            Result::None
        );
        assert!(!d.popup.is_open());
        d.key_at(&mut fs, input::DOWN, body);
        d.key(&mut fs, input::END);
        d.key(&mut fs, 13);
        assert_eq!(d.encoding, Some(Encoding::Utf8));
    }

    #[test]
    fn quoted_and_extensionless_open_names_resolve_and_wildcards_filter_without_opening() {
        let mut fs = FileSystem::new();
        let id = fs.touch(1, "hello world.txt", b"hello");
        fs.touch(1, "picture.bmp", b"bitmap");
        fs.touch(1, "README", b"no extension");
        let mut d = FileDialog::new(&mut fs, Mode::Open, None, "\"hello world\"", Filter::Text);
        assert_eq!(d.submit(&mut fs), Result::Open(id));
        d.name = "*.bmp".into();
        assert_eq!(d.submit(&mut fs), Result::None);
        assert_eq!(d.entries.len(), 1);
        assert_eq!(d.entries[0].name, "picture.bmp");
        assert!(wildcard_matches("*.txt;*.BMP", "PICTURE.bmp"));
        assert!(wildcard_matches("*.*", "README"));
        assert!(wildcard_matches("文?.txt", "文档.txt"));
    }

    #[test]
    fn default_extension_directory_is_navigated_and_accept_button_uses_list_selection() {
        let mut fs = FileSystem::new();
        let folder = fs.mkdir(1, "folder.txt");
        let mut d = FileDialog::new(&mut fs, Mode::SaveAs, None, "folder", Filter::Text);
        assert_eq!(d.submit(&mut fs), Result::None);
        assert_eq!(d.directory, folder);
        assert!(d.name.is_empty());
        let child = fs.mkdir(folder, "child");
        d.refresh(&mut fs);
        d.focus = Focus::List;
        d.list.select(d.list.rows()[0].id, Modifiers::default());
        let body = Rect::new(0, 0, 700, 460);
        let accept = layout::Layout::new(body).accept;
        assert_eq!(
            d.click(&mut fs, body, accept.x + 2, accept.y + 2),
            Result::None
        );
        assert_eq!(d.directory, child);
    }
    #[test]
    fn document_save_type_changes_extension_and_overwrite_confirmation_is_path_specific() {
        let mut fs = FileSystem::new();
        fs.touch(1, "report.rtf", b"original");
        let body = Rect::new(0, 0, 700, 460);
        let mut d = FileDialog::new(&mut fs, Mode::SaveAs, None, "report.rtf", Filter::Documents);
        assert_eq!(d.filter, Filter::RichText);
        assert_eq!(d.submit(&mut fs), Result::None);
        assert!(d.overwrite.is_some());
        d.key(&mut fs, 27);
        d.focus = Focus::Filter;
        d.key_at(&mut fs, b' ', body);
        d.key(&mut fs, input::DOWN);
        d.key(&mut fs, 13);
        assert_eq!(d.filter, Filter::Docx);
        assert_eq!(d.name, "report.docx");
        assert_eq!(
            d.submit(&mut fs),
            Result::Save {
                parent: 1,
                name: "report.docx".into(),
                existing: None
            }
        );
        assert_eq!(fs.children(1).len(), 1);
    }
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
        assert_eq!(d.offset(), 3);
        let bar = super::super::shared::scrollbar::Scrollbar::new(
            l.scroll,
            d.entries.len(),
            (l.list.height / 24) as usize,
            d.offset(),
        )
        .unwrap();
        d.pointer(&mut fs, b, bar.thumb.x + 2, bar.thumb.y + 2, true, true);
        assert!(d.captures_pointer());
        d.pointer(&mut fs, b, -30, 1000, false, true);
        assert_eq!(d.offset(), bar.maximum);
        d.pointer(&mut fs, b, -30, 1000, false, false);
        assert!(!d.captures_pointer());
        assert_eq!(d.name, "keep.txt");
        assert_eq!(d.selected(), None);
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
