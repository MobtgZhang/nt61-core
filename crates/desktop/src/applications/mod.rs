//! Built-in applications. Core behavior is shared by Explorer-hosted windows and native PE images.
//! Native application identities and compatibility limits live in `catalog`.
use alloc::{boxed::Box,string::String,vec::Vec};
use crate::graphics::Surface;
pub mod calculator;
pub mod associations;
pub mod command_prompt;
pub mod catalog;
pub mod document;
pub mod file_dialog;
pub mod editor_view;
pub mod explorer;
pub mod host;
pub mod icons;
pub mod minesweeper;
pub mod paint;
pub mod processes;
pub mod resource_monitor;
pub mod view;
pub use catalog::{AppId,APPLICATIONS};
use calculator::Calculator;
use document::Document;
use minesweeper::Minesweeper;
use paint::Paint;
#[derive(Clone,Debug,PartialEq,Eq)]
pub enum AppAction { None,Close,RequestClose,Open,SaveAs,NewDocument,Launch(AppId),Save(String,Vec<u8>),Copy(String),Paste,Activate(u32),EndTask(u32),Type(u8),Capture }
#[derive(Clone,Debug,PartialEq)]
pub enum AppState { CommandPrompt(command_prompt::CommandPrompt),Calculator(Calculator),Paint(Box<Paint>),Document(Document),CharacterMap{page:u32,selected:u32},
    TaskManager{selected:Option<u32>,page:usize,offset:usize},Run{command:String},Keyboard{shift:bool},Magnifier{zoom:u32,image:Option<Surface>},
    Snipping{image:Option<Surface>},Minesweeper(Box<Minesweeper>),ResourceMonitor(resource_monitor::Monitor),Information{page:usize} }
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Pending { New, Open, Close }
#[derive(Clone,Debug,PartialEq)]
pub struct ApplicationSession {pub window_id:u32,pub id:AppId,pub state:AppState,pub status:String,pub hot:Option<crate::graphics::Rect>,pub dialog:Option<file_dialog::FileDialog>,pub pending:Option<Pending>,pub confirm:bool,pub menu:bool,pub menu_kind:u8,pub search:Option<editor_view::Search>,pub decoder:explorer::input::Decoder,pub input_time:u64}
#[derive(Clone,Debug,PartialEq,Eq)]pub struct Task {pub id:u32,pub title:String,pub minimized:bool}
#[derive(Clone,Debug,PartialEq,Eq)]pub struct Environment {pub width:u32,pub height:u32,pub network_adapters:u32,pub audio:bool,pub tasks:Vec<Task>,pub processes:Option<processes::Snapshot>}
impl ApplicationSession {
    pub fn new(window_id:u32,id:AppId,seed:u64)->Self {
        let state=match id {
            AppId::CommandPrompt=>AppState::CommandPrompt(command_prompt::CommandPrompt::new()),
            AppId::BcdEdit=>AppState::CommandPrompt(command_prompt::CommandPrompt::bcdedit()),
            AppId::DiskPart=>AppState::CommandPrompt(command_prompt::CommandPrompt::diskpart()),
            AppId::Calculator=>AppState::Calculator(Calculator::default()),
            AppId::Paint=>AppState::Paint(Box::new(Paint::new(480,240))),
            AppId::StickyNotes|AppId::WordPad|AppId::Notepad=>AppState::Document(Document::new(if id==AppId::WordPad{"Document.rtf"}else if id==AppId::StickyNotes{"StickyNote.txt"}else{"Untitled.txt"})),
            AppId::CharacterMap=>AppState::CharacterMap{page:32,selected:65},
            AppId::TaskManager=>AppState::TaskManager{selected:None,page:0,offset:0},
            AppId::Run=>AppState::Run{command:String::new()},
            AppId::OnScreenKeyboard=>AppState::Keyboard{shift:false},
            AppId::Magnifier=>AppState::Magnifier{zoom:2,image:None},
            AppId::SnippingTool=>AppState::Snipping{image:None},
            AppId::Minesweeper=>AppState::Minesweeper(Box::new(Minesweeper::new(seed))),
            AppId::ResourceMonitor=>AppState::ResourceMonitor(resource_monitor::Monitor::default()),
            _=>AppState::Information{page:0},
        };Self{window_id,id,state,status:String::new(),hot:None,dialog:None,pending:None,confirm:false,menu:false,menu_kind:0,search:None,decoder:explorer::input::Decoder::default(),input_time:0}
    }
    pub fn key(&mut self,key:u8)->AppAction {
        if let Some(search)=&mut self.search{if key==27{self.search=None;}else if let AppState::Document(d)=&mut self.state{search.key(d,key);}return AppAction::None;}
        if matches!(self.state,AppState::Document(_)|AppState::Paint(_)) {
            match key {15=>return AppAction::Open,14=>return AppAction::NewDocument,19=>return self.save(),_=>{}}
        }
        match &mut self.state {
        AppState::Calculator(c)=>c.press(key),
        AppState::Document(d)=>{match key {26=>d.undo(),25=>d.redo(),3=>return AppAction::Copy(d.selected_text()),24=>return AppAction::Copy(d.cut()),22=>return AppAction::Paste,6=>self.search=Some(editor_view::Search::new(false)),_=>{d.key(key);}}},
        AppState::Paint(p)=>match key{26=>p.undo(),25=>p.redo(),explorer::input::LEFT=>p.scroll.0=p.scroll.0.saturating_sub(32),explorer::input::RIGHT=>p.scroll.0=(p.scroll.0+32).min(p.canvas.width()-1),explorer::input::UP=>p.scroll.1=p.scroll.1.saturating_sub(32),explorer::input::DOWN=>p.scroll.1=(p.scroll.1+32).min(p.canvas.height()-1),_=>{}},
        AppState::Run{command}=>match key {8=>{command.pop();},13|10=>{return self.run();},0x20..=0x7e=>{if command.len()<260{command.push(key as char);}},_=>{}},
        AppState::Minesweeper(g) if key==b'n'||key==b'N'=>{**g=Minesweeper::new(g.mines.iter().filter(|m|**m).count()as u64+77);},
        _=>{}
    }AppAction::None}
    pub fn save(&mut self)->AppAction {match &mut self.state {
        AppState::Document(d)=>AppAction::Save(d.filename.clone(),if self.id==AppId::WordPad{d.rtf()}else{d.encoded()}),
        AppState::Paint(p)=>{p.end();AppAction::Save(p.filename.clone(),paint::encode_bmp(&p.canvas))},
        AppState::Snipping{image:Some(image)}=>AppAction::Save("Capture.bmp".into(),paint::encode_bmp(image)),
        _=>AppAction::None,
    }}
    pub fn dirty(&self)->bool{match &self.state{AppState::Document(d)=>d.dirty,AppState::Paint(p)=>p.dirty,_=>false}}
    pub fn current_file(&self)->Option<u32>{match &self.state{AppState::Document(d)=>d.file_id,AppState::Paint(p)=>p.file_id,_=>None}}
    pub fn file_name(&self)->&str{match &self.state{AppState::Document(d)=>&d.filename,AppState::Paint(p)=>&p.filename,_=>"Capture.bmp"}}
    pub fn saved(&mut self,id:u32,name:String){match &mut self.state{AppState::Document(d)=>{d.file_id=Some(id);d.filename=name;d.mark_saved();},AppState::Paint(p)=>{p.file_id=Some(id);p.filename=name;p.mark_saved();},_=>{}}}
    pub fn run(&mut self)->AppAction {if let AppState::Run{command}=&self.state{if let Some(id)=catalog::resolve(command){return AppAction::Launch(id);}self.status="Program not found. Enter a registered executable name.".into();}AppAction::None}
    pub fn capture(&mut self,image:Surface){match &mut self.state{AppState::Snipping{image:target}|AppState::Magnifier{image:target,..}=>*target=Some(image),_=>{}}}
    pub fn paste(&mut self,text:&str){if let AppState::Document(d)=&mut self.state{d.insert(text);}}
}
