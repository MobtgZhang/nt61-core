//! Window console. Mounted sessions dispatch to the system command service.
use alloc::{format,string::String,vec::Vec};
use crate::file::{FileSystem,NodeKind};
use super::{AppAction,catalog,explorer::input::{self,Decoder}};
#[derive(Clone,Debug,PartialEq,Eq)]pub struct CommandPrompt {pub input:String,pub lines:Vec<String>,pub directory:u32,pub cwd:String,pub cursor:usize,history:Vec<String>,history_index:Option<usize>,draft:String,decoder:Decoder,pub disk_focus:Option<[u32;3]>,pub diskpart_only:bool,pub bcdedit_only:bool}
impl CommandPrompt {
    pub fn new()->Self{Self{input:String::new(),lines:alloc::vec!["NT61 [Version 6.1.7601]".into(),"Type help for commands.".into()],directory:1,cwd:"C:\\".into(),cursor:0,history:Vec::new(),history_index:None,draft:String::new(),decoder:Decoder::default(),disk_focus:None,diskpart_only:false,bcdedit_only:false}}
    pub fn bcdedit()->Self{let mut c=Self::new();c.bcdedit_only=true;c.lines=alloc::vec!["Boot Configuration Data Editor".into(),"Enter BCDEdit options, e.g. /enum all, /export C:\\store.bcd, /?".into(),"EXIT closes this console. Active ESP edits require a writable mount.".into()];c}
    pub fn diskpart()->Self{let mut c=Self::new();c.diskpart_only=true;c.enter_diskpart();c}
    fn enter_diskpart(&mut self){self.disk_focus=Some([u32::MAX;3]);self.lines.push("DiskPart version 6.1.7601 (NT61)".into());self.lines.push("Type HELP for partition commands; EXIT returns to CMD.".into());}
    pub fn display_prompt(&self)->&str{if self.bcdedit_only{"BCDEDIT"}else if self.disk_focus.is_some(){"DISKPART"}else{&self.cwd}}
    pub fn prompt(&self,files:&FileSystem)->String{if files.is_mounted(){self.cwd.clone()}else{files.path(self.directory).unwrap_or_else(||"C:\\".into())}}
    /// Console navigation arrives as ANSI sequences from all four input backends.
    pub fn key(&mut self, files: &mut FileSystem, byte: u8) -> AppAction {
        let Some(key) = self.decoder.feed(byte, 0) else { return AppAction::None; };
        self.cursor = self.valid_cursor();
        match key {
            13 | 10 => return self.execute(files),
            3 => { self.lines.push(format!("{}>{}^C", self.cwd, self.input)); self.reset_input(); }
            input::UP => self.recall(true),
            input::DOWN => self.recall(false),
            input::LEFT => { if self.cursor > 0 { self.cursor = self.input[..self.cursor].char_indices().next_back().map_or(0, |(i,_)| i); } }
            input::RIGHT => { if let Some(c) = self.input[self.cursor..].chars().next() { self.cursor += c.len_utf8(); } }
            input::HOME => self.cursor = 0,
            input::END => self.cursor = self.input.len(),
            input::DELETE => { if self.cursor < self.input.len() { self.input.remove(self.cursor); } }
            8 => { if self.cursor > 0 { let previous = self.input[..self.cursor].char_indices().next_back().map_or(0, |(i,_)| i); self.input.drain(previous..self.cursor); self.cursor = previous; } }
            0x20..=0x7e if self.input.len() < 1024 => { self.input.insert(self.cursor, key as char); self.cursor += 1; }
            _ => {}
        }
        AppAction::None
    }
    pub fn display_input(&self) -> String {
        let cursor = self.valid_cursor();
        format!("{}|{}", &self.input[..cursor], &self.input[cursor..])
    }
    fn valid_cursor(&self) -> usize {
        let mut cursor = self.cursor.min(self.input.len());
        while !self.input.is_char_boundary(cursor) { cursor -= 1; }
        cursor
    }
    fn reset_input(&mut self) {
        self.input.clear(); self.cursor = 0; self.history_index = None; self.draft.clear();
    }
    fn recall(&mut self, previous: bool) {
        if self.history.is_empty() { return; }
        let index = if previous {
            match self.history_index {
                None => { self.draft.clone_from(&self.input); self.history.len() - 1 }
                Some(i) => i.saturating_sub(1),
            }
        } else {
            let Some(i) = self.history_index else { return; };
            i + 1
        };
        if index >= self.history.len() {
            self.input.clone_from(&self.draft); self.history_index = None;
        } else {
            self.input.clone_from(&self.history[index]); self.history_index = Some(index);
        }
        self.cursor = self.input.len();
    }
    pub fn execute(&mut self,files:&mut FileSystem)->AppAction{
        if !self.input.trim().is_empty() && self.history.last() != Some(&self.input) {
            if self.history.len() == 64 { self.history.remove(0); }
            self.history.push(self.input.clone());
        }
        self.cursor = 0; self.history_index = None; self.draft.clear();
        if self.bcdedit_only||self.disk_focus.is_some()||files.is_mounted(){return self.execute_system();}

        let input=core::mem::take(&mut self.input);self.lines.push(format!("{}>{input}",files.path(self.directory).unwrap_or_else(||"C:\\".into())));let input=input.trim();let (command,args)=input.split_once(' ').unwrap_or((input,""));let args=args.trim().trim_matches('"');
        match command.to_ascii_lowercase().as_str(){
            ""=>{},"help"=>self.lines.push("ver, cls, echo, dir, cd, mkdir, type, del, start <app>".into()),
            "ver"=>self.lines.push("NT61 version 6.1.7601".into()),"cls"=>self.lines.clear(),"echo"=>self.lines.push(args.into()),
            "dir"=>{self.lines.push(format!("Directory of {}",files.path(self.directory).unwrap_or_default()));for node in files.children(self.directory){self.lines.push(format!("{}  {}",if node.kind==NodeKind::Directory{"<DIR>".into()}else{format!("{}",node.contents.len())},node.name));}},
            "cd"=>{if args.is_empty(){self.lines.push(files.path(self.directory).unwrap_or_default());}else{let target=if args==".."{files.node(self.directory).map(|n|n.parent).filter(|id|*id!=0)}else if args.eq_ignore_ascii_case("C:\\"){Some(1)}else{files.children(self.directory).iter().find(|n|n.name.eq_ignore_ascii_case(args)&&n.kind==NodeKind::Directory).map(|n|n.id)};if let Some(id)=target{self.directory=id;}else{self.lines.push("The system cannot find the path specified.".into());}}},
            "mkdir"|"md"=>{if args.is_empty()||args.contains(['\\','/']){self.lines.push("Specify one folder name.".into());}else if files.children(self.directory).iter().any(|n|n.name.eq_ignore_ascii_case(args)){self.lines.push("A file or directory already exists.".into());}else{files.mkdir(self.directory,args);}},
            "type"=>{if let Some(node)=files.children(self.directory).iter().find(|n|n.name.eq_ignore_ascii_case(args)&&n.kind==NodeKind::File){self.lines.extend(String::from_utf8_lossy(&node.contents).lines().map(String::from));}else{self.lines.push("File not found.".into());}},
            "del"=>{let id=files.children(self.directory).iter().find(|n|n.name.eq_ignore_ascii_case(args)&&n.kind==NodeKind::File).map(|n|n.id);if let Some(id)=id{files.recycle(id,0);}else{self.lines.push("File not found.".into());}},
            "start"=>{if let Some(id)=catalog::resolve(args){return AppAction::Launch(id);}self.lines.push("Program not found.".into());},
            _=>{if let Some(id)=catalog::resolve(input){return AppAction::Launch(id);}self.lines.push("Command not recognized. Type help.".into());}
        }if self.lines.len()>128{self.lines.drain(..self.lines.len()-128);}AppAction::None
    }
    fn execute_system(&mut self)->AppAction{
        let input=core::mem::take(&mut self.input);
        self.lines.push(format!("{}>{input}",self.display_prompt()));
        let prefixed;
        let mut line=input.trim();
        if self.bcdedit_only{if line.eq_ignore_ascii_case("exit"){return AppAction::Close;}if !line.to_ascii_lowercase().starts_with("bcdedit"){prefixed=format!("bcdedit {line}");line=&prefixed;}}
        if let Some(focus)=self.disk_focus{
            match crate::platform::services::diskpart(line,focus){
                Ok((focus,_code,output,exit))=>{self.disk_focus=Some(focus);self.lines.extend(output.replace('\r',"").lines().map(String::from));if exit{self.disk_focus=None;if self.diskpart_only{return AppAction::Close;}}},
                Err(status)=>self.lines.push(format!("DiskPart service failed (0x{status:08X}); no operation was completed.")),
            }
            self.trim_output();return AppAction::None;
        }
        if line.eq_ignore_ascii_case("diskpart")||line.eq_ignore_ascii_case("diskpart.exe"){self.enter_diskpart();return AppAction::None;}
        if line.eq_ignore_ascii_case("cls"){self.lines.clear();return AppAction::None;}
        if let Some((verb,args))=line.split_once(' '){if verb.eq_ignore_ascii_case("start"){
            if let Some(id)=catalog::resolve(args.trim()){return AppAction::Launch(id);}
        }}
        match crate::platform::services::command(line,&self.cwd){
            Ok(result)=>{
                self.cwd=result.directory;
                self.lines.extend(String::from_utf8_lossy(&result.bytes).replace('\r',"").lines().map(String::from));
                if result.exit{return AppAction::Close;}
            },
            Err(status)=>self.lines.push(format!("Command service failed (0x{status:08X})")),
        }
        self.trim_output();AppAction::None
    }
    fn trim_output(&mut self){if self.lines.len()>1024{self.lines.drain(..self.lines.len()-1024);}}
}
#[cfg(test)]mod tests{use super::*;#[test]fn commands_modify_shared_store_and_resolve_applications(){let mut files=FileSystem::new();let mut c=CommandPrompt::new();for text in ["mkdir Test","cd Test"]{c.input=text.into();c.execute(&mut files);}assert_ne!(c.directory,1);files.touch(c.directory,"hello.txt",b"hello world");c.input="type hello.txt".into();c.execute(&mut files);assert_eq!(c.lines.last().unwrap(),"hello world");c.input="start calc".into();assert_eq!(c.execute(&mut files),AppAction::Launch(super::super::AppId::Calculator));}}

