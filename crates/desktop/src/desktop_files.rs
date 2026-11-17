//! Desktop filesystem projection and persistent Shell New templates.
use alloc::{format, string::String, vec::Vec};
use crate::{file::NodeKind, graphics::Rect, resources::ImageResource, shell::{DesktopItem, ShellModel, WindowKind}};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Editor {
    Rename { id: u32, value: String },
    Shortcut { target: String, name: String, naming: bool },
}
impl ShellModel {
    pub fn desktop_directory(&mut self) -> Option<u32> {
        let name = if self.session.identity.is_empty() { "Administrator" } else { &self.session.identity };
        self.files.resolve(&format!("C:\\Users\\{name}\\Desktop"))
    }
    /// Reconcile by node identity so refreshing preserves icon positions and selection.
    pub fn sync_desktop_files(&mut self) -> bool {
        let Some(directory) = self.desktop_directory() else { return false; };
        let selected: Vec<_> = self.desktop.selected.iter().filter_map(|i| self.desktop.items.get(*i))
            .map(|i| (i.file_id, i.name.clone())).collect();
        let nodes: Vec<_> = self.files.iter_children(directory).map(|n| (n.id, n.name.clone(), n.kind)).collect();
        let before = self.desktop.items.clone();
        self.desktop.items.retain(|i| i.file_id.is_none_or(|id| nodes.iter().any(|n| n.0 == id)));
        let rows = (self.viewport.1.saturating_sub(crate::shell::TASKBAR_HEIGHT + 12) / 96).max(1) as usize;
        for (id, name, kind) in nodes {
            let icon = if kind == NodeKind::Directory { ImageResource::Folder } else { ImageResource::Document };
            if let Some(item) = self.desktop.items.iter_mut().find(|i| i.file_id == Some(id)) {
                item.name = name; item.icon = icon;
            } else {
                let slot = (0..).find(|slot| {
                    let x = 12 + (*slot / rows) as i32 * 96;
                    let y = 12 + (*slot % rows) as i32 * 96;
                    !self.desktop.items.iter().any(|i| (i.x-x).abs()<90 && (i.y-y).abs()<90)
                }).unwrap_or(0);
                self.desktop.items.push(DesktopItem { name, icon, file_id: Some(id),
                    x: 12 + (slot / rows) as i32 * 96, y: 12 + (slot % rows) as i32 * 96 });
            }
        }
        self.desktop.selected = self.desktop.items.iter().enumerate().filter(|(_,i)|
            selected.iter().any(|(id,name)| *id == i.file_id && (*id != None || *name == i.name)))
            .map(|(i,_)|i).collect();
        before != self.desktop.items
    }
    pub fn refresh_desktop_files(&mut self) -> bool {
        let Some(directory) = self.desktop_directory() else { return false; };
        if let Err(status) = self.files.refresh_directory(directory) {
            self.desktop_error = format!("Cannot read Desktop (0x{status:08X})"); return false;
        }
        self.sync_desktop_files()
    }
    fn unique_desktop_name(&self, directory: u32, stem: &str, extension: &str) -> String {
        for n in 1..10000 {
            let name = if n == 1 { format!("{stem}{extension}") } else { format!("{stem} ({n}){extension}") };
            if !self.files.iter_children(directory).any(|i| i.name.eq_ignore_ascii_case(&name)) { return name; }
        }
        String::new()
    }
    pub fn create_desktop_item(&mut self, index: usize) -> bool {
        self.desktop_error.clear();
        if index == 1 {
            self.desktop_editor = Some(Editor::Shortcut { target: String::new(), name: "New Shortcut".into(), naming: false });
            return true;
        }
        let Some(directory) = self.desktop_directory() else {
            self.desktop_error = "The Desktop folder is unavailable.".into(); return true;
        };
        if let Err(status)=self.files.refresh_directory(directory) { self.desktop_error=format!("Cannot read Desktop (0x{status:08X})."); return true; }
        let (stem, extension, bytes) = match index {
            0 => ("New folder", "", Vec::new()),
            2 => ("New Text Document", ".txt", Vec::new()),
            3 => ("New Bitmap image", ".bmp", blank_bitmap()),
            4 => ("New Rich Text Document", ".rtf", b"{\\rtf1\\ansi\\deff0{\\fonttbl{\\f0 Arial;}}\\f0\\fs20\\par}".to_vec()),
            5 => ("New Compressed (zipped) Folder", ".zip", [b"PK\x05\x06".as_slice(), &[0;18]].concat()),
            6 => ("New Contact", ".contact", b"<?xml version=\"1.0\" encoding=\"UTF-8\"?><c:contact xmlns:c=\"http://schemas.microsoft.com/Contact\" c:Version=\"1\"/>".to_vec()),
            _ => return false,
        };
        let name = self.unique_desktop_name(directory, stem, extension);
        let id = if index == 0 { self.files.mkdir(directory, &name) } else { self.files.touch(directory, &name, &bytes) };
        if id == 0 { self.desktop_error = format!("Cannot create item (0x{:08X})", self.files.last_error.unwrap_or(0)); return true; }
        self.sync_desktop_files();
        self.desktop.selected = self.desktop.items.iter().position(|i| i.file_id == Some(id)).into_iter().collect();
        self.desktop_editor = Some(Editor::Rename { id, value: name });
        true
    }
    pub fn open_desktop_file(&mut self, id: u32) -> bool {
        let Some(node) = self.files.node(id) else { return false; };
        if node.kind == NodeKind::Directory {
            let title = node.name.clone();
            self.files.navigate(id); self.open_window_with_kind(&title, 80, 80, 640, 450, WindowKind::Explorer); return true;
        }
        if node.name.to_ascii_lowercase().ends_with(".lnk") {
            if self.files.load_file(id).is_err() { return false; }
            let target = self.files.node(id).and_then(|n| decode_shortcut(&n.contents));
            let Some(target) = target.and_then(|p| self.files.resolve(&p)) else {
                self.desktop_error = "The shortcut target cannot be found.".into(); return true;
            };
            if target == id || self.files.node(target).is_some_and(|n| n.name.to_ascii_lowercase().ends_with(".lnk")) { return false; }
            return self.open_desktop_file(target);
        }
        self.open_application_file(id).is_some()
    }
    pub fn desktop_editor_key(&mut self, key: u8) -> bool {
        if key == 27 { self.desktop_editor = None; self.desktop_error.clear(); return true; }
        let Some(mut editor) = self.desktop_editor.take() else { return false; };
        if matches!(key, 13 | 10) {
            match &mut editor {
                Editor::Rename { id, value } => {
                    if !self.files.rename(*id, value) { self.desktop_error = "Enter a valid, unique file name.".into(); self.desktop_editor = Some(editor); }
                    else { self.desktop_error.clear(); self.sync_desktop_files(); }
                }
                Editor::Shortcut { target, naming, .. } if !*naming => {
                    if !target.get(..3).is_some_and(|prefix| prefix.eq_ignore_ascii_case("C:\\")) || self.files.resolve(target).is_none() { self.desktop_error = "The target does not exist. Enter a full file or folder path.".into(); }
                    else { *naming = true; self.desktop_error.clear(); }
                    self.desktop_editor = Some(editor);
                }
                Editor::Shortcut { target, name, .. } => {
                    let file_name = if name.to_ascii_lowercase().ends_with(".lnk") { name.clone() } else { format!("{name}.lnk") };
                    let id = self.desktop_directory().map(|d| self.files.touch(d, &file_name, &encode_shortcut(target))).unwrap_or(0);
                    if id == 0 { self.desktop_error = "Cannot create shortcut. Choose a valid, unique name.".into(); self.desktop_editor = Some(editor); }
                    else { self.desktop_error.clear(); self.sync_desktop_files(); }
                }
            }
            return true;
        }
        let value = match &mut editor {
            Editor::Rename { value, .. } => value,
            Editor::Shortcut { target, name, naming } => if *naming { name } else { target },
        };
        if key == 8 { value.pop(); }
        else if key == 1 { value.clear(); }
        else if (32..=126).contains(&key) && value.len() < 240 { value.push(key as char); }
        self.desktop_editor = Some(editor); true
    }
    pub fn desktop_editor_bounds(&self) -> Rect {
        Rect::new((self.viewport.0 as i32 - 440)/2, (self.viewport.1 as i32 - 220)/2, 440, 220)
    }
    pub fn desktop_editor_click(&mut self, x: i32, y: i32) -> bool {
        if self.desktop_editor.is_none() && self.desktop_error.is_empty() { return false; }
        let b = self.desktop_editor_bounds();
        if Rect::new(b.x+332,b.y+178,88,26).contains(x,y) { self.desktop_editor=None;self.desktop_error.clear(); }
        else if Rect::new(b.x+232,b.y+178,88,26).contains(x,y) {
            if self.desktop_editor.is_some() { self.desktop_editor_key(13); } else { self.desktop_error.clear(); }
        }
        true
    }
}
fn blank_bitmap() -> Vec<u8> {
    let mut b = alloc::vec![0;58]; b[..2].copy_from_slice(b"BM");
    for (offset,value) in [(2,58u32),(10,54),(14,40),(18,1),(22,1),(34,4)] { b[offset..offset+4].copy_from_slice(&value.to_le_bytes()); }
    b[26]=1;b[28]=24;b[54..57].fill(255);b
}
/// MS-SHLLINK LinkInfo for a local absolute target; UTF-16 paths preserve names.
fn encode_shortcut(target: &str) -> Vec<u8> {
    let mut b = alloc::vec![0;76]; b[0]=76;
    b[4..20].copy_from_slice(&[1,20,2,0,0,0,0,0,192,0,0,0,0,0,0,70]);
    b[20..24].copy_from_slice(&0x82u32.to_le_bytes()); b[60]=1;
    let ansi:Vec<u8>=target.chars().map(|c|if c.is_ascii(){c as u8}else{b'?'}).chain(core::iter::once(0)).collect();
    let unicode:Vec<u8>=target.encode_utf16().chain(core::iter::once(0)).flat_map(u16::to_le_bytes).collect();
    // LinkInfoHeaderSize=0x24; fixed-drive VolumeID with an empty volume label.
    let local=36+20; let suffix=local+ansi.len(); let local_unicode=suffix+1;
    let suffix_unicode=local_unicode+unicode.len(); let size=suffix_unicode+2;
    for word in [size as u32,36,1,36,local as u32,0,suffix as u32,local_unicode as u32,suffix_unicode as u32] {
        b.extend_from_slice(&word.to_le_bytes());
    }
    for word in [20u32,3,0,16,0] {b.extend_from_slice(&word.to_le_bytes());}
    b.extend_from_slice(&ansi);b.push(0);b.extend_from_slice(&unicode);b.extend_from_slice(&[0;2]);
    b.extend_from_slice(&[0;4]);b
}
fn decode_shortcut(b: &[u8]) -> Option<String> {
    fn word(b:&[u8],offset:usize)->Option<u32>{Some(u32::from_le_bytes(b.get(offset..offset+4)?.try_into().ok()?))}
    if word(b,0)?!=76 || b.get(4..20)? != [1,20,2,0,0,0,0,0,192,0,0,0,0,0,0,70] { return None; }
    let flags=word(b,20)?;
    let mut offset=76usize;
    if flags&1!=0 {offset=offset.checked_add(2+u16::from_le_bytes(b.get(offset..offset+2)?.try_into().ok()?)as usize)?;}
    if flags&2==0 {return None;}
    let size=word(b,offset)? as usize;
    let end=offset.checked_add(size)?;
    b.get(end..end+4)?;
    let info=b.get(offset..end)?;
    if word(info,4)? < 36 || word(info,8)?&1==0 {return None;}
    let path_offset=word(info,28)?as usize;
    let path=info.get(path_offset..)?;
    let mut units=Vec::new();
    for pair in path.chunks_exact(2) {
        let unit=u16::from_le_bytes([pair[0],pair[1]]);
        if unit==0 {return String::from_utf16(&units).ok();}
        units.push(unit);
    }
    None
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn desktop_projection_and_new_editor_lifecycle() {
        let mut shell=ShellModel::new(crate::theme::Theme::aero());
        shell.refresh_controls(1024,768);
        let root=shell.files.root_id();
        let users=shell.files.mkdir(root,"Users");
        let account=shell.files.mkdir(users,"Administrator");
        let desktop=shell.files.mkdir(account,"Desktop");
        assert_ne!(desktop,0);
        // Explorer/provider changes are reconciled without recreating stable IDs.
        let external=shell.files.touch(desktop,"external.txt",b"data");
        assert!(shell.sync_desktop_files());
        let index=shell.desktop.items.iter().position(|i|i.file_id==Some(external)).unwrap();
        shell.desktop.selected=alloc::vec![index];
        let position=(shell.desktop.items[index].x,shell.desktop.items[index].y);
        assert!(shell.files.rename(external,"renamed.txt"));
        assert!(shell.sync_desktop_files());
        let selected=&shell.desktop.items[shell.desktop.selected[0]];
        assert_eq!(selected.name,"renamed.txt");assert_eq!((selected.x,selected.y),position);
        assert!(shell.files.recycle_permanently(external));assert!(shell.sync_desktop_files());
        assert!(shell.desktop.selected.is_empty());
        for item in [0,2,3,4,5,6] {
            assert!(shell.create_desktop_item(item));
            let Some(Editor::Rename{id,..})=shell.desktop_editor.clone() else {panic!("rename expected")};
            assert!(shell.files.node(id).is_some());
            shell.desktop_editor_key(13);assert!(shell.desktop_editor.is_none());
        }
        shell.create_desktop_item(2);shell.desktop_editor_key(13);
        assert!(shell.files.resolve("C:\\Users\\Administrator\\Desktop\\New Text Document (2).txt").is_some());
        shell.create_desktop_item(1);
        for key in b"C:\\Users\\Administrator\\Desktop\\New Text Document.txt" {shell.desktop_editor_key(*key);}
        shell.desktop_editor_key(13);
        assert!(matches!(shell.desktop_editor,Some(Editor::Shortcut{naming:true,..})));
        shell.desktop_editor_key(1);
        for key in b"Link" {shell.desktop_editor_key(*key);}
        shell.desktop_editor_key(13);assert!(shell.desktop_editor.is_none());
        let link=shell.files.resolve("C:\\Users\\Administrator\\Desktop\\Link.lnk").unwrap();
        assert_eq!(decode_shortcut(&shell.files.node(link).unwrap().contents).as_deref(),Some("C:\\Users\\Administrator\\Desktop\\New Text Document.txt"));
        assert!(!shell.create_desktop_item(99));
    }
    #[test] fn templates_and_shortcuts_have_valid_payloads() {
        assert!(crate::bmp::decode_bmp(&blank_bitmap()).is_ok());
        let path="C:\\Users\\Administrator\\Desktop\\目标.txt";
        let encoded=encode_shortcut(path);assert_eq!(decode_shortcut(&encoded).as_deref(),Some(path));
        for n in 0..encoded.len()-4 { assert!(decode_shortcut(&encoded[..n]).is_none()); }
    }
}
