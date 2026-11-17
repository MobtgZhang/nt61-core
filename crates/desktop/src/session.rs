//! Desktop interaction state; authentication and power changes are kernel operations.
use alloc::{string::String, vec::Vec};
use crate::{graphics::{Rect, Surface}, compositor::aero, login::PasswordEntry};
use crate::shell::PowerAction;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen { Desktop, Security, ShutDown, Users, Credentials, ChangePassword, Progress }
pub struct Session {
    pub screen: Screen,
    pub accounts: Vec<crate::accounts::Account>,
    pub artwork: Option<alloc::rc::Rc<crate::accounts::Artwork>>,
    pub named_user: bool,
    pub user: String,
    pub identity: String,
    pub locked: bool,
    pub pending_close: Option<PowerAction>,
    pub closing: Option<u32>,
    pub selected: usize,
    pub expanded: bool,
    pub switching: bool,
    pub logged_off: bool,
    pub password: PasswordEntry,
    pub old_password: PasswordEntry,
    pub new_password: PasswordEntry,
    pub field: usize,
    pub message: String,
    pub progress: Option<PowerAction>,
    pub power_menu: bool,
    pub power_popup: crate::power_menu::PowerMenu,
}
impl Clone for Session {
    fn clone(&self) -> Self { Self { screen:self.screen, accounts:self.accounts.clone(), artwork:self.artwork.clone(), named_user:self.named_user, user:self.user.clone(), identity:self.identity.clone(), locked:self.locked, pending_close:self.pending_close, closing:self.closing, selected:self.selected, expanded:self.expanded, switching:self.switching, logged_off:self.logged_off, field:self.field, message:self.message.clone(), progress:self.progress, power_menu:self.power_menu, power_popup:self.power_popup.clone(), ..Self::default() } }
}
impl core::fmt::Debug for Session {
    fn fmt(&self, f:&mut core::fmt::Formatter<'_>)->core::fmt::Result {f.debug_struct("Session").field("screen",&self.screen).field("user",&self.user).finish_non_exhaustive()}
}
impl PartialEq for Session {
    fn eq(&self, other:&Self)->bool {self.identity==other.identity&&self.locked==other.locked&&self.pending_close==other.pending_close&&self.closing==other.closing&&self.power_popup==other.power_popup&&self.screen==other.screen && self.user==other.user && self.selected==other.selected && self.expanded==other.expanded && self.message==other.message && self.progress==other.progress && self.field==other.field && self.switching==other.switching && self.logged_off==other.logged_off && self.power_menu==other.power_menu}
}
impl Default for Session {
    fn default() -> Self { Self { screen: Screen::Desktop, accounts:Vec::new(), artwork:None, named_user:false, user: "Administrator".into(), identity:"Administrator".into(), locked:false, pending_close:None, closing:None, selected: 4, expanded: false, switching: false, logged_off: false, password: PasswordEntry::new(), old_password: PasswordEntry::new(), new_password: PasswordEntry::new(), field: 0, message: String::new(), progress: None, power_menu: false, power_popup:crate::power_menu::PowerMenu::default() } }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Intent { None, Cancel, Action(PowerAction), Authenticate, ChangePassword, TaskManager }
pub const ACTIONS: [PowerAction; 5] = [PowerAction::SwitchUser, PowerAction::LogOff, PowerAction::Restart, PowerAction::Sleep, PowerAction::ShutDown];
pub fn title(action: PowerAction) -> &'static str { match action { PowerAction::SwitchUser=>"Switch user", PowerAction::Lock=>"Lock", PowerAction::LogOff=>"Log off", PowerAction::Restart=>"Restart", PowerAction::Sleep=>"Sleep", PowerAction::ShutDown=>"Shut down" } }
impl Session {
    pub fn show(&mut self, screen: Screen) { self.clear_passwords(); self.screen=screen; self.expanded=false; self.power_menu=false; self.power_popup.open=false; self.message.clear(); self.field=0; if screen==Screen::Security{self.selected=0;} }
    pub fn select_users(&mut self) {
        self.show(Screen::Users);self.selected=0;
        match crate::platform::services::accounts() {
            Ok(accounts)=>self.accounts=accounts.into_iter().filter(|a|a.enabled).collect(),
            Err(code)=>{self.accounts.clear();self.message=alloc::format!("Unable to load user accounts (0x{code:08X}).");}
        }
        if self.artwork.is_none(){self.artwork=crate::accounts::Artwork::load();}
    }
    fn select_account(&mut self,index:usize) {
        if let Some(account)=self.accounts.get(index){self.user=account.name.clone();self.named_user=true;self.show(Screen::Credentials);}
    }
    pub fn clear_passwords(&mut self) { self.password.clear(); self.old_password.clear(); self.new_password.clear(); }
    pub fn key(&mut self, key: u8) -> Intent {
        if self.power_popup.open {return self.power_popup.key(match key{0x26=>0x81,0x28=>0x82,k=>k}).map_or(Intent::None,Intent::Action);}
        match self.screen {
            Screen::Desktop | Screen::Progress => Intent::None,
            Screen::Users=>{
                match key {
                    9|0x28|0x27=>{self.selected=(self.selected+1)%self.accounts.len().max(1);},
                    0x26|0x25=>{self.selected=(self.selected+self.accounts.len().max(1)-1)%self.accounts.len().max(1);},
                    13|10=>self.select_account(self.selected),
                    27 if !self.logged_off=>return Intent::Cancel,
                    _=>{}
                }Intent::None
            },
            Screen::Security => match key {
                27=>Intent::Cancel, b'l'|b'L'=>Intent::Action(PowerAction::Lock),b't'|b'T'=>Intent::TaskManager,b's'|b'S'=>Intent::Action(PowerAction::SwitchUser),
                9|0x28=>{self.selected=(self.selected+1)%6;Intent::None},0x26=>{self.selected=(self.selected+5)%6;Intent::None},
                13|10=>match self.selected{0=>Intent::Action(PowerAction::Lock),1=>Intent::Action(PowerAction::SwitchUser),2=>Intent::Action(PowerAction::LogOff),3=>{self.show(Screen::ChangePassword);Intent::None},4=>Intent::TaskManager,_=>Intent::Cancel},_=>Intent::None
            },
            Screen::ShutDown => match key {
                27 => if self.expanded {self.expanded=false; Intent::None} else {Intent::Cancel},
                13|10 => if self.expanded {self.expanded=false; Intent::None} else {Intent::Action(ACTIONS[self.selected])},
                0x26 | b'k' => {self.selected=self.selected.saturating_sub(1);Intent::None},
                0x28 | b'j' => {self.selected=(self.selected+1).min(ACTIONS.len()-1);Intent::None},
                b' '=>{self.expanded=!self.expanded;Intent::None}, _=>Intent::None,
            },
            Screen::Credentials | Screen::ChangePassword => {
                if key==27 && self.switching && self.named_user {self.select_users();return Intent::None;}
                if key==27 && !self.logged_off {return Intent::Cancel;}
                if key==9 {self.field=(self.field+1)%if self.screen==Screen::ChangePassword{3}else if self.switching&&!self.named_user{2}else{1};return Intent::None;}
                if matches!(key,13|10) {return if self.screen==Screen::ChangePassword{Intent::ChangePassword}else{Intent::Authenticate};}
                if self.screen==Screen::Credentials && self.switching && !self.named_user && self.field==0 {
                    if key==8 {self.user.pop();} else if (32..=126).contains(&key) && self.user.len()<64 { self.user.push(key as char); }
                } else {
                    let entry=if self.screen==Screen::ChangePassword {match self.field {0=>&mut self.old_password,1=>&mut self.new_password,_=>&mut self.password}} else {&mut self.password};
                    if key==8 {entry.backspace();} else if (32..=126).contains(&key) {entry.append(key);}
                }
                Intent::None
            }
        }
    }
    pub fn dialog(w:u32,h:u32)->Rect {Rect::new(w.saturating_sub(440) as i32/2,h.saturating_sub(294) as i32/2,440.min(w),294.min(h))}
    pub fn hit(&mut self,x:i32,y:i32,w:u32,h:u32)->Intent {
        if matches!(self.screen,Screen::Security|Screen::Users|Screen::Credentials) {
            let (main,anchor)=crate::logon_power::LogonPowerButtons::bounds(w,h as i32-54);
            if self.power_popup.open {return self.power_popup.pointer(anchor,x,y,true).map_or(Intent::None,Intent::Action);}
            if anchor.contains(x,y){self.power_popup.toggle();return Intent::None;}
            if main.contains(x,y){return Intent::Action(PowerAction::ShutDown);}
        }
        if self.screen==Screen::Users {
            let range=crate::accounts::visible(w,h,self.accounts.len(),self.selected);
            for (local,i) in range.clone().enumerate(){if crate::accounts::tile(w,h,local,range.len()).contains(x,y){self.selected=i;self.select_account(i);return Intent::None;}}
            if range.start>0 && crate::accounts::page_button(w,h,false).contains(x,y){self.selected=range.start-1;}
            if range.end<self.accounts.len() && crate::accounts::page_button(w,h,true).contains(x,y){self.selected=range.end;}
            return Intent::None;
        }
        if self.screen==Screen::ShutDown {
            let b=Self::dialog(w,h);
            if self.expanded {
                for i in 0..5 {if Rect::new(b.x+36,b.y+155+i*25,360,25).contains(x,y){self.selected=i as usize;self.expanded=false;return Intent::None;}}
            }
            if Rect::new(b.x+36,b.y+128,360,27).contains(x,y){self.expanded=!self.expanded;return Intent::None;}
            for i in 0..3 {if Rect::new(b.x+165+i*86,b.y+250,78,25).contains(x,y){return match i {0=>Intent::Action(ACTIONS[self.selected]),1=>Intent::Cancel,_=>{self.message="Choose an action, then click OK.".into();Intent::None}};}}
        } else if self.screen==Screen::Security {
            let left=w as i32/2-140;let top=h as i32/2-150;
            for i in 0..6 {if Rect::new(left,top+i*49,280,40).contains(x,y){return match i {0=>Intent::Action(PowerAction::Lock),1=>Intent::Action(PowerAction::SwitchUser),2=>Intent::Action(PowerAction::LogOff),3=>{self.show(Screen::ChangePassword);Intent::None},4=>Intent::TaskManager,_=>Intent::Cancel};}}

        } else if matches!(self.screen,Screen::Credentials|Screen::ChangePassword) {
            let submit_y=h as i32/2-20+if self.screen==Screen::ChangePassword{76}else if self.switching&&!self.named_user{38}else{0};
            if Rect::new(w as i32/2+145,submit_y,40,32).contains(x,y){return if self.screen==Screen::ChangePassword{Intent::ChangePassword}else{Intent::Authenticate};}
            if Rect::new(w as i32/2-50,h as i32/2+145,100,28).contains(x,y)&&!self.logged_off{return Intent::Cancel;}
            let top=h as i32/2-20;
            for i in 0..3 {if Rect::new(w as i32/2-140,top+i*38,280,32).contains(x,y){self.field=i as usize;}}
        }
        Intent::None
    }
}
pub fn draw(session:&Session, frame:&mut Surface, text:impl Fn(&mut Surface,&str,Rect,u32)) { draw_with_background(session,frame,None,text); }
pub fn draw_with_background(session:&Session, frame:&mut Surface,background:Option<&Surface>, text:impl Fn(&mut Surface,&str,Rect,u32)) {
    let w=frame.width();let h=frame.height();
    if session.screen==Screen::Desktop{return;}
    if session.screen==Screen::ShutDown {
        frame.blend_rect(Rect::new(0,0,w,h),0x60000000);
        let b=Session::dialog(w,h);aero::shadow(frame,b,12);
        aero::blend_rounded_rect(frame,b,7,0xF0A4CCE8);
        frame.fill_rect(b.x+8,b.y+30,b.width-16,b.height-38,0xFFF0F0F0);
        text(frame,"Shut Down Windows",Rect::new(b.x+12,b.y+8,380,20),0xFF142536);
        text(frame,"Windows 7",Rect::new(b.x+36,b.y+49,360,34),0xFF153C65);
        text(frame,"What do you want the computer to do?",Rect::new(b.x+36,b.y+96,370,22),0xFF111111);
        frame.fill_rect(b.x+36,b.y+128,360,27,0xFF7F9DB9);frame.fill_rect(b.x+37,b.y+129,358,25,0xFFFFFFFF);
        text(frame,title(ACTIONS[session.selected]),Rect::new(b.x+42,b.y+134,324,20),0xFF111111);
        text(frame,"v",Rect::new(b.x+376,b.y+134,16,20),0xFF222222);
        let desc=match ACTIONS[session.selected]{PowerAction::ShutDown=>"Closes all programs and turns off the computer.",PowerAction::Restart=>"Closes all programs and restarts the computer.",PowerAction::LogOff=>"Closes your programs and logs you off.",PowerAction::SwitchUser=>"Keeps your programs open and switches users.",_=>"Puts the computer into a low-power state."};
        text(frame,desc,Rect::new(b.x+36,b.y+170,370,40),0xFF111111);
        text(frame,&session.message,Rect::new(b.x+36,b.y+211,370,32),0xFF9C2424);
        for (i,label) in ["OK","Cancel","Help"].iter().enumerate(){let r=Rect::new(b.x+165+i as i32*86,b.y+250,78,25);aero::blend_rounded_rect(frame,r,3,0xFF8796A3);aero::blend_rounded_rect(frame,Rect::new(r.x+1,r.y+1,r.width-2,r.height-2),2,0xFFF5F5F5);text(frame,label,Rect::new(r.x+12,r.y+5,65,20),0xFF111111);}
        if session.expanded {let r=Rect::new(b.x+36,b.y+155,360,125);aero::menu_surface(frame,r);for(i,a)in ACTIONS.iter().enumerate(){let r=Rect::new(r.x+1,r.y+i as i32*25,358,25);if i==session.selected{frame.fill_rect(r.x,r.y,r.width,r.height,0xFF3399FF);}text(frame,title(*a),Rect::new(r.x+5,r.y+4,345,20),if i==session.selected{0xFFFFFFFF}else{0xFF111111});}}
        return;
    }
    if session.screen==Screen::Desktop{return;}
    if let Some(bg)=background{frame.copy_from(bg);}else{frame.vertical_gradient(0xFF0B477F,0xFF072A50);}
    if background.is_none(){frame.blend_rounded_rect(Rect::new(-200,h as i32/2,w+100,h),h/2,0x153CB4E8);}
    text(frame,"Windows 7",Rect::new(w as i32/2-90,h as i32-65,200,35),0xFFFFFFFF);
    match session.screen {
        Screen::Security=>{
            let x=w as i32/2-140;let y=h as i32/2-150;
            for(i,label)in ["Lock this computer","Switch User","Log off","Change a password...","Start Task Manager","Cancel"].iter().enumerate(){let r=Rect::new(x,y+i as i32*49,280,40);if i==session.selected||i==5{aero::blend_rounded_rect(frame,r,4,if i==session.selected{0x8077AACA}else{0x5077AACA});}text(frame,label,Rect::new(r.x+25,r.y+10,255,24),0xFFFFFFFF);}

        },
        Screen::Users=>{
            let range=crate::accounts::visible(w,h,session.accounts.len(),session.selected);
            for (local,i) in range.clone().enumerate() {
                let account=&session.accounts[i];
                let r=crate::accounts::tile(w,h,local,range.len());
                if i==session.selected {aero::blend_rounded_rect(frame,r,6,0x3878C8FA);}
                if let Some(art)=&session.artwork {art.portrait(frame,Rect::new(r.x+25,r.y,100,100),account,true);}
                text(frame,&account.name,Rect::new(r.x+5,r.y+108,140,24),0xFFFFFFFF);
                if account.name.eq_ignore_ascii_case(&session.identity)&&!session.logged_off {text(frame,"Logged on",Rect::new(r.x+5,r.y+131,140,20),0xFFDFEFFF);}
            }
            text(frame,"Choose a user to log on",Rect::new(w as i32/2-160,55,320,30),0xFFFFFFFF);
            for next in [false,true] {if if next{range.end<session.accounts.len()}else{range.start>0}{let b=crate::accounts::page_button(w,h,next);aero::blend_rounded_rect(frame,b,4,0x5077AACA);text(frame,if next{"Next >"}else{"< Previous"},b,0xFFFFFFFF);}}
            text(frame,&session.message,Rect::new(w as i32/2-240,h as i32-130,480,40),0xFFFFFFFF);
        },
        Screen::Credentials|Screen::ChangePassword=>{
            let x=w as i32/2-140;let y=h as i32/2;
            if session.screen==Screen::Credentials {
                if let Some(art)=&session.artwork {if let Some(account)=session.accounts.iter().find(|a|a.name.eq_ignore_ascii_case(&session.user)){art.portrait(frame,Rect::new(w as i32/2-50,y-192,100,100),account,true);}}
                text(frame,&session.user,Rect::new(x,y-82,280,28),0xFFFFFFFF);
                if !session.switching&&!session.logged_off{text(frame,"Locked",Rect::new(x,y-52,280,24),0xFFDFEFFF);}
            }else{text(frame,"Change a password",Rect::new(x,y-105,280,32),0xFFFFFFFF);text(frame,&session.user,Rect::new(x,y-63,280,28),0xFFFFFFFF);}
            let labels:Vec<String>=if session.screen==Screen::ChangePassword{alloc::vec![alloc::format!("Old password: {}","*".repeat(session.old_password.len())),alloc::format!("New password: {}","*".repeat(session.new_password.len())),alloc::format!("Confirm: {}","*".repeat(session.password.len()))]}else if session.switching&&!session.named_user {alloc::vec![session.user.clone(),"*".repeat(session.password.len())]}else{alloc::vec!["*".repeat(session.password.len())]};
            for(i,label)in labels.iter().enumerate(){let r=Rect::new(x,y-20+i as i32*38,280,32);frame.fill_rect(r.x,r.y,r.width,r.height,if session.field==i{0xFF70C4F5}else{0xFF7592A9});frame.fill_rect(r.x+2,r.y+2,r.width-4,r.height-4,0xFFFFFFFF);text(frame,label,Rect::new(r.x+6,r.y+7,265,24),0xFF111111);}
            let r=Rect::new(x+285,y-20+(labels.len() as i32-1)*38,40,32);aero::blend_rounded_rect(frame,r,4,0xFF70AED2);text(frame,"->",Rect::new(r.x+9,r.y+8,30,20),0xFFFFFFFF);
            text(frame,&session.message,Rect::new(x-80,y+105,460,35),0xFFFFFFFF);
            if !session.logged_off {text(frame,"Cancel",Rect::new(w as i32/2-30,y+150,100,28),0xFFFFFFFF);}
        },
        Screen::Progress=>text(frame,match session.progress {Some(PowerAction::Restart)=>"Restarting...",Some(PowerAction::LogOff)=>"Logging off...",Some(PowerAction::Sleep)=>"Sleeping...",_=>"Shutting down..."},Rect::new(w as i32/2-100,h as i32/2-12,300,40),0xFFFFFFFF),
        _=>{}
    }
    if matches!(session.screen,Screen::Security|Screen::Users|Screen::Credentials){
        let (main,arrow)=crate::logon_power::LogonPowerButtons::bounds(w,h as i32-54);
        if let Some(art)=&session.artwork {art.power.draw(frame,main,arrow,crate::logon_power::ButtonState::Normal,if session.power_popup.open{crate::logon_power::ButtonState::Pressed}else{crate::logon_power::ButtonState::Normal});}
        session.power_popup.draw(frame,arrow,&text);
    }
}
#[cfg(test)] mod tests {
    use super::*;
    #[test]
    fn shutdown_checks_disconnected_users_before_closing_current_documents() {
        use crate::{applications::AppId, shell::ShellModel, theme::Theme};
        for action in [PowerAction::ShutDown, PowerAction::Restart] {
            let mut shell=ShellModel::new(Theme::aero());
            let document=shell.launch_application(AppId::Notepad);
            shell.saved_desktops.push(SavedDesktop {
                user:"Other".into(), applications:Vec::new(), explorers:Vec::new(),
                windows:Vec::new(), textboxes:Vec::new(), selected:None,
            });
            shell.power_action=Some(action);
            assert_eq!(shell.advance_session(),None);
            assert!(shell.application(document).is_some());
            assert!(shell.session.pending_close.is_none());
            assert_eq!(shell.session.screen,Screen::ShutDown);
        }
    }
    #[test] fn shutdown_cancel_and_selection_do_not_commit_early(){let mut s=Session::default();s.show(Screen::ShutDown);assert_eq!(s.key(27),Intent::Cancel);s.expanded=true;assert_eq!(s.key(13),Intent::None);assert!(!s.expanded);assert_eq!(s.key(13),Intent::Action(PowerAction::ShutDown));}
    #[test] fn lock_does_not_escape_to_logged_off_desktop(){let mut s=Session::default();s.show(Screen::Credentials);s.logged_off=true;assert_eq!(s.key(27),Intent::None);s.password.append(b'x');s.show(Screen::Security);assert!(s.password.is_empty());}
    #[test] fn popup_rows_use_same_geometry_as_render(){let mut s=Session::default();s.show(Screen::ShutDown);s.expanded=true;let b=Session::dialog(1024,768);assert_eq!(s.hit(b.x+50,b.y+160,1024,768),Intent::None);assert_eq!(s.selected,0);assert_eq!(s.key(13),Intent::Action(PowerAction::SwitchUser));}
}

/// A disconnected desktop keeps its application state, including dirty documents.
#[derive(Clone,Debug,PartialEq)]
pub struct SavedDesktop {
    pub user:String,
    pub applications:Vec<crate::applications::ApplicationSession>,
    pub explorers:Vec<crate::applications::explorer::Explorer>,
    pub windows:Vec<crate::shell::WindowState>,
    pub textboxes:Vec<crate::widgets::textbox::TextBox>,
    pub selected:Option<u32>,
}
impl crate::shell::ShellModel {
    pub fn session_hotkeys(&mut self,keys:u32) {
        use nt61_abi::session_bridge as w;
        if keys&w::SECURITY!=0 {self.start_menu.close();self.flyout=None;self.context_menu=None;self.session.show(if self.session.locked||self.session.logged_off{Screen::Credentials}else{Screen::Security});return;}
        if self.session.locked || self.session.logged_off || self.session.screen!=Screen::Desktop{return;}
        if keys&w::LOCK!=0 {self.power_action=Some(PowerAction::Lock);}
        else if keys&w::TASK_MANAGER!=0 {self.launch_application(crate::applications::AppId::TaskManager);}
        else if keys&w::CLOSE!=0 {
            if let Some(id)=self.selected_window {self.request_application_close(id);}else{self.start_menu.close();self.flyout=None;self.session.show(Screen::ShutDown);}
        }
    }
    pub fn session_intent(&mut self,intent:Intent){
        use crate::platform::services;
        match intent {
            Intent::None=>{},
            Intent::Cancel if self.session.switching=>{self.session.select_users();},
            Intent::Cancel=>{self.session.clear_passwords();self.session.show(if self.session.locked||self.session.logged_off{Screen::Credentials}else{Screen::Desktop});},
            Intent::Action(a)=>{self.power_action=Some(a);},
            Intent::TaskManager=>{if !self.session.locked&&!self.session.logged_off {self.session.show(Screen::Desktop);self.launch_application(crate::applications::AppId::TaskManager);}},
            Intent::Authenticate=>{
                let result=if self.session.switching {
                    let mut payload=[0u8;100];let user=self.session.user.as_bytes();let pass=self.session.password.as_str().unwrap_or("").as_bytes();
                    let n=user.len().min(64);payload[..n].copy_from_slice(&user[..n]);payload[n+1..n+1+pass.len()].copy_from_slice(pass);
                    let result=services::session("switch",&payload[..n+1+pass.len()]);
                    for b in &mut payload {unsafe{core::ptr::write_volatile(b,0);}}result
                }else{services::session("unlock",self.session.password.as_str().unwrap_or("").as_bytes())};
                self.session.clear_passwords();
                if result.is_ok(){
                    if self.session.switching {
                        let user=self.session.user.clone();
                        if !self.session.logged_off {
                            self.saved_desktops.push(SavedDesktop{user:self.session.identity.clone(),applications:core::mem::take(&mut self.applications),explorers:core::mem::take(&mut self.explorers),windows:core::mem::take(&mut self.taskbar.windows),textboxes:core::mem::take(&mut self.textboxes),selected:self.selected_window});
                        }
                        if let Some(i)=self.saved_desktops.iter().position(|d|d.user.eq_ignore_ascii_case(&user)){
                            let d=self.saved_desktops.remove(i);self.applications=d.applications;self.explorers=d.explorers;self.taskbar.windows=d.windows;self.textboxes=d.textboxes;self.selected_window=d.selected;
                        }else{self.selected_window=None;}
                        self.session.identity=user;self.text_target=None;self.desktop.selected.clear();self.desktop_editor=None;self.desktop_error.clear();
                    }
                    self.session.locked=false;self.session.logged_off=false;self.session.switching=false;self.session.show(Screen::Desktop);
                    self.refresh_audio();
                }else{self.session.message="The user name or password is incorrect.".into();}
            },
            Intent::ChangePassword=>{
                if self.session.new_password.as_str()!=self.session.password.as_str(){self.session.message="The passwords do not match.".into();return;}
                let mut payload=[0u8;65];let old=self.session.old_password.as_str().unwrap_or("").as_bytes();let new=self.session.new_password.as_str().unwrap_or("").as_bytes();
                payload[..old.len()].copy_from_slice(old);payload[old.len()+1..old.len()+1+new.len()].copy_from_slice(new);
                let result=services::session("password",&payload[..old.len()+1+new.len()]);for b in &mut payload{unsafe{core::ptr::write_volatile(b,0);}}
                self.session.clear_passwords();self.session.message=if result.is_ok(){"Your password has been changed.".into()}else{"The password could not be changed. Check your old password.".into()};
            }
        }
    }
    /// Returns a committed power operation only after each application accepts
    /// closing. Save prompts remain interactive; Cancel cancels the transition.
    pub fn advance_session(&mut self)->Option<PowerAction>{
        if let Some(action)=self.power_action.take(){
            self.start_menu.close();self.flyout=None;self.context_menu=None;
            match action {
                PowerAction::Lock|PowerAction::SwitchUser=>{
                    if crate::platform::services::session("lock",&[]).is_err(){self.session.message="Unable to lock the session.".into();return None;}
                    self.session.locked=true;self.session.switching=action==PowerAction::SwitchUser;self.session.named_user=true;
                    self.session.select_users();
                    if !self.session.switching {self.session.show(Screen::Credentials);self.session.user=self.session.identity.clone();}
                    return None;
                },
                PowerAction::Sleep=>{self.session.show(Screen::ShutDown);self.session.message="Sleep is not available on this computer.".into();return None;},
                _=>{self.session.show(Screen::Desktop);self.session.pending_close=Some(action);self.session.closing=None;}
            }
        }
        let action=self.session.pending_close?;
        if matches!(action,PowerAction::ShutDown|PowerAction::Restart)&&!self.saved_desktops.is_empty(){
            self.session.pending_close=None;self.session.closing=None;self.session.show(Screen::ShutDown);self.session.message="Other users are signed in. Log off their sessions before shutting down.".into();return None;
        }
        if let Some(id)=self.session.closing {
            if let Some(app)=self.application(id){
                if app.pending.is_none() && !app.confirm && app.dialog.is_none(){self.session.pending_close=None;self.session.closing=None;}
                return None;
            }
            self.session.closing=None;
        }
        if let Some(id)=self.taskbar.windows.last().map(|w|w.id){
            self.selected_window=Some(id);self.session.closing=Some(id);self.request_application_close(id);return None;
        }
        self.session.pending_close=None;self.session.show(Screen::Progress);self.session.progress=Some(action);Some(action)
    }
}
