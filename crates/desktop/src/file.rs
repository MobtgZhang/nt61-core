use alloc::string::String;
use alloc::vec::Vec;

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
        };
        filesystem.next_id = 2;
        filesystem.nodes.push(FileNode {
            id: 1,
            name: String::from("C:"),
            kind: NodeKind::Directory,
            parent: 0,
            contents: Vec::new(),
            deleted: false,
            original_path: None,
            deleted_at: 0,
            target: None,
        });
        filesystem.navigation.push(1);
        filesystem
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
            deleted: false,
            original_path: None,
            deleted_at: 0,
            target: None,
        });
        id
    }

    pub fn mkdir(&mut self, parent: u32, name: &str) -> u32 {
        self.create_node(name, NodeKind::Directory, parent)
    }

    pub fn touch(&mut self, parent: u32, name: &str, contents: &[u8]) -> u32 {
        let id = self.create_node(name, NodeKind::File, parent);
        if let Some(node) = self.node_mut(id) {
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

    fn copy_one(&mut self, source: u32, destination: u32) -> Option<u32> {
        let node = self.node(source).cloned()?;
        let id = self.next_id;
        self.next_id += 1;
        self.nodes.push(FileNode { id, ..node });
        if let Some(node) = self.node_mut(id) {
            node.parent = destination;
            node.name = String::from("Copy of ") + &node.name;
        }
        Some(id)
    }

    pub fn recycle(&mut self, id: u32, now: u64) -> bool {
        self.node_mut(id)
            .map(|node| {
                node.deleted = true;
                node.deleted_at = now;
                node.original_path = Some(String::from("C:\\") + &node.name);
            })
            .is_some()
    }

    pub fn recycle_permanently(&mut self, id: u32) -> bool {
        self.node_mut(id).map(|node| node.deleted = true).is_some()
    }

    pub fn restore(&mut self, id: u32) -> bool {
        self.node_mut(id)
            .map(|node| {
                node.deleted = false;
                node.original_path = None;
                node.deleted_at = 0;
            })
            .is_some()
    }

    pub fn restore_to(&mut self, id: u32, parent: u32) -> bool {
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

impl Default for FileSystem {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