#[cfg(test)] mod editing_tests {
    use super::*;
    fn type_bytes(c: &mut CommandPrompt, fs: &mut FileSystem, bytes: &[u8]) {
        for &b in bytes { c.key(fs,b); }
    }
    #[test] fn ansi_navigation_edits_the_command_that_executes_and_restores_draft() {
        let mut fs = FileSystem::new(); let mut c = CommandPrompt::new();
        type_bytes(&mut c,&mut fs,b"echo ac\x1b[Db\r");
        assert_eq!(c.lines.last().unwrap(),"abc");
        type_bytes(&mut c,&mut fs,b"draft\x1b[A");
        assert_eq!(c.input,"echo abc");
        type_bytes(&mut c,&mut fs,b"\x1b[B"); assert_eq!(c.input,"draft");
        type_bytes(&mut c,&mut fs,b"\x1b[H\x1b[3~D\x1b[F!"); assert_eq!(c.input,"Draft!");
        c.key(&mut fs,3); assert!(c.input.is_empty()); assert_eq!(c.cursor,0);
        assert!(c.lines.last().unwrap().ends_with("Draft!^C"));
    }
    #[test] fn history_is_bounded_per_window_and_does_not_execute_navigation_bytes() {
        let mut fs = FileSystem::new(); let mut c = CommandPrompt::new();
        for i in 0..80 { c.input=format!("echo {i}"); c.execute(&mut fs); }
        assert_eq!(c.history.len(),64);
        for _ in 0..90 { type_bytes(&mut c,&mut fs,b"\x1b[A"); }
        assert_eq!(c.input,"echo 16");
        let other=CommandPrompt::new(); assert!(other.history.is_empty());
    }
}

#[cfg(test)] mod unicode_cursor_tests {
    use super::*;
    #[test] fn externally_set_cursor_is_normalized_to_a_utf8_boundary() {
        let mut c=CommandPrompt::new();let mut fs=FileSystem::new();
        c.input="echo 中文".into();c.cursor=6;
        assert_eq!(c.display_input(),"echo |中文");
        c.key(&mut fs,b'x');assert_eq!(c.input,"echo x中文");
        c.key(&mut fs,8);assert_eq!(c.input,"echo 中文");
    }
}
