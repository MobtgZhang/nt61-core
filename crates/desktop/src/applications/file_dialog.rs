//! Common file picker backed by the same VFS as Explorer. No sample file list.
use alloc::{format, string::{String}, vec::Vec};
use crate::{file::{FileSystem, NodeKind}, graphics::{Rect, Surface}, compositor::aero};
use super::explorer::input;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode { Open, SaveAs }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter { Text, RichText, Bitmap, All }
impl Filter {
    fn accepts(self, name: &str) -> bool {
        let name = name.to_ascii_lowercase();
        match self { Self::Text => name.ends_with(".txt"), Self::RichText => name.ends_with(".rtf"), Self::Bitmap => name.ends_with(".bmp"), Self::All => true }
    }
    pub fn label(self) -> &'static str { match self { Self::Text=>"Text Documents (*.txt)", Self::RichText=>"Rich Text (*.rtf)", Self::Bitmap=>"Bitmap (*.bmp)", Self::All=>"All Files (*.*)" } }
    fn extension(self) -> &'static str { match self { Self::Text=>".txt", Self::RichText=>".rtf", Self::Bitmap=>".bmp", Self::All=>"" } }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry { pub id: u32, pub name: String, pub directory: bool, pub size: u64 }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus { Name, Address, List, NewFolder }
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileDialog {
    pub mode: Mode, pub directory: u32, pub address: String, pub name: String,
    pub filter: Filter, default_filter: Filter, pub entries: Vec<Entry>, pub selected: Option<usize>,
    pub offset: usize, pub visible_rows: usize, pub focus: Focus, pub status: String, pub overwrite: Option<u32>,
    pub folder_name: String, select_all: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Result { None, Cancel, Open(u32), Save { parent: u32, name: String, existing: Option<u32> } }
impl FileDialog {
    pub fn new(files: &mut FileSystem, mode: Mode, current: Option<u32>, name: &str, filter: Filter) -> Self {
        let directory = current.and_then(|id| files.node(id).map(|n| n.parent)).filter(|id|*id!=0)
            .or_else(||files.resolve("C:\\Users\\Administrator\\Documents"))
            .unwrap_or(files.root_id());
        let mut dialog = Self { mode, directory, address: String::new(), name: name.into(), filter, default_filter: filter,
            entries: Vec::new(), selected: None, offset: 0, visible_rows: 6, focus: Focus::Name, status: String::new(),
            overwrite: None, folder_name: String::new(), select_all: true };
        dialog.refresh(files); dialog
    }
    pub fn refresh(&mut self, files: &mut FileSystem) {
        self.entries.clear(); self.selected=None; self.offset=0;
        self.address=files.path(self.directory).unwrap_or_default();
        if let Err(status)=files.refresh_directory(self.directory) { self.status=format!("Cannot read folder (0x{status:08X})"); return; }
        self.entries=files.children(self.directory).iter().filter(|n|n.kind==NodeKind::Directory||self.filter.accepts(&n.name))
            .map(|n|Entry{id:n.id,name:n.name.clone(),directory:n.kind==NodeKind::Directory,size:n.size.unwrap_or(n.contents.len() as u64)}).collect();
        self.entries.sort_by(|a,b|b.directory.cmp(&a.directory).then_with(||a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase())));
        self.status.clear();
    }
    fn navigate(&mut self, files: &mut FileSystem, id: u32) {
        if files.node(id).is_some_and(|n|n.kind==NodeKind::Directory) {self.directory=id;self.refresh(files);}
        else { self.status="The folder does not exist.".into(); }
    }
    pub fn submit(&mut self, files: &mut FileSystem) -> Result {
        if self.focus==Focus::NewFolder {
            let id=files.mkdir(self.directory,self.folder_name.trim());
            if id==0 {self.status=format!("Cannot create folder (0x{:08X})",files.last_error.unwrap_or(0));}
            else {self.focus=Focus::Name;self.refresh(files);}
            return Result::None;
        }
        if self.focus==Focus::Address {
            if let Some(id)=files.resolve(&if self.address.as_bytes().get(1)==Some(&b':') { self.address.clone() } else {format!("{}\\{}",files.path(self.directory).unwrap_or_default(),self.address.trim())}) { self.navigate(files,id); }
            else {self.status="The folder does not exist.".into();}
            return Result::None;
        }
        if self.focus==Focus::List {
            if let Some(entry)=self.selected.and_then(|i|self.entries.get(i)) {
                if entry.directory {let id=entry.id;self.navigate(files,id);return Result::None;}
                self.name=entry.name.clone();
            }
        }
        let name=self.name.trim();
        if name.is_empty() {self.status="Enter a file name.".into();return Result::None;}
        // Filename input is relative to the last successfully opened directory,
        // never an unsubmitted edit in the address field.
        let base=files.path(self.directory).unwrap_or_default();
        let mut path=if name.as_bytes().get(1)==Some(&b':'){name.into()}else if name.starts_with(['\\','/']){format!("C:{name}")}else{format!("{}\\{name}",base.trim_end_matches('\\'))};
        if let Some(id)=files.resolve(&path) {
            if files.node(id).is_some_and(|n|n.kind==NodeKind::Directory){self.navigate(files,id);return Result::None;}
            if self.mode==Mode::Open {return Result::Open(id);}
        }
        if self.mode==Mode::SaveAs && files.resolve(&path).is_none() && !name.rsplit(['\\','/']).next().unwrap_or(name).contains('.') {path.push_str(self.filter.extension());}
        let existing=files.resolve(&path);
        if self.mode==Mode::Open {self.status="File not found. Check the file name.".into();return Result::None;}
        let Some((parent,name))=path.rsplit_once(['\\','/']) else {return Result::None;};
        let parent=if parent.ends_with(':'){format!("{parent}\\")}else{parent.into()};
        let Some(parent)=files.resolve(&parent).filter(|id|files.node(*id).is_some_and(|n|n.kind==NodeKind::Directory)) else {self.status="The destination folder does not exist.".into();return Result::None;};
        if !crate::file::valid_component(name) {self.status="The file name is invalid.".into();return Result::None;}
        if let Some(id)=existing {
            if self.overwrite!=Some(id) {self.overwrite=Some(id);self.status=format!("{name} already exists. Replace it?");return Result::None;}
        }
        Result::Save{parent,name:name.into(),existing}
    }
    pub fn key(&mut self, files: &mut FileSystem, key: u8) -> Result {
        if key==27 {if self.overwrite.take().is_some(){self.status.clear();return Result::None;}return Result::Cancel;}
        if self.overwrite.is_some() {return match key {b'y'|b'Y'|13=>self.submit(files), b'n'|b'N'=>{self.overwrite=None;self.status.clear();Result::None},_=>Result::None};}
        match key {
            9=>{self.focus=match self.focus{Focus::Name=>Focus::Address,Focus::Address=>Focus::List,_=>Focus::Name};self.select_all=true;},
            1=>self.select_all=true,
            13|10=>return self.submit(files),
            input::UP|input::DOWN|input::PAGE_DOWN|input::PAGE_UP=>{
                self.focus=Focus::List;let count=self.entries.len();if count!=0{
                    let i=match key {input::UP=>self.selected.unwrap_or(0).saturating_sub(1),input::PAGE_UP=>self.selected.unwrap_or(0).saturating_sub(self.visible_rows),input::PAGE_DOWN=>self.selected.unwrap_or(0).saturating_add(self.visible_rows).min(count-1),_=>self.selected.map_or(0,|i|(i+1).min(count-1))};
                    self.selected=Some(i);if i<self.offset{self.offset=i;}if i>=self.offset+self.visible_rows{self.offset=i+1-self.visible_rows;}
                    if !self.entries[i].directory{self.name=self.entries[i].name.clone();}
                }
            },
            8|0x20..=0x7e if self.focus!=Focus::List=>{
                let field=match self.focus{Focus::Address=>&mut self.address,Focus::NewFolder=>&mut self.folder_name,_=>&mut self.name};
                if self.select_all{field.clear();self.select_all=false;}
                if key==8{field.pop();}else if field.len()<1000{field.push(key as char);}
                self.overwrite=None;
            },_=>{}
        } Result::None
    }
    pub fn click(&mut self, files: &mut FileSystem, body: Rect, x: i32, y: i32) -> Result {
        let r=bounds(body);
        self.visible_rows=(list_bounds(body).height/24).max(1) as usize;
        if self.overwrite.is_some() {
            if Rect::new(r.x+12,r.y+r.height as i32-38,100,28).contains(x,y){return self.submit(files);}
            if Rect::new(r.x+124,r.y+r.height as i32-38,100,28).contains(x,y){self.overwrite=None;self.status.clear();}
            return Result::None;
        }
        if Rect::new(r.x+12,r.y+36,52,26).contains(x,y){if let Some(id)=files.node(self.directory).map(|n|n.parent).filter(|id|*id!=0){self.navigate(files,id);}return Result::None;}
        if Rect::new(r.x+70,r.y+36,r.width.saturating_sub(82),26).contains(x,y){self.focus=Focus::Address;self.select_all=true;return Result::None;}
        if Rect::new(r.x+12,r.y+68,100,26).contains(x,y){self.focus=Focus::NewFolder;self.folder_name="New folder".into();self.select_all=true;return Result::None;}
        let list=list_bounds(body);
        if list.contains(x,y){let index=self.offset+((y-list.y)/24)as usize;if let Some(entry)=self.entries.get(index){let activate=self.selected==Some(index);self.selected=Some(index);self.focus=Focus::List;if !entry.directory{self.name=entry.name.clone();}if activate{return self.submit(files);}}return Result::None;}
        let bottom=r.y+r.height as i32;
        if Rect::new(r.x+104,bottom-112,r.width.saturating_sub(120),26).contains(x,y){self.focus=Focus::Name;self.select_all=true;}
        if Rect::new(r.x+104,bottom-80,r.width.saturating_sub(120),26).contains(x,y){self.filter=if self.filter==Filter::All{self.default_filter}else{Filter::All};self.refresh(files);}
        if Rect::new(r.x+r.width as i32-224,bottom-38,100,28).contains(x,y){self.focus=Focus::Name;return self.submit(files);}
        if Rect::new(r.x+r.width as i32-112,bottom-38,100,28).contains(x,y){return Result::Cancel;}
        Result::None
    }
}
pub fn bounds(body: Rect)->Rect {Rect::new(body.x+6,body.y+6,body.width.saturating_sub(12),body.height.saturating_sub(12))}
pub fn list_bounds(body:Rect)->Rect{let r=bounds(body);Rect::new(r.x+12,r.y+102,r.width.saturating_sub(24),r.height.saturating_sub(224))}
pub fn draw<F:Fn(&mut Surface,&str,Rect,u32)>(d:&FileDialog,frame:&mut Surface,body:Rect,text:&F){
    let r=bounds(body);frame.fill_rect(r.x,r.y,r.width,r.height,0xFFF0F0F0);
    text(frame,if d.mode==Mode::Open{"Open"}else{"Save As"},Rect::new(r.x+12,r.y+8,r.width-24,22),0xFF163B60);
    let mut label=|s:&str,b:Rect,active:bool|{aero::menu_item(frame,b,active);text(frame,s,Rect::new(b.x+5,b.y+4,b.width.saturating_sub(10),22),0xFF162A3F);};
    label("Up",Rect::new(r.x+12,r.y+36,52,26),false);
    label(&d.address,Rect::new(r.x+70,r.y+36,r.width.saturating_sub(82),26),d.focus==Focus::Address);
    label("New folder",Rect::new(r.x+12,r.y+68,100,26),d.focus==Focus::NewFolder);
    if d.focus==Focus::NewFolder{label(&d.folder_name,Rect::new(r.x+122,r.y+68,r.width.saturating_sub(134),26),true);}
    let list=list_bounds(body);frame.fill_rect(list.x,list.y,list.width,list.height,0xFFFFFFFF);
    for(i,e)in d.entries.iter().enumerate().skip(d.offset).take((list.height/24)as usize){let row=Rect::new(list.x,list.y+((i-d.offset)*24)as i32,list.width,24);aero::menu_item(frame,row,d.selected==Some(i));text(frame,&format!("{} {}",if e.directory{"[Folder]"}else{"        "},e.name),Rect::new(row.x+4,row.y+3,row.width-8,21),0xFF162A3F);}
    let bottom=r.y+r.height as i32;
    text(frame,"File name:",Rect::new(r.x+12,bottom-109,88,24),0xFF162A3F);
    let n=Rect::new(r.x+104,bottom-112,r.width.saturating_sub(120),26);aero::menu_item(frame,n,d.focus==Focus::Name);text(frame,&d.name,Rect::new(n.x+4,n.y+4,n.width-8,22),0xFF162A3F);
    text(frame,"File type:",Rect::new(r.x+12,bottom-77,88,24),0xFF162A3F);
    let n=Rect::new(r.x+104,bottom-80,r.width.saturating_sub(120),26);aero::menu_item(frame,n,false);text(frame,d.filter.label(),Rect::new(n.x+4,n.y+4,n.width-8,22),0xFF162A3F);
    text(frame,&d.status,Rect::new(r.x+12,bottom-53,r.width-24,22),0xFF974321);
    for(label,b)in if d.overwrite.is_some(){[("Replace",Rect::new(r.x+12,bottom-38,100,28)),("Keep file",Rect::new(r.x+124,bottom-38,100,28))]}else{[(if d.mode==Mode::Open{"Open"}else{"Save"},Rect::new(r.x+r.width as i32-224,bottom-38,100,28)),("Cancel",Rect::new(r.x+r.width as i32-112,bottom-38,100,28))]}{aero::menu_item(frame,b,true);text(frame,label,Rect::new(b.x+8,b.y+5,b.width-16,22),0xFF162A3F);}
}

#[cfg(test)]mod tests{
    use super::*;
    #[test]fn navigation_filters_folders_and_absolute_names_use_the_real_tree(){
        let mut fs=FileSystem::default();let dir=fs.mkdir(1,"pictures");let image=fs.touch(dir,"one.bmp",b"bmp");fs.touch(dir,"hidden.txt",b"text");
        let mut d=FileDialog::new(&mut fs,Mode::Open,None,"",Filter::Bitmap);d.name="pictures".into();assert_eq!(d.submit(&mut fs),Result::None);assert_eq!(d.directory,dir);assert_eq!(d.entries.len(),1);assert_eq!(d.entries[0].id,image);
        d.name="one.bmp".into();assert_eq!(d.submit(&mut fs),Result::Open(image));
        d.focus=Focus::NewFolder;d.folder_name="new folder".into();d.submit(&mut fs);assert!(fs.resolve("C:\\pictures\\new folder").is_some());
        d.focus=Focus::Name;d.name="C:\\pictures\\missing.bmp".into();assert_eq!(d.submit(&mut fs),Result::None);assert!(d.status.contains("not found"));
    }
    #[test]fn filename_uses_navigated_folder_and_invalid_names_never_write(){
        let mut fs=FileSystem::default();let mut d=FileDialog::new(&mut fs,Mode::SaveAs,None,"",Filter::Text);
        d.address="C:\\not-submitted".into();d.name="new".into();assert_eq!(d.submit(&mut fs),Result::Save{parent:1,name:"new.txt".into(),existing:None});
        d.name="bad?.txt".into();assert_eq!(d.submit(&mut fs),Result::None);assert!(!d.status.is_empty());assert!(fs.children(1).is_empty());
    }
}
