//! Window/session integration, shared by Explorer and the native application runner.
use alloc::{format,string::String};
use crate::{graphics::{Rect,Surface},shell::{ShellModel,WindowKind}};
use super::{AppAction,AppId,AppState,ApplicationSession,Environment,Task,explorer};
impl ShellModel {
    pub fn application_environment(&self)->Environment{Environment{width:self.viewport.0,height:self.viewport.1,network_adapters:self.network_adapters,audio:self.audio_available,processes:self.process_snapshot.clone(),tasks:self.taskbar.windows.iter().map(|w|Task{id:w.id,title:w.title.clone(),minimized:w.minimized}).collect()}}
    pub fn application(&self,id:u32)->Option<&ApplicationSession>{self.applications.iter().find(|a|a.window_id==id)}
    pub fn launch_application(&mut self,id:AppId)->u32{
        if id==AppId::Explorer {return self.open_window_with_kind("Windows Explorer",60,60,640,450,WindowKind::Explorer);}
        let(width,height)=match id{AppId::Calculator=>(350,430),AppId::StickyNotes=>(430,350),AppId::Run=>(540,240),AppId::Minesweeper=>(510,420),AppId::OnScreenKeyboard=>(520,340),_=>(640,460)};
        let offset=(self.taskbar.windows.len()%5)as i32*22;
        let window=self.open_window_with_kind(id.application().title,60+offset,60+offset,width,height,if id==AppId::CommandPrompt {WindowKind::CommandPrompt}else{WindowKind::Application(id)});
        self.applications.push(ApplicationSession::new(window,id,window as u64*7919));window
    }
    pub fn application_body(&self,id:u32)->Option<Rect>{self.taskbar.windows.iter().find(|w|w.id==id).map(|w|Rect::new(w.x+6,w.y+30,w.width.saturating_sub(12)as u32,w.height.saturating_sub(36)as u32))}
    pub fn application_key(&mut self,id:u32,key:u8)->bool{
        if let Some(explorer)=self.explorers.iter_mut().find(|e|e.window_id==id){let changed=explorer.stream_key(&mut self.files,key);let open=explorer.pending_open.take();if let Some(file)=open{self.open_application_file(file);}return changed;}
        let Some(index)=self.applications.iter().position(|a|a.window_id==id)else{return false;};
        if matches!(self.applications[index].state,AppState::CommandPrompt(_)) {
            let AppState::CommandPrompt(c)=&mut self.applications[index].state else{unreachable!()};let action=c.key(&mut self.files,key);self.apply_application_action(id,action);return true;
        }
        let now=self.applications[index].input_time;
        let Some(key)=self.applications[index].decoder.feed(key,now)else{return false;};
        self.application_decoded_key(id,key);true
    }
    fn application_decoded_key(&mut self,id:u32,key:u8){
        let Some(index)=self.applications.iter().position(|a|a.window_id==id)else{return;};
        if self.applications[index].confirm{
            match key{b's'|b'S'|13=>{self.applications[index].confirm=false;let action=self.applications[index].save();self.apply_application_action(id,action);},b'n'|b'N'=>{self.applications[index].confirm=false;self.finish_pending(id);},27=>{self.applications[index].confirm=false;self.applications[index].pending=None;},_=>{}}return;
        }
        if let Some(mut dialog)=self.applications[index].dialog.take(){
            if let Some(body)=self.application_body(id){dialog.visible_rows=(super::file_dialog::list_bounds(body).height/24).max(1) as usize;}
            let result=dialog.key(&mut self.files,key);self.finish_file_dialog(id,dialog,result);return;
        }
        let action=self.applications[index].key(key);self.apply_application_action(id,action);
    }
    /// Complete a solitary Escape only after allowing ANSI navigation bytes to arrive.
    pub fn flush_application_input(&mut self,now:u64)->bool{
        let mut expired=alloc::vec::Vec::new();
        for app in &mut self.applications{app.input_time=now;if let Some(key)=app.decoder.expire(now){expired.push((app.window_id,key));}}
        let changed=!expired.is_empty();for(id,key)in expired{self.application_decoded_key(id,key);}changed
    }
    pub fn request_application_close(&mut self,id:u32){self.request_pending(id,super::Pending::Close);}
    fn request_pending(&mut self,id:u32,pending:super::Pending){
        let Some(app)=self.applications.iter_mut().find(|a|a.window_id==id)else{if pending==super::Pending::Close{self.close_window(id);}return;};
        if app.dialog.is_some()||app.confirm{return;}
        app.menu=false;app.search=None;app.pending=Some(pending);
        if app.dirty(){app.confirm=true;}else{self.finish_pending(id);}
    }
    fn finish_pending(&mut self,id:u32){
        let Some(index)=self.applications.iter().position(|a|a.window_id==id)else{return;};
        let pending=self.applications[index].pending.take();
        match pending{
            Some(super::Pending::Close)=>self.close_window(id),
            Some(super::Pending::Open)=>self.begin_file_dialog(id,super::file_dialog::Mode::Open),
            Some(super::Pending::New)=>{let app=&mut self.applications[index];app.state=ApplicationSession::new(id,app.id,0).state;app.status.clear();self.update_application_title(id);},None=>{}
        }
    }
    fn begin_file_dialog(&mut self,id:u32,mode:super::file_dialog::Mode){
        let Some(index)=self.applications.iter().position(|a|a.window_id==id)else{return;};
        let app=&self.applications[index];let current=app.current_file();let name=if mode==super::file_dialog::Mode::Open{String::new()}else{app.file_name().into()};
        let filter=match app.state{AppState::Paint(_)|AppState::Snipping{..}=>super::file_dialog::Filter::Bitmap,_ if app.id==AppId::WordPad=>super::file_dialog::Filter::RichText,_=>super::file_dialog::Filter::Text};
        let dialog=super::file_dialog::FileDialog::new(&mut self.files,mode,current,&name,filter);
        let app=&mut self.applications[index];app.dialog=Some(dialog);app.menu=false;app.hot=None;
    }
    fn finish_file_dialog(&mut self,id:u32,mut dialog:super::file_dialog::FileDialog,result:super::file_dialog::Result){
        use super::file_dialog::Result as R;
        let outcome:Result<(),String>=match result{
            R::None=>{if let Some(app)=self.applications.iter_mut().find(|a|a.window_id==id){app.dialog=Some(dialog);}return;},
            R::Cancel=>{if let Some(app)=self.applications.iter_mut().find(|a|a.window_id==id){app.pending=None;}return;},
            R::Open(file)=>self.load_into_application(id,file),
            R::Save{parent,name,existing}=>{
                let action=self.applications.iter_mut().find(|a|a.window_id==id).map(|a|a.save());
                if let Some(AppAction::Save(_,bytes))=action{
                    let result=if let Some(file)=existing{self.files.save(file,&bytes).map(|()|file)}else{let file=self.files.touch(parent,&name,&bytes);if file==0{Err(self.files.last_error.unwrap_or(nt61_abi::desktop_bridge::STATUS_NOT_FOUND))}else{Ok(file)}};
                    match result{Ok(file)=>{if let Some(app)=self.applications.iter_mut().find(|a|a.window_id==id){app.saved(file,name);app.status=format!("Saved {}",self.files.path(file).unwrap_or_default());}self.update_application_title(id);self.finish_pending(id);Ok(())},Err(status)=>Err(format!("Save failed (0x{status:08X})"))}
                }else{Err("This application has no document to save.".into())}
            }
        };
        if let Err(message)=outcome{dialog.status=message;dialog.overwrite=None;if let Some(app)=self.applications.iter_mut().find(|a|a.window_id==id){app.dialog=Some(dialog);}}
    }
    fn load_into_application(&mut self,id:u32,file:u32)->Result<(),String>{
        self.files.load_file(file).map_err(|s|format!("Open failed (0x{s:08X})"))?;
        let node=self.files.node(file).ok_or_else(||String::from("File not found."))?;
        let name=node.name.clone();let app=self.applications.iter_mut().find(|a|a.window_id==id).ok_or_else(||String::from("Window closed."))?;
        match &mut app.state{AppState::Document(d)=>d.load(&node.contents,app.id==AppId::WordPad).map_err(String::from)?,AppState::Paint(p)=>p.load_bmp(&node.contents).map_err(String::from)?,_=>return Err("Unsupported document type.".into())}
        app.saved(file,name);app.status.clear();app.search=None;self.update_application_title(id);Ok(())
    }
    fn update_application_title(&mut self,id:u32){
        let Some(app)=self.application(id)else{return;};let title=format!("{} - {}",app.file_name(),app.id.application().title);
        if let Some(w)=self.taskbar.windows.iter_mut().find(|w|w.id==id){w.title=title;}
    }
    /// Clear hover when a popup or another window takes pointer ownership.
    pub fn clear_application_hover(&mut self, id:u32)->bool {
        if let Some(explorer)=self.explorers.iter_mut().find(|e|e.window_id==id) {
            return explorer.hot.take().is_some();
        }
        self.applications.iter_mut().find(|a|a.window_id==id)
            .is_some_and(|app|app.hot.take().is_some())
    }
    pub fn application_pointer(&mut self,id:u32,x:i32,y:i32,down:bool,held:bool,right:bool)->bool{
        let Some(body)=self.application_body(id)else{return false;};
        if let Some(index)=self.explorers.iter().position(|e|e.window_id==id){
            let (changed,action)=self.explorers[index].pointer(&mut self.files,body,x,y,down,held,right);
            if let explorer::Action::Open(file_id)=action{self.open_application_file(file_id);}return changed;
        }
        if !body.contains(x,y){if !held{if let Some(app)=self.applications.iter_mut().find(|a|a.window_id==id){match &mut app.state{AppState::Paint(p)=>p.end(),AppState::Document(d)=>d.drag_anchor=None,_=>{}}}}return self.clear_application_hover(id);}
        let env=self.application_environment();let Some(app)=self.applications.iter_mut().find(|a|a.window_id==id)else{return false;};
        if app.confirm{if down{let r=super::view::confirm_buttons(body);let key=if r[0].contains(x,y){Some(b's')}else if r[1].contains(x,y){Some(b'n')}else if r[2].contains(x,y){Some(27)}else{None};if let Some(key)=key{self.application_decoded_key(id,key);}}return down;}
        if let Some(mut dialog)=app.dialog.take(){let result=if down{dialog.click(&mut self.files,body,x,y)}else{super::file_dialog::Result::None};self.finish_file_dialog(id,dialog,result);return down;}
        if let Some(search)=&mut app.search{if down{if let AppState::Document(d)=&mut app.state{if super::editor_view::search_click(search,d,body,x,y){app.search=None;}}}return down;}
        let hot=super::view::buttons(app,body,&env).into_iter().rev().find(|b|b.bounds.contains(x,y)).map(|b|b.bounds);
        let hover_changed=app.hot!=hot;app.hot=hot;
        if let AppState::Paint(p)=&mut app.state{let canvas=super::view::paint_bounds(body);if !app.menu&&canvas.contains(x,y)&&(held||right){let px=((x-canvas.x)as u32*100/p.zoom+p.scroll.0)as i32;let py=((y-canvas.y)as u32*100/p.zoom+p.scroll.1)as i32;if down{p.begin_button(px,py,right);}else{p.stroke(px,py);}return true;}if !held{p.end();}}
        let mut editor_changed=false;
        if !app.menu{if let AppState::Document(d)=&mut app.state{editor_changed=super::editor_view::pointer(d,body,x,y,down,held);}}
        if down{let action=super::view::click(app,body,&env,x,y,right);self.apply_application_action(id,action);}down||hover_changed||editor_changed
    }
    pub fn apply_application_action(&mut self,id:u32,action:AppAction){match action{
        AppAction::None|AppAction::Capture=>{},
        AppAction::Close=>self.close_window(id),
        AppAction::RequestClose=>self.request_application_close(id),
        AppAction::Open=>self.request_pending(id,super::Pending::Open),
        AppAction::NewDocument=>self.request_pending(id,super::Pending::New),
        AppAction::SaveAs=>self.begin_file_dialog(id,super::file_dialog::Mode::SaveAs),
        AppAction::Launch(app)=>{self.launch_application(app);},
        AppAction::Save(name,bytes)=>{
            if self.application(id).is_some_and(|a|a.current_file().is_none()){self.begin_file_dialog(id,super::file_dialog::Mode::SaveAs);return;}
            let result=self.save_application_file(id,&name,&bytes);
            if let Some(app)=self.applications.iter_mut().find(|a|a.window_id==id){
                match &result {
                    Ok(path)=>{app.status=format!("Saved {path}");let file=app.current_file();if let Some(file)=file{app.saved(file,name.clone());}},
                    Err(status)=>app.status=format!("Save failed (0x{status:08X})"),
                }
            }
            if result.is_ok(){self.finish_pending(id);}
        },
        AppAction::Copy(text)=>self.files.set_clipboard_text(&text),
        AppAction::Paste=>{let text=String::from(self.files.clipboard_text());if let Some(app)=self.applications.iter_mut().find(|a|a.window_id==id){app.paste(&text);}},
        AppAction::Activate(target)=>{if let Some(w)=self.taskbar.windows.iter_mut().find(|w|w.id==target){w.minimized=false;}self.select_window(target);},
        AppAction::EndTask(target)=>self.close_window(target),
        AppAction::Type(key)=>{if let Some(target)=self.text_target.filter(|target|*target!=id){if self.applications.iter().any(|a|a.window_id==target){self.application_key(target,key);}else{let old=self.selected_window;self.selected_window=Some(target);self.handle_window_key(key);self.selected_window=old;}}},
    }}
    fn save_application_file(&mut self,id:u32,_name:&str,bytes:&[u8])->Result<String,u32>{
        let file=self.application(id).and_then(|a|a.current_file()).ok_or(nt61_abi::desktop_bridge::STATUS_NOT_FOUND)?;
        self.files.save(file,bytes)?;self.files.path(file).ok_or(nt61_abi::desktop_bridge::STATUS_NOT_FOUND)
    }
    pub fn open_application_file(&mut self, file_id: u32) -> Option<u32> {
        if let Err(status) = self.files.load_file(file_id) {
            self.report_file_open(file_id, &format!("Open failed (0x{status:08X})"));
            return None;
        }
        let node = self.files.node(file_id)?;
        // Open only a registered executable through its canonical path. Never
        // display a PE/DLL as replacement characters in an editor.
        if node.name.to_ascii_lowercase().ends_with(".exe") {
            let path = self.files.path(file_id)?;
            if let Some(id) = super::catalog::APPLICATIONS.iter().find(|app| {
                app.install_path().is_some_and(|p| path.get(3..).is_some_and(|relative|relative.replace('\\',"/").eq_ignore_ascii_case(p)))
            }).map(|app| app.id) { return Some(self.launch_application(id)); }
            self.report_file_open(file_id,"This executable is not supported by the application host.");
            return None;
        }
        let app_id = match super::associations::resolve(&node.name) {
            Some(app) => app,
            None if super::document::decode_text(&node.contents).is_ok() => AppId::Notepad,
            None => {self.report_file_open(file_id,"No application is associated with this file type.");return None;}
        };
        let name = node.name.clone();
        let mut session = ApplicationSession::new(0, app_id, 0);
        match &mut session.state {
            AppState::Document(document) => {
                if let Err(message) = document.load(&node.contents, app_id == AppId::WordPad) {
                    self.report_file_open(file_id,message);return None;
                }
                document.file_id = Some(file_id); document.filename = name;
            }
            AppState::Paint(p) => match p.load_bmp(&node.contents) {
                Ok(()) => {
                    p.file_id=Some(file_id);p.filename=name.clone();
                }
                Err(_) => {self.report_file_open(file_id,"Invalid or unsupported bitmap.");return None;}
            },
            _ => {}
        }
        let window = self.launch_application(app_id);
        if let Some(app) = self.applications.iter_mut().find(|a|a.window_id==window) {app.state=session.state;}
        self.update_application_title(window);
        Some(window)
    }
    fn report_file_open(&mut self, file_id:u32, message:&str) {
        for explorer in &mut self.explorers {
            if explorer.selected == Some(file_id) {explorer.status=message.into();}
        }
    }
    /// Capture only the composed desktop beneath the application, supplied by the host renderer.
    pub fn capture_application(&mut self,id:u32,image:Surface){if let Some(app)=self.applications.iter_mut().find(|a|a.window_id==id){app.capture(image);}}
    pub fn needs_capture(&self,id:u32,x:i32,y:i32)->bool{let Some(app)=self.application(id)else{return false;};let Some(body)=self.application_body(id)else{return false;};super::view::buttons(app,body,&self.application_environment()).iter().any(|b|b.bounds.contains(x,y)&&b.command==super::view::Command::Capture)}
}
#[cfg(test)]mod tests{use super::*;use crate::theme::Theme;#[test]fn run_launch_save_and_task_manager_end_task_are_real_actions(){let mut shell=ShellModel::new(Theme::aero());let calc=shell.launch_application(AppId::Calculator);let task=shell.launch_application(AppId::TaskManager);shell.apply_application_action(task,AppAction::EndTask(calc));assert!(shell.application(calc).is_none());let editor=shell.launch_application(AppId::Notepad);for key in b"hello"{shell.application_key(editor,*key);}shell.application_key(editor,19);assert!(shell.application(editor).unwrap().dialog.is_some());for key in b"hello.txt\r"{shell.application_key(editor,*key);}let file=shell.files.resolve("C:\\hello.txt").unwrap();assert_eq!(shell.files.node(file).unwrap().contents,b"hello");let run=shell.launch_application(AppId::Run);for key in b"calc.exe\r"{shell.application_key(run,*key);}assert!(shell.applications.iter().any(|app|app.id==AppId::Calculator));}}

