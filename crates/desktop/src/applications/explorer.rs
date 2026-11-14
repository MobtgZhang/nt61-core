//! Per-window shell navigation backed by the session's mounted system volume.
//! The renderer and hit testing use the same client geometry.
use crate::{
    file::{FileNode, FileSystem, NodeKind},
    graphics::Rect,
};
use alloc::{format, string::String, vec::Vec};
pub mod input;
pub mod layout;
mod render;
use layout::{Layout, ROW_HEIGHT};
pub use render::draw;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sort {
    Name,
    Type,
    Size,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Details,
    List,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edit {
    Address,
    Search,
    Rename,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Back,
    Forward,
    Address,
    Refresh,
    Search,
    Command(usize),
    Navigation(u32),
    Breadcrumb(u32),
    Row(u32),
    Column(usize),
    Scroll(bool),
    Menu(usize),
    Blank,
    ConfirmDelete(bool),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Explorer {
    pub window_id: u32,
    pub directory: u32,
    pub selected: Option<u32>,
    history: Vec<u32>,
    index: usize,
    pub status: String,
    pub address_edit: Option<String>,
    pub query: String,
    pub edit: Option<Edit>,
    pub sort: Sort,
    pub descending: bool,
    pub view: View,
    pub offset: usize,
    pub hot: Option<Hit>,
    pressed: Option<Hit>,
    last_click: Option<(u32, u64, i32, i32)>,
    pub menu: Option<Rect>,
    pub rename: String,
    pub pending_open: Option<u32>,
    /// Supplied by the platform monotonic clock; absent for untimed callers.
    pub input_time: Option<u64>,
    page_size: usize,
    decoder: input::Decoder,
    search_queue: Vec<u32>,
    search_cursor: usize,
    search_results: Vec<u32>,
    search_scanned: usize,
    search_error: Option<u32>,
    pub delete_prompt: Option<u32>,
    pub delete_accept: bool,
    delete_job: Option<DeleteJob>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DeleteJob {
    scan: Vec<(u32,bool)>,
    order: Vec<u32>,
    cursor: usize,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    None,
    Open(u32),
}
impl Explorer {
    pub fn new(window_id: u32, directory: u32) -> Self {
        Self {
            window_id,
            directory,
            selected: None,
            history: alloc::vec![directory],
            index: 0,
            status: String::new(),
            address_edit: None,
            query: String::new(),
            edit: None,
            sort: Sort::Name,
            descending: false,
            view: View::Details,
            offset: 0,
            hot: None,
            pressed: None,
            last_click: None,
            menu: None,
            rename: String::new(),
            pending_open: None,
            input_time: None,
            page_size: 8,
            decoder: input::Decoder::default(),
            search_queue: Vec::new(),
            search_cursor: 0,
            search_results: Vec::new(),
            search_scanned: 0,
            search_error: None,
            delete_prompt: None,
            delete_accept: false,
            delete_job: None,
        }
    }
    fn reset_folder(&mut self, id: u32) {
        self.directory = id;
        self.selected = None;
        self.offset = 0;
        self.last_click = None;
        self.query.clear();
        self.cancel_search();
        self.address_edit = None;
        self.edit = None;
        self.menu = None;
        self.status.clear();
    }
    fn load(&mut self, files: &mut FileSystem, id: u32) -> bool {
        if !files
            .node(id)
            .is_some_and(|n| n.kind == NodeKind::Directory && !n.deleted)
        {
            return false;
        }
        if let Err(status) = files.refresh_directory(id) {
            self.status = format!("Cannot open folder (0x{status:08X})");
            return false;
        }
        true
    }
    pub fn navigate(&mut self, files: &mut FileSystem, id: u32) -> bool {
        if !self.load(files, id) {
            return false;
        }
        if id != self.directory {
            self.history.truncate(self.index + 1);
            self.history.push(id);
            self.index = self.history.len() - 1;
        }
        self.reset_folder(id);
        true
    }
    /// Cached navigation retained for older session callers.
    pub fn back(&mut self) {
        if self.index > 0 {
            self.index -= 1;
            self.reset_folder(self.history[self.index]);
        }
    }
    pub fn can_back(&self) -> bool {
        self.index > 0
    }
    pub fn can_forward(&self) -> bool {
        self.index + 1 < self.history.len()
    }
    fn history_step(&mut self, files: &mut FileSystem, forward: bool) {
        let next = if forward {
            self.index
                .checked_add(1)
                .filter(|i| *i < self.history.len())
        } else {
            self.index.checked_sub(1)
        };
        if let Some(index) = next {
            let id = self.history[index];
            if self.load(files, id) {
                self.index = index;
                self.reset_folder(id);
            }
        }
    }
    pub fn rows<'a>(&self, files: &'a FileSystem) -> Vec<&'a FileNode> {
        let query = self.query.to_lowercase();
        let mut rows = if query.is_empty() {
            files.children(self.directory)
        } else {
            self.search_results
                .iter()
                .filter_map(|id| files.node(*id))
                .filter(|n| !n.deleted)
                .collect()
        };
        if !query.is_empty() {
            rows.retain(|n| n.name.to_lowercase().contains(&query));
        }
        rows.sort_unstable_by(|a, b| {
            let folders = (a.kind != NodeKind::Directory).cmp(&(b.kind != NodeKind::Directory));
            if folders != core::cmp::Ordering::Equal {
                return folders;
            }
            let name = || {
                a.name
                    .bytes()
                    .map(|c| c.to_ascii_lowercase())
                    .cmp(b.name.bytes().map(|c| c.to_ascii_lowercase()))
            };
            let order = match self.sort {
                Sort::Name => name(),
                Sort::Type => file_type(a).cmp(&file_type(b)).then_with(name),
                Sort::Size => file_size(a).cmp(&file_size(b)).then_with(name),
            };
            if self.descending {
                order.reverse()
            } else {
                order
            }
        });
        rows
    }
    /// Load the standard profile locations once, outside the paint/input hot path.
    pub fn prepare(&mut self, files: &mut FileSystem) {
        files.load_shell_resources();
        let error = files.last_error;
        for path in [
            "C:\\Users\\Administrator\\Desktop",
            "C:\\Users\\Administrator\\Downloads",
            "C:\\Users\\Administrator\\Documents",
            "C:\\Users\\Administrator\\Music",
            "C:\\Users\\Administrator\\Pictures",
            "C:\\Users\\Administrator\\Videos",
        ] {
            let _ = files.resolve(path);
        }
        files.last_error = error;
    }
    fn cancel_search(&mut self) {
        self.search_queue.clear();
        self.search_cursor = 0;
        self.search_results.clear();
        self.search_scanned = 0;
        self.search_error = None;
    }
    fn start_search(&mut self) {
        self.cancel_search();
        if !self.query.is_empty() {
            self.search_queue.push(self.directory);
            self.status = "Searching...".into();
        } else {
            self.status.clear();
        }
    }
    pub fn search_pending(&self) -> bool {
        self.search_cursor < self.search_queue.len()
    }
    pub fn work_pending(&self)->bool{self.delete_job.is_some()||self.search_pending()}
    fn confirm_delete(&mut self){
        if let Some(id)=self.delete_prompt.take(){
            if self.delete_accept{
                self.cancel_search();
                self.delete_job=Some(DeleteJob{scan:alloc::vec![(id,false)],order:Vec::new(),cursor:0});
                self.status="Preparing to delete... (Esc to cancel)".into();
            }
        }
    }
    /// Preflight the complete subtree before deleting anything. Each job step
    /// enumerates one directory or removes one item, yielding to input between calls.
    fn delete_step(&mut self,files:&mut FileSystem)->bool{
        let Some(mut job)=self.delete_job.take()else{return false;};
        let result=(||->Result<bool,u32>{
            if let Some((id,expanded))=job.scan.pop(){
                if expanded{job.order.push(id);}
                else{
                    if id==files.root_id(){return Err(nt61_abi::desktop_bridge::STATUS_ACCESS_DENIED);}
                    let node=files.node(id).filter(|n|!n.deleted).ok_or(nt61_abi::desktop_bridge::STATUS_NOT_FOUND)?;
                    if node.kind==NodeKind::Directory{
                        files.refresh_directory(id)?;
                        job.scan.push((id,true));
                        for node in files.iter_children(id){job.scan.push((node.id,false));}
                    }else{job.order.push(id);}
                    if job.scan.len()+job.order.len()>65536{return Err(nt61_abi::desktop_bridge::STATUS_NOT_SUPPORTED);}
                }
                return Ok(false);
            }
            if let Some(id)=job.order.get(job.cursor).copied(){files.remove(id)?;job.cursor+=1;}
            Ok(job.cursor==job.order.len())
        })();
        match result{
            Ok(true)=>{self.selected=None;self.status="Deleted permanently".into();}
            Ok(false)=>{self.status=if job.cursor==0{"Preparing to delete... (Esc to cancel)".into()}else{format!("Deleting {} of {} items (Esc to cancel)",job.cursor,job.order.len())};self.delete_job=Some(job);}
            Err(status)=>{self.status=format!("Delete stopped (0x{status:08X}); {} item(s) deleted",job.cursor);files.last_error=Some(status);}
        }
        true
    }
    /// A cancellable shell job, bounded to one provider call or 128 cached entries.
    /// This deliberately uses the existing service ABI rather than impersonating
    /// explorer7's COM scheduler or creating kernel threads from the painter.
    pub fn work(&mut self, files: &mut FileSystem) -> bool {
        if self.delete_job.is_some(){return self.delete_step(files);}
        if !self.search_pending() {
            return false;
        }
        let id = self.search_queue[self.search_cursor];
        if self.search_scanned == 0 {
            if let Err(status) = files.refresh_directory(id) {
                self.search_error = Some(status);
                self.search_cursor += 1;
                self.finish_search_status();
                return true;
            }
        }
        let query = self.query.to_lowercase();
        let mut nodes = files.iter_children(id).skip(self.search_scanned);
        for node in nodes.by_ref().take(128) {
            if node.name.to_lowercase().contains(&query) {
                self.search_results.push(node.id);
            }
            if node.kind == NodeKind::Directory && !self.search_queue.contains(&node.id) {
                self.search_queue.push(node.id);
            }
        }
        self.search_scanned += 128;
        if nodes.next().is_none() {
            self.search_cursor += 1;
            self.search_scanned = 0;
        }
        self.finish_search_status();
        true
    }
    fn finish_search_status(&mut self) {
        if !self.search_pending() {
            self.status = if let Some(error) = self.search_error {
                format!("Some folders could not be searched (0x{error:08X})")
            } else {
                String::new()
            };
        }
    }
    pub fn breadcrumbs<'a>(&self, files: &'a FileSystem, b: Rect) -> Vec<(Rect, &'a FileNode)> {
        let mut chain = Vec::new();
        let mut id = self.directory;
        while let Some(node) = files.node(id) {
            chain.push(node);
            id = node.parent;
            if id == 0 {
                break;
            }
        }
        chain.reverse();
        let mut result = Vec::new();
        let mut x = b.x + 22;
        // Keep the current folder reachable when the path is wider than the bar.
        let total: u32 = chain
            .iter()
            .map(|n| (n.name.chars().count() as u32 * 8 + 26).min(140))
            .sum();
        let mut remaining = total;
        for node in chain {
            let width = (node.name.chars().count() as u32 * 8 + 26).min(140);
            if remaining <= b.width.saturating_sub(26) || node.id == self.directory {
                let available = (b.x + b.width as i32 - x).max(0) as u32;
                result.push((
                    Rect::new(x, b.y + 1, width.min(available), b.height.saturating_sub(2)),
                    node,
                ));
                x += width as i32;
            }
            remaining = remaining.saturating_sub(width);
        }
        result
    }
    /// Resolve only real folders. Missing libraries never alias the disk root.
    pub fn navigation(&self, files: &FileSystem) -> Vec<(&'static str, Option<u32>, u32)> {
        let find = |path: &str| {
            let mut id = files.root_id();
            for part in path.split('\\') {
                id = files
                    .children(id)
                    .into_iter()
                    .find(|n| n.name.eq_ignore_ascii_case(part) && n.kind == NodeKind::Directory)?
                    .id;
            }
            Some(id)
        };
        alloc::vec![
            ("Favorites", None, 0),
            ("Desktop", find("Users\\Administrator\\Desktop"), 1),
            ("Downloads", find("Users\\Administrator\\Downloads"), 1),
            ("Libraries", None, 0),
            (
                "Documents",
                find("Users\\Administrator\\Documents").or_else(|| find("Documents")),
                1
            ),
            ("Music", find("Users\\Administrator\\Music"), 1),
            ("Pictures", find("Users\\Administrator\\Pictures"), 1),
            ("Videos", find("Users\\Administrator\\Videos"), 1),
            ("Computer", Some(files.root_id()), 0),
            ("Local Disk (C:)", Some(files.root_id()), 1)
        ]
    }
    /// Poll one visible folder after input has been handled. Compare metadata
    /// only: cloning file bodies here makes multi-window input unresponsive.
    pub fn poll_directory(&mut self, files: &mut FileSystem) -> bool {
        if self.delete_prompt.is_some()||self.delete_job.is_some()||!files.is_mounted() || self.edit.is_some() || self.menu.is_some() || !self.query.is_empty(){return false;}
        let before:Vec<_>=files.iter_children(self.directory)
            .map(|n|(n.id,n.name.clone(),n.kind,n.size)).collect();
        if let Err(error)=files.refresh_directory(self.directory){
            let message=format!("Refresh failed (0x{error:08X})");
            let changed=self.status!=message;self.status=message;return changed;
        }
        let changed=before!=files.iter_children(self.directory)
            .map(|n|(n.id,n.name.clone(),n.kind,n.size)).collect::<Vec<_>>();
        let recovered=self.status.starts_with("Refresh failed");
        if recovered {self.status.clear();}
        let missing=self.selected.is_some_and(|id| !files.node(id).is_some_and(|n|!n.deleted));
        if missing {self.selected=None;self.last_click=None;}
        if changed {self.offset=self.offset.min(self.rows(files).len().saturating_sub(self.page_size));}
        changed || recovered || missing
    }

    fn refresh(&mut self, files: &mut FileSystem) {
        self.status = match files.refresh_directory(self.directory) {
            Ok(()) => String::new(),
            Err(status) => format!("Refresh failed (0x{status:08X})"),
        };
        if self
            .selected
            .is_some_and(|id| !files.node(id).is_some_and(|n| !n.deleted))
        {
            self.selected = None;
        }
        if !self.query.is_empty() {
            self.start_search();
        }
        self.offset = self
            .offset
            .min(self.rows(files).len().saturating_sub(self.page_size));
    }
    fn open(&mut self, files: &mut FileSystem, id: u32) -> Action {
        if files
            .node(id)
            .is_some_and(|n| n.kind == NodeKind::Directory)
        {
            self.navigate(files, id);
            Action::None
        } else {
            Action::Open(id)
        }
    }
    fn begin_address(&mut self, files: &FileSystem) {
        self.edit = Some(Edit::Address);
        self.address_edit = Some(files.path(self.directory).unwrap_or_default());
    }
    fn command(&mut self, files: &mut FileSystem, command: usize) -> Action {
        files.last_error = None;
        match command {
            0 => {
                if let Some(id) = self.selected {
                    return self.open(files, id);
                }
            }
            1 => {
                if let Some(id) = self.selected {
                    files.copy_selected_to_clipboard(&[id]);
                    self.status = "Copied to clipboard".into();
                }
            }
            2 => {
                let pasted = files.paste_from_clipboard(self.directory);
                self.selected = pasted.first().copied();
                self.status = format!("{} item(s) pasted", pasted.len());
            }
            3 => {
                let mut count = 1;
                let name = loop {
                    let n = if count == 1 {
                        String::from("New folder")
                    } else {
                        format!("New folder ({count})")
                    };
                    if !files
                        .children(self.directory)
                        .iter()
                        .any(|child| child.name.eq_ignore_ascii_case(&n))
                    {
                        break n;
                    }
                    count += 1;
                };
                let id = files.mkdir(self.directory, &name);
                self.selected = (id != 0).then_some(id);
                self.status = String::new();
            }
            4 => {
                if let Some(id) = self.selected {
                    self.rename = files.node(id).map(|n| n.name.clone()).unwrap_or_default();
                    self.edit = Some(Edit::Rename);
                }
            }
            5 => {
                if let Some(id) = self.selected {
                    self.delete_prompt=Some(id);self.delete_accept=false;self.menu=None;self.edit=None;
                }
            }
            6 => self.refresh(files),
            7 => {
                let mut count=1;
                let name=loop {
                    let name=if count==1 {String::from("New Text Document.txt")} else {format!("New Text Document ({count}).txt")};
                    if !files.iter_children(self.directory).any(|n|n.name.eq_ignore_ascii_case(&name)){break name;}
                    count+=1;
                };
                let id=files.touch(self.directory,&name,&[]);
                self.selected=(id!=0).then_some(id);
                if id!=0 {self.rename=name;self.edit=Some(Edit::Rename);self.status.clear();}
            },
            _ => {}
        }
        if let Some(status) = files.last_error {
            self.status = format!("Operation failed (0x{status:08X})");
        }
        Action::None
    }
    pub fn stream_key(&mut self, files: &mut FileSystem, key: u8) -> bool {
        match self.decoder.feed(key, self.input_time.unwrap_or(0)) {
            Some(key) => self.key(files, key),
            None => true,
        }
    }
    pub fn flush_input(&mut self, files: &mut FileSystem) -> bool {
        self.decoder
            .expire(self.input_time.unwrap_or(0))
            .is_some_and(|key| self.key(files, key))
    }
    pub fn key(&mut self, files: &mut FileSystem, key: u8) -> bool {
        if self.delete_job.is_some(){
            if key==27{self.delete_job=None;self.status="Deletion cancelled; completed deletions remain applied".into();}
            return true;
        }
        if self.delete_prompt.is_some(){
            match key{
                27=>{self.delete_prompt=None;},
                9|input::LEFT|input::RIGHT=>self.delete_accept=!self.delete_accept,
                13|10=>self.confirm_delete(),
                _=>{},
            }
            return true;
        }
        if key == 12 {
            self.begin_address(files);
            return true;
        }
        if key == 6 {
            self.edit = Some(Edit::Search);
            return true;
        }
        if let Some(edit) = self.edit {
            if key == 27 {
                self.edit = None;
                self.address_edit = None;
                if edit == Edit::Search {
                    self.query.clear();
                    self.cancel_search();
                    self.status.clear();
                }
                return true;
            }
            if matches!(key, 13 | 10) {
                match edit {
                    Edit::Address => {
                        let mut path = self.address_edit.take().unwrap_or_default();
                        if !path.contains(':') && !path.starts_with(['\\', '/']) {
                            path = format!(
                                "{}\\{path}",
                                files.path(self.directory).unwrap_or_default()
                            );
                        }
                        if let Some(id) = files.resolve(&path) {
                            if let Action::Open(id) = self.open(files, id) {
                                self.pending_open = Some(id);
                            }
                        } else {
                            self.status = "The system cannot find the path specified.".into();
                        }
                    }
                    Edit::Search => {}
                    Edit::Rename => {
                        if let Some(id) = self.selected {
                            files.last_error = None;
                            if !files.rename(id, &self.rename) {
                                self.status = format!(
                                    "Rename failed (0x{:08X})",
                                    files.last_error.unwrap_or(
                                        nt61_abi::desktop_bridge::STATUS_INVALID_PARAMETER
                                    )
                                );
                            }
                        }
                    }
                }
                self.edit = None;
                return true;
            }
            let value = match edit {
                Edit::Address => self.address_edit.get_or_insert_with(String::new),
                Edit::Search => &mut self.query,
                Edit::Rename => &mut self.rename,
            };
            match key {
                1 => value.clear(),
                8 => {
                    value.pop();
                }
                0x20..=0x7e => {
                    if value.len() < 1024 {
                        value.push(key as char);
                    }
                }
                _ => {}
            }
            if edit == Edit::Search {
                self.offset = 0;
                self.selected = None;
                self.last_click = None;
                self.start_search();
            }
            return true;
        }
        if key == 27 {
            self.menu = None;
            self.query.clear();
            self.cancel_search();
            self.status.clear();
            self.last_click = None;
            return true;
        }
        let action = match key {
            8 => {
                self.history_step(files, false);
                Action::None
            }
            18 | input::REFRESH => {
                self.refresh(files);
                Action::None
            }
            3 => self.command(files, 1),
            22 => self.command(files, 2),
            13 | 10 => self
                .selected
                .map(|id| self.open(files, id))
                .unwrap_or(Action::None),
            input::LEFT => {
                self.history_step(files, false);
                Action::None
            }
            input::RIGHT => {
                self.history_step(files, true);
                Action::None
            }
            input::DELETE => self.command(files, 5),
            input::RENAME => self.command(files, 4),
            input::UP
            | input::DOWN
            | input::PAGE_UP
            | input::PAGE_DOWN
            | input::HOME
            | input::END => {
                let rows = self.rows(files);
                let count = rows.len();
                if count > 0 {
                    let current = self
                        .selected
                        .and_then(|id| rows.iter().position(|n| n.id == id));
                    let next = match key {
                        input::HOME => 0,
                        input::END => count - 1,
                        input::UP => current.unwrap_or(1).saturating_sub(1),
                        input::DOWN => current.map(|i| i + 1).unwrap_or(0).min(count - 1),
                        input::PAGE_UP => current.unwrap_or(0).saturating_sub(self.page_size),
                        _ => (current.unwrap_or(0) + self.page_size).min(count - 1),
                    };
                    self.selected = Some(rows[next].id);
                    if next < self.offset {
                        self.offset = next;
                    } else if next >= self.offset + self.page_size {
                        self.offset = (next + 1).saturating_sub(self.page_size);
                    }
                }
                Action::None
            }
            _ => return false,
        };
        if let Action::Open(id) = action {
            self.pending_open = Some(id);
        }
        true
    }
    fn hit(&self, files: &FileSystem, l: Layout, x: i32, y: i32) -> Hit {
        if self.delete_prompt.is_some(){
            let (_,yes,no)=layout::delete_dialog(l.body);
            return if yes.contains(x,y){Hit::ConfirmDelete(true)}else if no.contains(x,y){Hit::ConfirmDelete(false)}else{Hit::Blank};
        }
        if let Some(menu) = self.menu {
            if menu.contains(x, y) {
                let index = ((y - menu.y) / 26) as usize;
                return if (index + 1) * 26 <= menu.height as usize {
                    Hit::Menu(index)
                } else {
                    Hit::Blank
                };
            }
        }
        for (r, hit) in [
            (l.back, Hit::Back),
            (l.forward, Hit::Forward),
            (l.refresh, Hit::Refresh),
            (l.search, Hit::Search),
        ] {
            if r.contains(x, y) {
                return hit;
            }
        }
        if l.address.contains(x, y) {
            if self.edit != Some(Edit::Address) {
                for (r, node) in self.breadcrumbs(files, l.address) {
                    if r.contains(x, y) {
                        return Hit::Breadcrumb(node.id);
                    }
                }
            }
            return Hit::Address;
        }
        for i in 0..4 {
            if l.command(i).contains(x, y) {
                return Hit::Command(i);
            }
        }
        if l.navigation.contains(x, y) {
            let row = ((y - l.navigation.y - 6).max(0) / 24) as usize;
            if let Some((_, Some(id), _)) = self.navigation(files).get(row) {
                return Hit::Navigation(*id);
            }
        }
        if l.header.contains(x, y) {
            if self.view == View::List {
                return Hit::Column(0);
            }
            for i in [0, 2, 3] {
                if l.column(i).contains(x, y) {
                    return Hit::Column(i);
                }
            }
        }
        if l.scroll.contains(x, y) {
            return Hit::Scroll(y >= l.scroll.y + l.scroll.height as i32 / 2);
        }
        if l.list.contains(x, y) {
            let row = ((y - l.list.y) / ROW_HEIGHT as i32) as usize;
            if row < l.capacity() {
                if let Some(node) = self.rows(files).get(self.offset + row) {
                    return Hit::Row(node.id);
                }
            }
        }
        Hit::Blank
    }
    /// Activation happens on release over the pressed control, like shell controls.
    pub fn pointer(
        &mut self,
        files: &mut FileSystem,
        b: Rect,
        x: i32,
        y: i32,
        down: bool,
        held: bool,
        right: bool,
    ) -> (bool, Action) {
        if self.delete_job.is_some(){return (false,Action::None);}
        let l = Layout::new(b);
        self.page_size = l.capacity().max(1);
        self.offset = self
            .offset
            .min(self.rows(files).len().saturating_sub(self.page_size));
        let hit = if b.contains(x, y) {
            Some(self.hit(files, l, x, y))
        } else {
            None
        };
        let changed = self.hot != hit;
        self.hot = hit;
        if self.delete_prompt.is_some(){
            if down{self.pressed=hit;}
            else if !held&&self.pressed.take()==hit{
                if let Some(Hit::ConfirmDelete(accept))=hit{self.delete_accept=accept;self.confirm_delete();}
            }
            return (changed||down||!held,Action::None);
        }
        if right {
            if let Some(Hit::Row(id)) = hit {
                self.selected = Some(id);
            }
            let h = (8 * 26).min(b.height);
            let w = 184.min(b.width);
            self.menu = Some(Rect::new(
                x.min(b.x + b.width as i32 - w as i32).max(b.x),
                y.min(b.y + b.height as i32 - h as i32).max(b.y),
                w,
                h,
            ));
            self.pressed = None;
            return (true, Action::None);
        }
        if down {
            if self.menu.is_some() && !matches!(hit, Some(Hit::Menu(_))) {
                self.menu = None;
            }
            self.pressed = hit;
            if let Some(Hit::Row(id)) = hit {
                self.selected = Some(id);
                self.edit = None;
            }
            if hit == Some(Hit::Blank) {
                self.selected = None;
                self.edit = None;
                self.last_click = None;
            }
            return (true, Action::None);
        }
        if !held {
            if let Some(pressed) = self.pressed.take() {
                if hit == Some(pressed) {
                    let mut action = Action::None;
                    match pressed {
                        Hit::Back => self.history_step(files, false),
                        Hit::Forward => self.history_step(files, true),
                        Hit::Address => self.begin_address(files),
                        Hit::Breadcrumb(id) => {
                            if id == self.directory {
                                self.begin_address(files);
                            } else {
                                self.navigate(files, id);
                            }
                        }
                        Hit::Refresh => self.refresh(files),
                        Hit::Search => self.edit = Some(Edit::Search),
                        Hit::Navigation(id) => {
                            self.navigate(files, id);
                        }
                        Hit::Command(0) => {
                            let h = (8 * 26).min(b.height);
                            self.menu = Some(Rect::new(
                                b.x,
                                l.commands.y + (l.commands.height as i32),
                                184.min(b.width),
                                h,
                            ));
                            if let Some(r) = self.menu.as_mut() {
                                r.y = r.y.min(b.y + b.height as i32 - h as i32);
                            }
                        }
                        Hit::Command(1) => action = self.command(files, 0),
                        Hit::Command(2) => action = self.command(files, 3),
                        Hit::Command(3) => {
                            self.view = if self.view == View::Details {
                                View::List
                            } else {
                                View::Details
                            };
                        }
                        Hit::Column(column) => {
                            let sort = match column {
                                2 => Sort::Type,
                                3 => Sort::Size,
                                _ => Sort::Name,
                            };
                            self.descending = self.sort == sort && !self.descending;
                            self.sort = sort;
                            self.offset = 0;
                            self.last_click = None;
                        }
                        Hit::Scroll(down) => {
                            let maximum = self.rows(files).len().saturating_sub(self.page_size);
                            self.offset = if down {
                                (self.offset + self.page_size).min(maximum)
                            } else {
                                self.offset.saturating_sub(self.page_size)
                            };
                            self.last_click = None;
                        }
                        Hit::Menu(index) => {
                            self.menu = None;
                            action = self.command(files, index);
                        }
                        Hit::Row(id) => {
                            if let Some(now) = self.input_time {
                                if self.last_click.is_some_and(|(previous, time, px, py)| {
                                    previous == id
                                        && now >= time
                                        && now - time <= 500
                                        && (x - px).abs() <= 4
                                        && (y - py).abs() <= 4
                                }) {
                                    self.last_click = None;
                                    action = self.open(files, id);
                                } else {
                                    self.last_click = Some((id, now, x, y));
                                }
                            }
                        }
                        Hit::Blank | Hit::Command(_) | Hit::ConfirmDelete(_) => {}
                    }
                    return (true, action);
                } else {
                    self.last_click = None;
                    return (true, Action::None);
                }
            }
        }
        (changed, Action::None)
    }
    /// Untimed callers can select; opening requires Enter or a timed pointer event.
    pub fn click(&mut self, files: &mut FileSystem, b: Rect, x: i32, y: i32) -> Action {
        self.pointer(files, b, x, y, true, true, false);
        self.pointer(files, b, x, y, false, false, false).1
    }
}
pub fn file_size(node: &FileNode) -> u64 {
    node.size.unwrap_or(node.contents.len() as u64)
}
pub fn file_type(node: &FileNode) -> &'static str {
    if node.kind == NodeKind::Directory {
        return "File folder";
    }
    let ext = node.name.rsplit('.').next().unwrap_or("");
    for (extensions, kind) in [
        (&["txt", "log", "md"][..], "Text Document"),
        (&["exe"][..], "Application"),
        (&["dll"][..], "Application extension"),
        (&["sys"][..], "System file"),
        (&["png", "bmp", "jpg", "jpeg", "ico"][..], "Image"),
        (&["lnk"][..], "Shortcut"),
        (&["zip"][..], "Compressed folder"),
    ] {
        if extensions
            .iter()
            .any(|value| ext.eq_ignore_ascii_case(value))
        {
            return kind;
        }
    }
    "File"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{file::Backend, graphics::Surface};
    use nt61_abi::desktop_bridge as abi;
    fn body() -> Rect {
        Rect::new(10, 10, 640, 420)
    }
    fn point(state: &Explorer, files: &FileSystem, id: u32) -> (i32, i32) {
        let l = Layout::new(body());
        let index = state.rows(files).iter().position(|n| n.id == id).unwrap();
        let r = l.row(index - state.offset);
        (r.x + 34, r.y + 12)
    }
    fn timed(state: &mut Explorer, files: &mut FileSystem, p: (i32, i32), time: u64) -> Action {
        state.input_time = Some(time);
        state.pointer(files, body(), p.0, p.1, true, true, false);
        state
            .pointer(files, body(), p.0, p.1, false, false, false)
            .1
    }
    fn click_rect(state: &mut Explorer, files: &mut FileSystem, r: Rect) -> Action {
        state.click(files, body(), r.x + 5, r.y + 5)
    }
    #[test]
    fn repeated_selection_is_not_an_open_gesture() {
        let mut fs = FileSystem::new();
        let folder = fs.mkdir(1, "Documents");
        let mut state = Explorer::new(1, 1);
        let p = point(&state, &fs, folder);
        state.click(&mut fs, body(), p.0, p.1);
        state.click(&mut fs, body(), p.0, p.1);
        assert_eq!(state.selected, Some(folder));
        assert_eq!(state.directory, 1);
    }
    #[test]
    fn only_a_close_timed_double_click_opens_and_release_outside_cancels() {
        let mut fs = FileSystem::new();
        let folder = fs.mkdir(1, "Documents");
        let mut state = Explorer::new(1, 1);
        let p = point(&state, &fs, folder);
        timed(&mut state, &mut fs, p, 100);
        timed(&mut state, &mut fs, p, 900);
        assert_eq!(state.directory, 1);
        timed(&mut state, &mut fs, p, 1100);
        assert_eq!(state.directory, folder);
        state.back();
        state.pointer(&mut fs, body(), p.0, p.1, true, true, false);
        state.pointer(&mut fs, body(), -10, -10, false, false, false);
        assert_eq!(state.directory, 1);
    }
    #[test]
    fn windows_have_independent_history_and_new_navigation_truncates_forward() {
        let mut fs = FileSystem::new();
        let a = fs.mkdir(1, "A");
        let b = fs.mkdir(1, "B");
        let c = fs.mkdir(1, "C");
        let mut left = Explorer::new(1, 1);
        let mut right = Explorer::new(2, 1);
        left.navigate(&mut fs, a);
        left.navigate(&mut fs, b);
        right.navigate(&mut fs, c);
        left.history_step(&mut fs, false);
        assert_eq!(left.directory, a);
        assert!(left.can_forward());
        left.history_step(&mut fs, true);
        assert_eq!(left.directory, b);
        left.history_step(&mut fs, false);
        left.navigate(&mut fs, c);
        assert!(!left.can_forward());
        assert_eq!(right.directory, c);
    }
    #[test]
    fn keyboard_address_opens_real_paths_and_enter_returns_a_file_action() {
        let mut fs = FileSystem::new();
        let folder = fs.mkdir(1, "Documents");
        let file = fs.touch(folder, "note.txt", b"real bytes");
        let mut state = Explorer::new(1, 1);
        for key in b"\x0c\x01C:\\Documents\r" {
            state.key(&mut fs, *key);
        }
        assert_eq!(state.directory, folder);
        state.selected = Some(file);
        state.key(&mut fs, 13);
        assert_eq!(state.pending_open.take(), Some(file));
        for key in b"\x0c\x01..\r" {
            state.key(&mut fs, *key);
        }
        assert_eq!(state.directory, 1);
        for key in b"\x0c\x01C:\\Missing\r" {
            state.key(&mut fs, *key);
        }
        assert_eq!(state.directory, 1);
        assert!(state.status.contains("cannot find"));
    }
    #[test]
    fn search_visits_subfolders_in_bounded_batches_and_cancels_old_results() {
        let mut fs = FileSystem::new();
        let sub = fs.mkdir(1, "Sub");
        let found = fs.touch(sub, "needle.txt", b"x");
        for i in 0..300 {
            fs.touch(1, &format!("unrelated-{i}"), b"");
        }
        let mut state = Explorer::new(1, 1);
        for key in b"\x06needle" {
            state.key(&mut fs, *key);
        }
        state.work(&mut fs);
        assert!(state.search_pending());
        assert!(state.rows(&fs).is_empty());
        for _ in 0..8 {
            state.work(&mut fs);
        }
        assert!(!state.search_pending());
        assert_eq!(state.rows(&fs)[0].id, found);
        state.key(&mut fs, 1);
        for key in b"absent" {
            state.key(&mut fs, *key);
        }
        assert!(state.rows(&fs).is_empty());
        while state.work(&mut fs) {}
        assert!(state.rows(&fs).is_empty());
        state.key(&mut fs, 27);
        assert!(state.query.is_empty());
        assert!(!state.search_pending());
    }
    #[test]
    fn sorting_keeps_folders_first_and_uses_metadata_not_unloaded_contents() {
        fn provider(op: u32, path: &str, _: &[u8]) -> Result<Vec<u8>, u32> {
            if op != abi::LIST || !matches!(path, "C:" | "C:\\") {
                return Err(abi::STATUS_ACCESS_DENIED);
            }
            let mut bytes = Vec::new();
            for (name, size, flags) in [
                ("large.txt", 9000u64, 0u32),
                ("small.txt", 3, 0),
                ("z-folder", 0, abi::DIRECTORY),
            ] {
                bytes.extend_from_slice(&(name.len() as u32).to_le_bytes());
                bytes.extend_from_slice(&flags.to_le_bytes());
                bytes.extend_from_slice(&size.to_le_bytes());
                bytes.extend_from_slice(name.as_bytes());
            }
            Ok(bytes)
        }
        let fs = FileSystem::mounted(Backend(provider)).unwrap();
        let mut state = Explorer::new(1, 1);
        state.sort = Sort::Size;
        let rows = state.rows(&fs);
        assert_eq!(rows[0].name, "z-folder");
        assert_eq!(rows[1].name, "small.txt");
        assert_eq!(file_size(rows[2]), 9000);
        assert!(rows[2].contents.is_empty());
        state.descending = true;
        assert_eq!(state.rows(&fs)[1].name, "large.txt");
    }
    #[test]
    fn polling_observes_external_changes_without_losing_selection_or_file_bodies() {
        use core::sync::atomic::{AtomicUsize,Ordering};
        static GENERATION:AtomicUsize=AtomicUsize::new(0);
        fn provider(operation:u32,_:&str,_:&[u8])->Result<Vec<u8>,u32>{
            if operation!=abi::LIST{return Err(abi::STATUS_NOT_SUPPORTED);}
            let generation=GENERATION.load(Ordering::Relaxed);
            let mut out=Vec::new();
            for name in if generation==0 {&["old.txt"][..]}else{&["new.txt"][..]} {
                out.extend_from_slice(&(name.len()as u32).to_le_bytes());
                out.extend_from_slice(&0u32.to_le_bytes());out.extend_from_slice(&4u64.to_le_bytes());out.extend_from_slice(name.as_bytes());
            }Ok(out)
        }
        GENERATION.store(0,Ordering::Relaxed);
        let mut fs=FileSystem::mounted(Backend(provider)).unwrap();
        let mut state=Explorer::new(1,1);let old=fs.children(1)[0].id;
        fs.node_mut(old).unwrap().contents=b"body".to_vec();state.selected=Some(old);
        assert!(!state.poll_directory(&mut fs));assert_eq!(fs.node(old).unwrap().contents,b"body");
        GENERATION.store(1,Ordering::Relaxed);
        state.edit=Some(Edit::Rename);assert!(!state.poll_directory(&mut fs));
        state.edit=None;assert!(state.poll_directory(&mut fs));
        assert_eq!(fs.children(1)[0].name,"new.txt");assert_eq!(state.selected,None);
        assert!(!state.poll_directory(&mut fs));
    }

    #[test]
    fn recursive_delete_requires_confirmation_preflights_and_yields_between_items() {
        let mut fs=FileSystem::new();let folder=fs.mkdir(1,"folder");let child=fs.mkdir(folder,"child");
        let note=fs.touch(child,"note.txt",b"preserve until confirmed");let sibling=fs.touch(1,"keep.txt",b"keep");
        let mut state=Explorer::new(1,1);state.selected=Some(folder);
        state.key(&mut fs,input::DELETE);
        assert_eq!(state.delete_prompt,Some(folder));assert!(!state.work_pending());
        state.key(&mut fs,13); // Cancel is the initial button.
        assert!(!fs.node(folder).unwrap().deleted);
        state.key(&mut fs,input::DELETE);state.key(&mut fs,input::LEFT);state.key(&mut fs,13);
        for _ in 0..5{assert!(state.work(&mut fs));assert!(!fs.node(note).unwrap().deleted);}
        assert!(state.work(&mut fs));assert!(fs.node(note).unwrap().deleted);
        assert!(!fs.node(child).unwrap().deleted);assert!(!fs.node(folder).unwrap().deleted);
        while state.work(&mut fs){}
        assert!(fs.node(folder).unwrap().deleted);assert!(!fs.node(sibling).unwrap().deleted);
        assert_eq!(state.selected,None);assert!(!state.work_pending());
    }

    #[test]
    fn cancelling_delete_before_preflight_completes_preserves_the_tree() {
        let mut fs=FileSystem::new();let folder=fs.mkdir(1,"folder");let note=fs.touch(folder,"note.txt",b"body");
        let mut state=Explorer::new(1,1);state.selected=Some(folder);
        state.key(&mut fs,input::DELETE);state.key(&mut fs,input::LEFT);state.key(&mut fs,13);
        state.work(&mut fs);state.key(&mut fs,27);assert!(!state.work_pending());
        assert!(!fs.node(folder).unwrap().deleted);assert!(!fs.node(note).unwrap().deleted);
    }

    #[test]
    fn provider_failures_preserve_history_and_report_search_errors() {
        fn provider(_: u32, path: &str, _: &[u8]) -> Result<Vec<u8>, u32> {
            if matches!(path, "C:" | "C:\\") {
                let name = "Denied";
                let mut out = Vec::new();
                out.extend_from_slice(&(name.len() as u32).to_le_bytes());
                out.extend_from_slice(&abi::DIRECTORY.to_le_bytes());
                out.extend_from_slice(&0u64.to_le_bytes());
                out.extend_from_slice(name.as_bytes());
                Ok(out)
            } else {
                Err(abi::STATUS_ACCESS_DENIED)
            }
        }
        let mut fs = FileSystem::mounted(Backend(provider)).unwrap();
        let id = fs.children(1)[0].id;
        let mut state = Explorer::new(1, 1);
        assert!(!state.navigate(&mut fs, id));
        assert_eq!(state.directory, 1);
        assert!(!state.can_back());
        assert!(state.status.contains("C0000022"));
        for key in b"\x06no-match" {
            state.key(&mut fs, *key);
        }
        while state.work(&mut fs) {}
        assert!(state.status.contains("could not be searched"));
        state.selected = Some(id);
        state.command(&mut fs, 5);state.key(&mut fs,input::LEFT);state.key(&mut fs,13);while state.work(&mut fs){}
        assert_eq!(state.selected, Some(id));
        assert!(!fs.node(id).unwrap().deleted);
        assert!(state.status.contains("C0000022"));
    }
    #[test]
    fn commands_act_on_the_shared_store_and_only_activate_on_release() {
        let mut fs = FileSystem::new();
        let mut state = Explorer::new(1, 1);
        let r = Layout::new(body()).command(2);
        state.pointer(&mut fs, body(), r.x + 8, r.y + 8, true, true, false);
        assert!(fs.children(1).is_empty());
        state.pointer(&mut fs, body(), r.x + 8, r.y + 8, false, false, false);
        assert_eq!(fs.children(1)[0].name, "New folder");
        let file = fs.touch(1, "test.txt", b"content");
        state.selected = Some(file);
        state.key(&mut fs, 3);
        state.key(&mut fs, 22);
        assert_eq!(fs.children(1).len(), 3);
        assert_eq!(
            fs.node(state.selected.unwrap()).unwrap().contents,
            b"content"
        );
        click_rect(&mut state, &mut fs, r);
        assert_eq!(
            fs.children(1)
                .iter()
                .filter(|n| n.kind == NodeKind::Directory)
                .count(),
            2
        );
    }
    #[test]
    fn paging_and_keyboard_selection_reach_items_beyond_the_viewport() {
        let mut fs = FileSystem::new();
        for i in 0..100 {
            fs.touch(1, &format!("item-{i:03}.txt"), b"x");
        }
        let mut state = Explorer::new(1, 1);
        let l = Layout::new(body());
        state.pointer(&mut fs, body(), l.list.x, l.list.y, false, false, false);
        state.key(&mut fs, input::END);
        assert_eq!(state.selected, Some(state.rows(&fs)[99].id));
        assert_eq!(state.offset, 100 - l.capacity());
        state.key(&mut fs, input::HOME);
        assert_eq!(state.offset, 0);
        let r = Rect::new(
            l.scroll.x,
            l.scroll.y + l.scroll.height as i32 - 20,
            l.scroll.width,
            16,
        );
        click_rect(&mut state, &mut fs, r);
        assert_eq!(state.offset, l.capacity());
    }
    #[test]
    fn renderer_never_paints_outside_client_even_at_minimum_window_size() {
        for (w, h) in [(228, 84), (320, 160), (628, 414), (900, 650)] {
            let mut fs = FileSystem::new();
            let id = fs.touch(1, "a very long filename that must fit.txt", b"bytes");
            let mut state = Explorer::new(1, 1);
            state.selected = Some(id);
            let b = Rect::new(11, 9, w, h);
            let mut frame = Surface::new(w + 30, h + 30);
            frame.fill_rect(0, 0, w + 30, h + 30, 0xFFAB12CD);
            draw(&state, &fs, &mut frame, b, |surface, _, r, _| {
                surface.fill_rect(r.x, r.y, r.width, 16, 0xFF333333);
            });
            for y in 0..frame.height() {
                for x in 0..frame.width() {
                    if !b.contains(x as i32, y as i32) {
                        assert_eq!(
                            frame.pixel(x, y),
                            Some(0xFFAB12CD),
                            "leak at {x},{y} in {w}x{h}"
                        );
                    }
                }
            }
        }
    }
}
