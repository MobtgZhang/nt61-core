use alloc::string::String;
use alloc::vec::Vec;

use nt61_abi::desktop_bridge as abi;

/// One filesystem operation, returning bytes or an explicit NTSTATUS.
#[derive(Clone,Copy,Debug)]
pub struct Backend(pub fn(u32,&str,&[u8])->Result<Vec<u8>,u32>);
impl PartialEq for Backend {
    fn eq(&self,other:&Self)->bool {core::ptr::fn_addr_eq(self.0,other.0)}
}
impl Eq for Backend {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipboardMode {
    Copy,
    Cut,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind {
    Directory,
    File,
    Link,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileNode {
    pub id: u32,
    pub name: String,
    pub kind: NodeKind,
    pub parent: u32,
    pub contents: Vec<u8>,
    /// Length reported by the mounted provider; memory files use contents.len().
    pub size: Option<u64>,
    pub deleted: bool,
    pub original_path: Option<String>,
    pub deleted_at: u64,
    pub target: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileSystem {
    nodes: Vec<FileNode>,
    clipboard: Vec<u32>,
    clipboard_mode: ClipboardMode,
    clipboard_text: String,
    associations: Vec<(String, String)>,
    navigation: Vec<u32>,
    navigation_index: usize,
    next_id: u32,
    backend: Option<Backend>,
    loaded_directories: Vec<u32>,
    pub last_error: Option<u32>,
    pub shell_icons: Option<alloc::sync::Arc<crate::shell_resources::ShellIcons>>,
    pub resource_error: Option<u32>,
    resources_attempted: bool,
}

/// Win32 shell component rules (NTFS itself also supports other namespaces).
pub(crate) fn valid_component(name: &str) -> bool {
    if name.is_empty() || name.encode_utf16().count()>255 || name.ends_with([' ','.'])
        || name.chars().any(|c| c<' ' || matches!(c,'\\'|'/'|':'|'*'|'?'|'"'|'<'|'>'|'|')) {return false;}
    let stem=name.split('.').next().unwrap_or("").trim_end_matches(' ');
    if ["CON","PRN","AUX","NUL"].iter().any(|v|stem.eq_ignore_ascii_case(v)){return false;}
    !(stem.get(..3).is_some_and(|prefix|prefix.eq_ignore_ascii_case("COM")||prefix.eq_ignore_ascii_case("LPT"))
        && matches!(stem.get(3..),Some("1"|"2"|"3"|"4"|"5"|"6"|"7"|"8"|"9"|"¹"|"²"|"³")))
}

impl FileSystem {
    pub fn new() -> Self {
        let mut filesystem = Self {
            nodes: Vec::new(),
            clipboard: Vec::new(),
            clipboard_mode: ClipboardMode::Copy,
            clipboard_text: String::new(),
            associations: Vec::new(),
            navigation: Vec::new(),
            navigation_index: 0,
            next_id: 1,
            backend: None,
            loaded_directories: Vec::new(),
            last_error: None,
            shell_icons: None,
            resource_error: None,
            resources_attempted: false,
        };
        filesystem.next_id = 2;
        filesystem.nodes.push(FileNode {
            id: 1,
            name: String::from("C:"),
            kind: NodeKind::Directory,
            parent: 0,
            contents: Vec::new(),
            size: None,
            deleted: false,
            original_path: None,
            deleted_at: 0,
            target: None,
        });
        filesystem.navigation.push(1);
        filesystem
    }

    /// Mount only after an actual directory request succeeds. A failed mount
    /// leaves no invented Windows/Users hierarchy behind.
    pub fn mounted(backend:Backend)->Result<Self,u32>{
        let mut files=Self::new();files.backend=Some(backend);files.load_directory(1)?;Ok(files)
    }
    pub fn is_mounted(&self)->bool{self.backend.is_some()}
    fn request(&mut self,operation:u32,path:&str,data:&[u8])->Result<Vec<u8>,u32>{
        let backend=self.backend.ok_or(abi::STATUS_NOT_SUPPORTED)?;
        let result=(backend.0)(operation,path,data);
        self.last_error=result.as_ref().err().copied();result
    }
    fn child_path(&self,parent:u32,name:&str)->Option<String>{
        if !valid_component(name) || !self.node(parent).is_some_and(|n| !n.deleted && n.kind==NodeKind::Directory) {return None;}
        Some(alloc::format!("{}\\{name}",self.path(parent)?.trim_end_matches('\\')))
    }
    pub fn load_directory(&mut self,id:u32)->Result<(),u32>{
        if self.backend.is_none()||self.loaded_directories.contains(&id){return Ok(());}
        self.refresh_directory(id)
    }
    pub fn refresh_directory(&mut self,id:u32)->Result<(),u32>{
        if self.backend.is_none(){return Ok(());}
        let path=self.path(id).ok_or(abi::STATUS_NOT_FOUND)?;
        let bytes=self.request(abi::LIST,&path,&[])?;
        let entries=decode_directory(&bytes).inspect_err(|status|self.last_error=Some(*status))?;
        // A missing or replaced directory invalidates its entire cached tree.
        // Otherwise recreating its name can revive children from the old tree.
        // Do this only after the complete provider response has been validated.
        let removed:Vec<_>=self.nodes.iter().filter(|node|node.parent==id && !node.deleted)
            .filter(|node| !entries.iter().any(|(name,kind,_)|
                node.name.eq_ignore_ascii_case(name) && node.kind==*kind))
            .map(|node|node.id).collect();
        for child in removed {self.invalidate_cached_subtree(child);}
        for node in self.nodes.iter_mut().filter(|node|node.parent==id){node.deleted=true;}
        for (name,kind,size) in entries{
            if let Some(node)=self.nodes.iter_mut().find(|node|node.parent==id&&node.name.eq_ignore_ascii_case(&name)){
                if node.size!=Some(size) || node.kind!=kind {node.contents.clear();}
                node.deleted=false;node.name=name;node.kind=kind;node.size=Some(size);
            }else{let child=self.create_node(&name,kind,id);if let Some(node)=self.node_mut(child){node.size=Some(size);}}
        }
        if !self.loaded_directories.contains(&id){self.loaded_directories.push(id);}
        Ok(())
    }
    fn invalidate_cached_subtree(&mut self,id:u32){
        let mut removed=alloc::vec![id];
        let mut cursor=0;
        while cursor<removed.len(){
            let parent=removed[cursor];
            for node in &self.nodes{
                if node.parent==parent && !removed.contains(&node.id){removed.push(node.id);}
            }
            cursor+=1;
        }
        for node in &mut self.nodes{
            if removed.contains(&node.id){node.deleted=true;node.contents.clear();}
        }
        self.loaded_directories.retain(|id|!removed.contains(id));
        self.clipboard.retain(|id|!removed.contains(id));
    }
    pub fn load_file(&mut self,id:u32)->Result<(),u32>{
        if self.backend.is_none(){return Ok(());}
        let path=self.path(id).ok_or(abi::STATUS_NOT_FOUND)?;
        let bytes=self.request(abi::READ,&path,&[])?;
        let node=self.node_mut(id).ok_or(abi::STATUS_NOT_FOUND)?;node.size=Some(bytes.len() as u64);node.contents=bytes;
        Ok(())
    }
    pub fn save(&mut self,id:u32,bytes:&[u8])->Result<(),u32>{
        if self.backend.is_some(){
            let path=self.path(id).ok_or(abi::STATUS_NOT_FOUND)?;
            self.request(abi::WRITE,&path,bytes)?;
        }
        let node=self.node_mut(id).ok_or(abi::STATUS_NOT_FOUND)?;node.size=Some(bytes.len() as u64);node.contents=bytes.to_vec();Ok(())
    }
    /// Resolve each component lazily against the mounted directory provider.
    pub fn resolve(&mut self,path:&str)->Option<u32>{
        if path.contains('\0') || (path.contains(':') && !path.get(..2).is_some_and(|p| p.eq_ignore_ascii_case("C:"))) { return None; }
        let path=path.strip_prefix("C:").or_else(||path.strip_prefix("c:")).unwrap_or(path);
        let mut id=1;
        for part in path.split(['\\','/']).filter(|part|!part.is_empty()){
            if part=="." { continue; }
            if part==".." { id=self.node(id)?.parent; if id==0 {return None;} continue; }
            if self.node(id)?.kind!=NodeKind::Directory {return None;}
            self.load_directory(id).ok()?;
            if !self.children(id).iter().any(|node|node.name.eq_ignore_ascii_case(part)) {
                self.refresh_directory(id).ok()?;
            }
            id=self.children(id).into_iter().find(|node|node.name.eq_ignore_ascii_case(part))?.id;
        }
        Some(id)
    }

    pub fn load_shell_resources(&mut self){
        if self.resources_attempted || self.backend.is_none(){return;}
        self.resources_attempted=true;
        let backend=self.backend.unwrap();
        let loaded=crate::shell_resources::ShellIcons::load(|path,offset,length|{
            let mut input=offset.to_le_bytes().to_vec();input.extend_from_slice(&(length as u32).to_le_bytes());
            (backend.0)(abi::READ_AT,path,&input)
        });
        match loaded{Ok(icons)=>self.shell_icons=Some(alloc::sync::Arc::new(icons)),Err(status)=>self.resource_error=Some(status)}
    }

    pub fn root_id(&self) -> u32 {
        1
    }

    pub fn current_directory(&self) -> u32 {
        self.navigation
            .get(self.navigation_index)
            .copied()
            .unwrap_or(self.root_id())
    }

    pub fn iter_children(&self, parent:u32)->impl Iterator<Item=&FileNode> {
        self.nodes.iter().filter(move |node|node.parent==parent&&!node.deleted)
    }
    pub fn children(&self, parent: u32) -> Vec<&FileNode> {
        self.nodes
            .iter()
            .filter(|node| node.parent == parent && !node.deleted)
            .collect()
    }

    pub fn node(&self, id: u32) -> Option<&FileNode> {
        self.nodes.iter().find(|node| node.id == id)
    }

    pub fn node_mut(&mut self, id: u32) -> Option<&mut FileNode> {
        self.nodes.iter_mut().find(|node| node.id == id)
    }

    fn create_node(&mut self, name: &str, kind: NodeKind, parent: u32) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.nodes.push(FileNode {
            id,
            name: String::from(name),
            kind,
            parent,
            contents: Vec::new(),
            size: None,
            deleted: false,
            original_path: None,
            deleted_at: 0,
            target: None,
        });
        id
    }

    fn prepare_create(&mut self, parent: u32, name: &str) -> Option<String> {
        self.last_error=None;
        let Some(path)=self.child_path(parent,name) else {self.last_error=Some(abi::STATUS_INVALID_PARAMETER);return None;};
        if self.backend.is_some() && self.load_directory(parent).is_err(){return None;}
        if self.iter_children(parent).any(|n|n.name.eq_ignore_ascii_case(name)) {
            self.last_error=Some(abi::STATUS_OBJECT_NAME_COLLISION);return None;
        }
        Some(path)
    }

    pub fn mkdir(&mut self, parent: u32, name: &str) -> u32 {
        let Some(path)=self.prepare_create(parent,name) else {return 0;};
        if self.backend.is_some() && self.request(abi::MKDIR,&path,&[]).is_err(){return 0;}
        self.create_node(name, NodeKind::Directory, parent)
    }

    pub fn touch(&mut self, parent: u32, name: &str, contents: &[u8]) -> u32 {
        let Some(path)=self.prepare_create(parent,name) else {return 0;};
        if self.backend.is_some() && self.request(abi::CREATE,&path,contents).is_err(){return 0;}
        let id = self.create_node(name, NodeKind::File, parent);
        if let Some(node) = self.node_mut(id) {
            node.size=Some(contents.len() as u64);
            node.contents.extend_from_slice(contents);
        }
        id
    }

    pub fn link(&mut self, parent: u32, name: &str, target: &str) -> u32 {
        let id = self.create_node(name, NodeKind::Link, parent);
        if let Some(node) = self.node_mut(id) {
            node.target = Some(String::from(target));
        }
        id
    }

    pub fn rename(&mut self, id: u32, name: &str) -> bool {
        let Some(node)=self.node(id).filter(|node|!node.deleted).cloned() else{return false;};
        if id==1 || self.child_path(node.parent,name).is_none() || self.children(node.parent).iter().any(|n|n.id!=id&&n.name.eq_ignore_ascii_case(name)){
            self.last_error=Some(abi::STATUS_INVALID_PARAMETER);return false;
        }
        if self.backend.is_some(){
            let Some(source)=self.path(id)else{return false;};let destination=self.child_path(node.parent,name).unwrap();
            if self.request(abi::RENAME,&source,destination.as_bytes()).is_err(){return false;}
        }
        self.last_error=None;
        self.node_mut(id)
            .map(|node| node.name = String::from(name))
            .is_some()
    }

    pub fn copy_selected_to_clipboard(&mut self, ids: &[u32]) {
        self.clipboard_mode = ClipboardMode::Copy;
        self.clipboard = ids.to_vec();
    }

    pub fn cut_selected_to_clipboard(&mut self, ids: &[u32]) {
        self.clipboard_mode = ClipboardMode::Cut;
        self.clipboard = ids.to_vec();
    }

    pub fn paste_from_clipboard(&mut self, destination: u32) -> Vec<u32> {
        if self.backend.is_some(){return self.paste_mounted(destination);}
        let clipboard = self.clipboard.clone();
        match self.clipboard_mode {
            ClipboardMode::Cut => {
                let moved = clipboard
                    .iter()
                    .filter_map(|id| self.move_one(*id, destination))
                    .collect();
                self.clipboard.clear();
                self.clipboard_mode = ClipboardMode::Copy;
                moved
            }
            ClipboardMode::Copy => clipboard
                .iter()
                .filter_map(|id| self.copy_one(*id, destination))
                .collect(),
        }
    }

    fn paste_mounted(&mut self, destination: u32) -> Vec<u32> {
        let mut copied = Vec::new();
        // A cut needs an atomic rename operation. Never emulate it by deleting
        // the source after a potentially partial copy.
        if self.clipboard_mode == ClipboardMode::Cut {
            self.last_error = Some(abi::STATUS_NOT_SUPPORTED);
            return copied;
        }
        if self.load_directory(destination).is_err() { return copied; }
        for source in self.clipboard.clone() {
            if !self.valid_destination(source, destination) {
                self.last_error = Some(abi::STATUS_INVALID_PARAMETER);
                break;
            }
            let Some(node) = self.node(source).filter(|node| !node.deleted).cloned() else {
                self.last_error = Some(abi::STATUS_NOT_FOUND);
                break;
            };
            if node.kind != NodeKind::File {
                self.last_error = Some(abi::STATUS_NOT_SUPPORTED);
                break;
            }
            let mut name = node.name.clone();
            if self.children(destination).iter().any(|entry| entry.name.eq_ignore_ascii_case(&name)) {
                let (stem, extension) = node.name.rsplit_once('.')
                    .filter(|(stem, _)| !stem.is_empty())
                    .map(|(stem, ext)| (stem, alloc::format!(".{ext}")))
                    .unwrap_or((&node.name, String::new()));
                let mut number = 1;
                loop {
                    name = if number == 1 { alloc::format!("{stem} - Copy{extension}") }
                        else { alloc::format!("{stem} - Copy ({number}){extension}") };
                    if !self.children(destination).iter().any(|entry| entry.name.eq_ignore_ascii_case(&name)) { break; }
                    number += 1;
                }
            }
            if self.load_file(source).is_err() { break; }
            let bytes = self.node(source).unwrap().contents.clone();
            let id = self.touch(destination, &name, &bytes);
            if id == 0 { break; }
            copied.push(id);
        }
        copied
    }

    fn valid_destination(&self, source: u32, destination: u32) -> bool {
        if source == self.root_id() { return false; }
        let Some(target) = self.node(destination) else { return false; };
        if target.kind != NodeKind::Directory || target.deleted { return false; }
        let mut parent = destination;
        // Bound the walk even if malformed state was supplied by a caller.
        for _ in 0..=self.nodes.len() {
            if parent == source { return false; }
            if parent == 0 { return true; }
            let Some(node) = self.node(parent) else { return false; };
            parent = node.parent;
        }
        false
    }

    fn copy_one(&mut self, source: u32, destination: u32) -> Option<u32> {
        if !self.valid_destination(source, destination) { return None; }
        let node = self.node(source).filter(|node| !node.deleted)?.clone();
        let name = if node.parent == destination { String::from("Copy of ") + &node.name } else { node.name.clone() };
        let id = self.copy_tree(source, destination)?;
        self.node_mut(id)?.name = name;
        Some(id)
    }

    fn copy_tree(&mut self, source: u32, destination: u32) -> Option<u32> {
        let node = self.node(source)?.clone();
        let children: Vec<u32> = self.children(source).iter().map(|node| node.id).collect();
        let id = self.next_id;
        self.next_id += 1;
        self.nodes.push(FileNode { id, parent: destination, ..node });
        for child in children { self.copy_tree(child, id)?; }
        Some(id)
    }

    /// Permanent removal is distinct from the recycle-bin operation.
    pub fn remove(&mut self,id:u32)->Result<(),u32>{
        if id==1{return Err(abi::STATUS_ACCESS_DENIED);}
        let node=self.node(id).filter(|n|!n.deleted).cloned().ok_or(abi::STATUS_NOT_FOUND)?;
        if self.backend.is_some(){
            let path=self.path(id).ok_or(abi::STATUS_NOT_FOUND)?;
            self.request(abi::REMOVE,&path,&[])?;
        }else if node.kind==NodeKind::Directory && self.iter_children(id).next().is_some(){
            self.last_error=Some(0xc0000101);return Err(0xc0000101);
        }
        self.node_mut(id).unwrap().deleted=true;
        self.clipboard.retain(|v|*v!=id);Ok(())
    }

    pub fn recycle(&mut self, id: u32, now: u64) -> bool {
        if self.backend.is_some(){self.last_error=Some(abi::STATUS_NOT_SUPPORTED);return false;}
        if id == self.root_id() { return false; }
        let original_path = self.path(id);
        self.node_mut(id)
            .map(|node| {
                node.deleted = true;
                node.deleted_at = now;
                node.original_path = original_path;
            })
            .is_some()
    }

    pub fn recycle_permanently(&mut self, id: u32) -> bool {
        if self.backend.is_some(){self.last_error=Some(abi::STATUS_NOT_SUPPORTED);return false;}
        if id == self.root_id() || self.node(id).is_none() { return false; }
        let children: Vec<u32> = self.nodes.iter().filter(|node| node.parent == id).map(|node| node.id).collect();
        for child in children { self.recycle_permanently(child); }
        self.nodes.retain(|node| node.id != id);
        self.clipboard.retain(|node| *node != id);
        true
    }

    pub fn restore(&mut self, id: u32) -> bool {
        if self.backend.is_some(){self.last_error=Some(abi::STATUS_NOT_SUPPORTED);return false;}
        self.node_mut(id)
            .map(|node| {
                node.deleted = false;
                node.original_path = None;
                node.deleted_at = 0;
            })
            .is_some()
    }

    pub fn restore_to(&mut self, id: u32, parent: u32) -> bool {
        if self.backend.is_some(){self.last_error=Some(abi::STATUS_NOT_SUPPORTED);return false;}
        if !self.valid_destination(id, parent) { return false; }
        let Some(node) = self.node_mut(id) else {
            return false;
        };
        node.deleted = false;
        node.parent = parent;
        node.original_path = None;
        node.deleted_at = 0;
        true
    }

    pub fn drag_drop(&mut self, ids: &[u32], destination: u32) -> Vec<u32> {
        self.cut_selected_to_clipboard(ids);
        self.paste_from_clipboard(destination)
    }

    pub fn set_clipboard_text(&mut self, text: &str) {
        self.clipboard_text = String::from(text);
    }

    pub fn clipboard_text(&self) -> &str {
        &self.clipboard_text
    }

    pub fn associate(&mut self, extension: &str, command: &str) {
        if let Some(association) = self
            .associations
            .iter_mut()
            .find(|(existing, _)| existing == extension)
        {
            association.1 = String::from(command);
        } else {
            self.associations
                .push((String::from(extension), String::from(command)));
        }
    }

    pub fn association_for(&self, name: &str) -> Option<&str> {
        let extension = name.rsplit('.').next()?;
        self.associations
            .iter()
            .find(|(existing, _)| existing == extension)
            .map(|(_, command)| command.as_str())
    }

    pub fn path(&self, id: u32) -> Option<String> {
        let mut parts = Vec::new();
        let mut current = self.node(id)?;
        loop {
            if current.deleted || parts.len() >= self.nodes.len() { return None; }
            parts.push(current.name.clone());
            if current.parent == 0 {
                break;
            }
            current = self.node(current.parent)?;
        }
        parts.reverse();
        Some(parts.join("\\"))
    }

    pub fn navigate(&mut self, id: u32) {
        self.navigation.truncate(self.navigation_index + 1);
        self.navigation.push(id);
        self.navigation_index = self.navigation.len() - 1;
    }

    pub fn back(&mut self) -> Option<u32> {
        if self.navigation_index == 0 {
            return None;
        }
        self.navigation_index -= 1;
        self.navigation.get(self.navigation_index).copied()
    }

    pub fn forward(&mut self) -> Option<u32> {
        if self.navigation_index + 1 >= self.navigation.len() {
            return None;
        }
        self.navigation_index += 1;
        self.navigation.get(self.navigation_index).copied()
    }

    fn move_one(&mut self, source: u32, destination: u32) -> Option<u32> {
        if !self.valid_destination(source, destination) { return None; }
        let Some(node) = self.node(source).cloned() else {
            return None;
        };
        if node.id == destination || node.parent == destination {
            return None;
        }
        let Some(node) = self.node_mut(source) else {
            return None;
        };
        node.parent = destination;
        node.deleted = false;
        Some(node.id)
    }

    pub fn recycle_bin(&self) -> Vec<&FileNode> {
        self.nodes.iter().filter(|node| node.deleted).collect()
    }

    pub fn libraries(&self) -> [(&'static str, u32); 4] {
        [
            ("Documents", 1),
            ("Music", 1),
            ("Pictures", 1),
            ("Videos", 1),
        ]
    }
}

/// Validate the complete wire response before altering cached nodes.
fn decode_directory(mut bytes:&[u8])->Result<Vec<(String,NodeKind,u64)>,u32>{
    let mut entries=Vec::new();
    while !bytes.is_empty(){
        if bytes.len()<abi::ENTRY_HEADER{return Err(abi::STATUS_INVALID_PARAMETER);}
        let n=u32::from_le_bytes(bytes[..4].try_into().map_err(|_|abi::STATUS_INVALID_PARAMETER)?)as usize;
        let flags=u32::from_le_bytes(bytes[4..8].try_into().map_err(|_|abi::STATUS_INVALID_PARAMETER)?);
        let size=u64::from_le_bytes(bytes[8..16].try_into().map_err(|_|abi::STATUS_INVALID_PARAMETER)?);
        bytes=&bytes[abi::ENTRY_HEADER..];
        if n==0||n>abi::MAX_PATH||n>bytes.len(){return Err(abi::STATUS_INVALID_PARAMETER);}
        let name=core::str::from_utf8(&bytes[..n]).map_err(|_|abi::STATUS_INVALID_PARAMETER)?;
        if name.contains(['\\','/','\0'])||matches!(name,"."|".."){return Err(abi::STATUS_INVALID_PARAMETER);}
        entries.push((name.into(),if flags&abi::DIRECTORY!=0{NodeKind::Directory}else{NodeKind::File},size));
        bytes=&bytes[n..];
    }
    Ok(entries)
}

impl Default for FileSystem {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listing(name: &str, directory: bool) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&(name.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&(if directory { abi::DIRECTORY } else { 0 }).to_le_bytes());
        bytes.extend_from_slice(&0u64.to_le_bytes());
        bytes.extend_from_slice(name.as_bytes());
        bytes
    }

    fn mounted_backend(operation: u32, path: &str, input: &[u8]) -> Result<Vec<u8>, u32> {
        match (operation, path) {
            (abi::LIST, "C:" | "C:\\") => Ok(listing("Documents", true)),
            (abi::LIST, "C:\\Documents") => Ok(listing("note.txt", false)),
            (abi::READ, "C:\\Documents\\note.txt") => Ok(b"from volume".to_vec()),
            (abi::WRITE, "C:\\Documents\\note.txt") if input == b"accepted" => Ok(Vec::new()),
            (abi::WRITE | abi::CREATE | abi::MKDIR, _) => Err(abi::STATUS_ACCESS_DENIED),
            _ => Err(abi::STATUS_NOT_FOUND),
        }
    }

    #[test]
    fn mounted_files_resolve_lazily_and_failed_writes_preserve_contents() {
        let mut files = FileSystem::mounted(Backend(mounted_backend)).unwrap();
        assert_eq!(files.children(1).len(), 1);
        let documents = files.children(1)[0].id;
        assert!(files.children(documents).is_empty());
        let note = files.resolve("c:\\documents\\NOTE.txt").unwrap();
        assert!(files.node(note).unwrap().contents.is_empty());
        files.load_file(note).unwrap();
        assert_eq!(files.node(note).unwrap().contents, b"from volume");
        assert_eq!(files.save(note, b"denied"), Err(abi::STATUS_ACCESS_DENIED));
        assert_eq!(files.node(note).unwrap().contents, b"from volume");
        files.save(note, b"accepted").unwrap();
        assert_eq!(files.node(note).unwrap().contents, b"accepted");
        assert_eq!(files.touch(documents, "new.txt", b"denied"), 0);
        assert_eq!(files.mkdir(documents, "new folder"), 0);
        assert_eq!(files.children(documents).len(), 1);
    }

    fn copy_backend(operation: u32, path: &str, input: &[u8]) -> Result<Vec<u8>, u32> {
        match (operation, path) {
            (abi::LIST, "C:" | "C:\\") => Ok(listing("Documents", true)),
            (abi::LIST, "C:\\Documents") => Ok(listing("note.txt", false)),
            (abi::READ, "C:\\Documents\\note.txt") => Ok(b"actual disk bytes".to_vec()),
            (abi::CREATE, "C:\\Documents\\note - Copy.txt") if input == b"actual disk bytes" => Ok(Vec::new()),
            (abi::WRITE | abi::CREATE, _) => Err(abi::STATUS_ACCESS_DENIED),
            _ => Err(abi::STATUS_NOT_FOUND),
        }
    }

    #[test]
    fn mounted_copy_reads_source_and_only_caches_successful_writes() {
        let mut files = FileSystem::mounted(Backend(copy_backend)).unwrap();
        let source = files.resolve("C:\\Documents\\note.txt").unwrap();
        let parent = files.node(source).unwrap().parent;
        files.copy_selected_to_clipboard(&[source]);
        let copied = files.paste_from_clipboard(parent);
        assert_eq!(copied.len(), 1);
        assert_eq!(files.node(copied[0]).unwrap().name, "note - Copy.txt");
        assert_eq!(files.node(copied[0]).unwrap().contents, b"actual disk bytes");
        assert!(files.paste_from_clipboard(parent).is_empty());
        assert_eq!(files.last_error, Some(abi::STATUS_ACCESS_DENIED));
        assert_eq!(files.children(parent).len(), 2);
    }

    #[test]
    fn invalid_directory_response_preserves_cached_entries() {
        let mut files = FileSystem::mounted(Backend(mounted_backend)).unwrap();
        let before = files.children(1).into_iter().cloned().collect::<Vec<_>>();
        files.backend = Some(Backend(|_, _, _| {
            let mut bytes = listing("injected", false);
            bytes.push(0); // a truncated second header must reject the entire listing
            Ok(bytes)
        }));
        assert_eq!(files.refresh_directory(1), Err(abi::STATUS_INVALID_PARAMETER));
        assert_eq!(files.children(1).into_iter().cloned().collect::<Vec<_>>(), before);
        assert_eq!(files.last_error, Some(abi::STATUS_INVALID_PARAMETER));
    }

    #[test]
    fn external_directory_removal_invalidates_cached_descendants_before_recreation() {
        fn initial(operation: u32, path: &str, _: &[u8]) -> Result<Vec<u8>, u32> {
            match (operation, path) {
                (abi::LIST, "C:") => Ok(listing("folder", true)),
                (abi::LIST, "C:\\folder") => Ok(listing("child", true)),
                (abi::LIST, "C:\\folder\\child") => Ok(listing("old.txt", false)),
                _ => Err(abi::STATUS_NOT_FOUND),
            }
        }
        let mut files = FileSystem::mounted(Backend(initial)).unwrap();
        let old = files.resolve("C:\\folder\\child\\old.txt").unwrap();
        let child = files.node(old).unwrap().parent;
        let folder = files.node(child).unwrap().parent;
        files.backend = Some(Backend(|_, _, _| Ok(Vec::new())));
        files.refresh_directory(1).unwrap();
        assert!(files.children(folder).is_empty(), "deleted subtree must not expose cached children");
        assert!(files.children(child).is_empty());
        assert!(files.path(old).is_none());
        files.backend = Some(Backend(|operation, path, _| match (operation, path) {
            (abi::LIST, "C:") => Ok(listing("folder", true)),
            (abi::LIST, "C:\\folder") => Ok(listing("child", true)),
            (abi::LIST, "C:\\folder\\child") => Ok(listing("new.txt", false)),
            _ => Err(abi::STATUS_NOT_FOUND),
        }));
        files.refresh_directory(1).unwrap();
        assert!(files.resolve("C:\\folder\\child\\old.txt").is_none());
        assert!(files.resolve("C:\\folder\\child\\new.txt").is_some());
        assert!(files.path(old).is_none(), "recreating a parent cannot revive deleted children");
    }

    #[test]
    fn create_and_rename_validate_names_and_preserve_existing_files() {
        let mut fs=FileSystem::new();
        let id=fs.touch(1,"note.txt",b"original");
        for name in ["NOTE.txt","../escape","a:b","bad?","trailing.","NUL.txt","COM1","", "bad\0name"] {
            assert_eq!(fs.touch(1,name,b"replacement"),0,"{name:?}");
        }
        assert_eq!(fs.node(id).unwrap().contents,b"original");
        assert!(!fs.rename(id,"bad|name"));
        assert_eq!(fs.children(1).len(),1);
        assert_eq!(fs.mkdir(id,"child"),0);
    }

    #[test]
    fn stale_child_ids_cannot_access_deleted_ancestors_or_path_cycles() {
        let mut fs=FileSystem::new();let folder=fs.mkdir(1,"folder");let child=fs.touch(folder,"note.txt",b"body");
        fs.node_mut(folder).unwrap().deleted=true;
        assert_eq!(fs.path(child),None);assert_eq!(fs.child_path(folder,"new.txt"),None);
        fs.node_mut(folder).unwrap().deleted=false;fs.node_mut(folder).unwrap().parent=folder;
        assert_eq!(fs.path(child),None);
    }

    #[test]
    fn failed_mount_has_no_synthetic_fallback() {
        assert!(FileSystem::mounted(Backend(|_, _, _| Err(abi::STATUS_NOT_SUPPORTED))).is_err());
    }

    #[test]
    fn directory_copy_is_recursive_and_moves_cannot_create_cycles() {
        let mut fs = FileSystem::new();
        let source = fs.mkdir(1, "source");
        let child = fs.mkdir(source, "child");
        fs.touch(child, "note.txt", b"payload");
        assert!(fs.drag_drop(&[source], child).is_empty());
        assert_eq!(fs.path(child).as_deref(), Some("C:\\source\\child"));
        let dest = fs.mkdir(1, "destination");
        fs.copy_selected_to_clipboard(&[source]);
        let copied = fs.paste_from_clipboard(dest)[0];
        let copied_child = fs.children(copied)[0].id;
        assert_eq!(fs.children(copied_child)[0].contents, b"payload");
        assert!(fs.recycle_permanently(copied));
        assert!(fs.node(copied_child).is_none());
        assert_eq!(fs.children(child)[0].contents, b"payload");
    }

    #[test]
    fn create_rename_copy_and_recycle_are_win7_semantics() {
        let mut filesystem = FileSystem::new();
        let document = filesystem.touch(1, "计划.txt", b"hello");
        filesystem.copy_selected_to_clipboard(&[document]);
        let pasted = filesystem.paste_from_clipboard(1);
        assert_eq!(pasted.len(), 1);
        assert!(filesystem.rename(document, "新建.txt"));
        assert!(filesystem.recycle(document, 42));
        assert_eq!(filesystem.recycle_bin().len(), 1);
        assert!(filesystem.restore(document));
        assert_eq!(filesystem.recycle_bin().len(), 0);
    }

    #[test]
    fn cut_paste_moves_without_duplicating_and_drag_drop_matches() {
        let mut filesystem = FileSystem::new();
        let documents = filesystem.mkdir(1, "Documents");
        let file = filesystem.touch(1, "readme.txt", b"data");
        filesystem.cut_selected_to_clipboard(&[file]);
        assert_eq!(filesystem.paste_from_clipboard(documents), vec![file]);
        assert_eq!(filesystem.node(file).unwrap().parent, documents);
        let source = filesystem.node(file).unwrap().id;
        assert_eq!(filesystem.drag_drop(&[source], 1), vec![file]);
    }

    #[test]
    fn navigation_paths_and_file_associations_are_resolved() {
        let mut filesystem = FileSystem::new();
        let documents = filesystem.mkdir(1, "Documents");
        filesystem.navigate(documents);
        assert_eq!(
            filesystem.path(documents),
            Some(String::from("C:\\Documents"))
        );
        assert_eq!(filesystem.back(), Some(1));
        assert_eq!(filesystem.forward(), Some(documents));
        filesystem.associate("txt", "notepad.exe %1");
        assert_eq!(
            filesystem.association_for("计划.txt"),
            Some("notepad.exe %1")
        );
        filesystem.set_clipboard_text("hello");
        assert_eq!(filesystem.clipboard_text(), "hello");
    }

    #[test]
    fn links_expose_target_paths() {
        let mut filesystem = FileSystem::new();
        let link = filesystem.link(1, "Notepad.lnk", "C:\\Windows\\System32\\notepad.exe");
        assert_eq!(
            filesystem.node(link).unwrap().target.as_deref(),
            Some("C:\\Windows\\System32\\notepad.exe")
        );
    }
}