#[cfg(test)] mod file_open_tests {
    use super::*;
    use crate::theme::Theme;
    #[test] fn explorer_reopens_rtf_in_wordpad_and_saves_to_the_same_vfs_node() {
        let mut shell=ShellModel::new(Theme::aero());
        let mut original=super::super::document::Document::new("Document.rtf");original.insert("Hello 中😀");original.bold=true;
        let file=shell.files.touch(1,"Document.RTF",&original.rtf());
        let id=shell.open_application_file(file).unwrap();
        let app=shell.application(id).unwrap();assert_eq!(app.id,AppId::WordPad);
        let AppState::Document(doc)=&app.state else {panic!("document");};
        assert_eq!(doc.editor.text,"Hello 中😀");assert!(doc.bold);assert_eq!(doc.file_id,Some(file));
        shell.application_key(id,b'!');shell.application_key(id,19);
        let mut saved=super::super::document::Document::new("Document.rtf");
        saved.load(&shell.files.node(file).unwrap().contents,true).unwrap();
        assert_eq!(saved.editor.text,"Hello 中😀!");
        assert!(!shell.files.children(1).iter().any(|node|node.name=="Documents"),
            "saving an open document must not depend on or create a default Documents folder");
    }
    #[test] fn unsupported_binary_and_bad_bitmap_do_not_launch_empty_editors() {
        let mut shell=ShellModel::new(Theme::aero());
        for (name,bytes) in [("ntdll.dll",b"MZ\0binary".as_slice()),("wrong.bmp",b"invalid")] {
            let file=shell.files.touch(1,name,bytes);assert_eq!(shell.open_application_file(file),None);
        }
        assert!(shell.applications.is_empty());
    }
    #[test] fn leaving_a_window_clears_button_and_explorer_hover() {
        let mut shell=ShellModel::new(Theme::aero());
        let calc=shell.launch_application(AppId::Calculator);
        shell.applications.iter_mut().find(|app|app.window_id==calc).unwrap().hot=Some(Rect::new(0,0,10,10));
        assert!(shell.application_pointer(calc,-1,-1,false,false,false));
        assert!(shell.application(calc).unwrap().hot.is_none());
        assert!(!shell.clear_application_hover(calc));
        let explorer=shell.launch_application(AppId::Explorer);
        shell.explorers.iter_mut().find(|e|e.window_id==explorer).unwrap().hot=Some(explorer::Hit::Back);
        assert!(shell.clear_application_hover(explorer));
        assert!(shell.explorers.iter().find(|e|e.window_id==explorer).unwrap().hot.is_none());
    }
}

#[cfg(test)] mod accessibility_tests {
    use super::*;
    use crate::theme::Theme;
    #[test]
    fn control_panel_accessibility_tools_launch_real_sessions() {
        let mut shell=ShellModel::new(Theme::aero());
        let center=shell.launch_application(AppId::EaseOfAccessCenter);
        let body=shell.application_body(center).unwrap();
        let env=shell.application_environment();
        let buttons=super::super::view::buttons(shell.application(center).unwrap(),body,&env);
        for target in [AppId::Magnifier,AppId::OnScreenKeyboard] {
            let bounds=buttons.iter().find(|b|b.command==super::super::view::Command::Launch(target)).unwrap().bounds;
            assert!(shell.application_pointer(center,bounds.x+4,bounds.y+4,true,true,false));
            assert!(shell.applications.iter().any(|app|app.id==target));
        }
    }
}

#[cfg(test)] mod document_workflow_tests {
    use super::*;
    use crate::theme::Theme;
    fn type_keys(s:&mut ShellModel,id:u32,text:&[u8]){for key in text{s.application_key(id,*key);}}
    fn escape(s:&mut ShellModel,id:u32){s.application_key(id,27);s.flush_application_input(200);}
    #[test]fn save_as_cancel_and_overwrite_preserve_the_right_file(){
        let mut s=ShellModel::new(Theme::aero());
        let source=s.files.touch(1,"source.txt",b"original");let target=s.files.touch(1,"target.txt",b"keep");
        let id=s.open_application_file(source).unwrap();type_keys(&mut s,id,b" edited");
        s.apply_application_action(id,AppAction::SaveAs);type_keys(&mut s,id,b"target.txt\r");
        assert_eq!(s.application(id).unwrap().dialog.as_ref().unwrap().overwrite,Some(target));
        assert_eq!(s.files.node(target).unwrap().contents,b"keep");
        type_keys(&mut s,id,b"n");escape(&mut s,id);
        assert!(s.application(id).unwrap().dialog.is_none());assert!(s.application(id).unwrap().dirty());
        assert_eq!(s.application(id).unwrap().current_file(),Some(source));
        s.apply_application_action(id,AppAction::SaveAs);type_keys(&mut s,id,b"target.txt\ry");
        assert_eq!(s.files.node(source).unwrap().contents,b"original");
        assert_eq!(s.files.node(target).unwrap().contents,b"original edited");
        assert_eq!(s.application(id).unwrap().current_file(),Some(target));assert!(!s.application(id).unwrap().dirty());
        type_keys(&mut s,id,b"!\x13");assert_eq!(s.files.node(target).unwrap().contents,b"original edited!");
    }
    #[test]fn dirty_close_save_discard_and_cancel_are_distinct(){
        let mut s=ShellModel::new(Theme::aero());let id=s.launch_application(AppId::Notepad);
        type_keys(&mut s,id,b"unsaved");s.request_application_close(id);assert!(s.application(id).unwrap().confirm);
        escape(&mut s,id);assert!(!s.application(id).unwrap().confirm);assert!(s.application(id).unwrap().pending.is_none());
        s.request_application_close(id);type_keys(&mut s,id,b"s");assert!(s.application(id).unwrap().dialog.is_some());
        type_keys(&mut s,id,b"closed.txt\r");assert!(s.application(id).is_none());
        let closed=s.files.resolve("C:\\closed.txt").unwrap();assert_eq!(s.files.node(closed).unwrap().contents,b"unsaved");
        let id=s.launch_application(AppId::Notepad);type_keys(&mut s,id,b"discard");s.request_application_close(id);type_keys(&mut s,id,b"n");assert!(s.application(id).is_none());
    }
    #[test]fn new_and_open_share_dirty_prompt_and_failed_open_is_atomic(){
        let mut s=ShellModel::new(Theme::aero());let bad=s.files.touch(1,"bad.txt",b"binary\0");let good=s.files.touch(1,"good.txt",b"loaded\r\ntext");
        let id=s.launch_application(AppId::Notepad);type_keys(&mut s,id,b"keep\x0f");assert!(s.application(id).unwrap().confirm);
        type_keys(&mut s,id,b"n");type_keys(&mut s,id,b"bad.txt\r");assert!(s.application(id).unwrap().dialog.is_some());
        let AppState::Document(d)=&s.application(id).unwrap().state else{panic!()};assert_eq!(d.editor.text,"keep");assert!(d.dirty);
        assert!(s.load_into_application(id,bad).is_err());assert!(s.load_into_application(id,good).is_ok());
        // Close the still-open failed picker, then create a new clean document.
        escape(&mut s,id);type_keys(&mut s,id,b"\x0e");
        let AppState::Document(d)=&s.application(id).unwrap().state else{panic!()};assert_eq!(d.editor.text,"");assert!(d.file_id.is_none());assert!(!d.dirty);
    }
    #[test]fn paint_open_keeps_full_canvas_and_save_as_round_trips(){
        let mut s=ShellModel::new(Theme::aero());let mut p=super::super::paint::Paint::new(700,500);p.color=0xffed1c24;p.begin(680,480);p.end();
        let file=s.files.touch(1,"large.bmp",&super::super::paint::encode_bmp(&p.canvas));let id=s.open_application_file(file).unwrap();
        let AppState::Paint(p)=&s.application(id).unwrap().state else{panic!()};assert_eq!(p.canvas.width(),700);assert_eq!(p.canvas.pixel(680,480),Some(0xffed1c24));
        s.apply_application_action(id,AppAction::SaveAs);type_keys(&mut s,id,b"copy\r");let copy=s.files.resolve("C:\\copy.bmp").unwrap();
        let image=crate::bmp::decode_bmp(&s.files.node(copy).unwrap().contents).unwrap();assert_eq!((image.width,image.height),(700,500));
    }
}
