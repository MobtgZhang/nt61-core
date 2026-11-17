//! Control Panel account applet. All mutations cross the authorized SAM boundary.
use alloc::{string::String,vec::Vec,rc::Rc};
use crate::{accounts::{Account,Artwork},graphics::{Surface,Rect},login::PasswordEntry,compositor::aero};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]pub enum Page {Accounts,Details,Create,Pictures,Password}
pub struct Accounts {pub rows:Vec<Account>,pub page:Page,pub selected:usize,pub list_page:usize,pub message:String,pub name:String,pub password:PasswordEntry,pub confirm:PasswordEntry,pub current_password:PasswordEntry,pub current_user:String,pub field:usize,pub administrator:bool,pub artwork:Option<Rc<Artwork>>}
impl Default for Accounts {fn default()->Self{Self{rows:Vec::new(),page:Page::Accounts,selected:0,list_page:0,message:String::new(),name:String::new(),password:PasswordEntry::new(),confirm:PasswordEntry::new(),current_password:PasswordEntry::new(),current_user:String::new(),field:0,administrator:false,artwork:None}}}
impl Clone for Accounts {fn clone(&self)->Self{Self{rows:self.rows.clone(),page:self.page,selected:self.selected,list_page:self.list_page,message:self.message.clone(),name:self.name.clone(),current_user:self.current_user.clone(),field:self.field,administrator:self.administrator,artwork:self.artwork.clone(),..Self::default()}}}
impl core::fmt::Debug for Accounts {fn fmt(&self,f:&mut core::fmt::Formatter<'_>)->core::fmt::Result{f.debug_struct("Accounts").field("page",&self.page).finish()}}
impl PartialEq for Accounts {fn eq(&self,o:&Self)->bool{self.rows==o.rows&&self.page==o.page&&self.selected==o.selected&&self.list_page==o.list_page&&self.name==o.name&&self.message==o.message&&self.field==o.field&&self.administrator==o.administrator}}
impl Accounts {
    pub fn refresh(&mut self){match crate::platform::services::accounts(){Ok(r)=>{self.rows=r;self.selected=self.selected.min(self.rows.len().saturating_sub(1));self.list_page=0;},Err(c)=>self.message=alloc::format!("Unable to read accounts (0x{c:08X}).")};self.current_user=crate::platform::services::session("user",&[]).ok().and_then(|b|String::from_utf8(b).ok()).unwrap_or_default();if self.artwork.is_none(){self.artwork=Artwork::load();}}
    fn clear_passwords(&mut self){self.password.clear();self.confirm.clear();self.current_password.clear();}
    fn own_account(&self)->bool{self.rows.get(self.selected).is_some_and(|a|a.name.eq_ignore_ascii_case(&self.current_user))}
    fn result(&mut self,result:Result<(),u32>){self.message=match result {Ok(())=>{self.refresh();"Your changes have been saved.".into()},Err(c)=>alloc::format!("The change could not be saved (0x{c:08X}).")};}
    pub fn key(&mut self,key:u8)->bool {
        if !matches!(self.page,Page::Create|Page::Password){return false;}
        match key {27=>{self.clear_passwords();self.page=Page::Accounts;},9=>self.field=(self.field+1)%3,13|10=>if self.page==Page::Create{self.create()}else{self.change_password()},_=>{
            if self.field==0&&self.page==Page::Create{if key==8{self.name.pop();}else if (32..127).contains(&key)&&self.name.len()<64{self.name.push(key as char);}}
            else {let entry=if self.field==0{&mut self.current_password}else if self.field==1{&mut self.password}else{&mut self.confirm};if key==8{entry.backspace();}else if (32..127).contains(&key){entry.append(key);}}
        }}true
    }
    fn change_password(&mut self){
        if !self.own_account(){self.clear_passwords();self.page=Page::Accounts;return;}
        if self.password.as_str()!=self.confirm.as_str(){self.message="The passwords do not match.".into();return;}
        let old=self.current_password.as_str().unwrap_or("").as_bytes();
        let new=self.password.as_str().unwrap_or("").as_bytes();
        let mut credentials=[0u8;65];credentials[..old.len()].copy_from_slice(old);credentials[old.len()+1..old.len()+1+new.len()].copy_from_slice(new);
        let result=crate::platform::services::session("password",&credentials[..old.len()+1+new.len()]).map(|_|());
        for byte in &mut credentials{unsafe{core::ptr::write_volatile(byte,0);}}
        self.clear_passwords();if result.is_ok(){self.page=Page::Details;}else{self.field=0;}
        self.result(result);
    }
    fn create(&mut self){
        if self.password.as_str()!=self.confirm.as_str(){self.message="The passwords do not match.".into();return;}
        let result=crate::platform::services::account_control(&self.name,"create",self.password.as_str().unwrap_or("").as_bytes());
        self.password.clear();self.confirm.clear();
        if result.is_ok(){if self.administrator{let result=crate::platform::services::account_control(&self.name,"type",b"1");if let Err(c)=result{self.message=alloc::format!("Account created; account type could not be changed (0x{c:08X}).");self.refresh();return;}}self.page=Page::Accounts;self.name.clear();}
        self.result(result);
    }
    pub fn click(&mut self,b:Rect,x:i32,y:i32)->bool {
        if !b.contains(x,y){return false;}
        if Rect::new(b.x+18,b.y+10,120,26).contains(x,y){self.page=Page::Accounts;self.clear_passwords();self.message.clear();return true;}
        match self.page {
            Page::Accounts=>{
                let capacity=list_capacity(b);let start=self.list_page*capacity;
                for (local,i) in (start..(start+capacity).min(self.rows.len())).enumerate(){if account_rect(b,local).contains(x,y){self.selected=i;self.page=Page::Details;self.message.clear();return true;}}
                if self.list_page>0&&list_nav(b,false).contains(x,y){self.list_page-=1;return true;}
                if start+capacity<self.rows.len()&&list_nav(b,true).contains(x,y){self.list_page+=1;return true;}
                if create_rect(b).contains(x,y){self.page=Page::Create;self.name.clear();self.password.clear();self.confirm.clear();self.field=0;self.message.clear();}
            },
            Page::Details=>{
                let Some(account)=self.rows.get(self.selected).cloned()else{return true;};
                if self.own_account()&&Rect::new(b.x+160,b.y+239,330,30).contains(x,y){self.clear_passwords();self.field=0;self.message.clear();self.page=Page::Password;return true;}
                for (i,command) in ["picture","type","enabled"].iter().enumerate(){if Rect::new(b.x+160,b.y+125+i as i32*38,330,30).contains(x,y){
                    if *command=="picture"{self.page=Page::Pictures;return true;}
                    let value=if if *command=="type"{!account.administrator}else{!account.enabled}{b"1"}else{b"0"};self.result(crate::platform::services::account_control(&account.name,command,value));return true;
                }}
            },
            Page::Create=>{
                for i in 0..3 {if Rect::new(b.x+180,b.y+90+i*42,280,30).contains(x,y){self.field=i as usize;}}
                if Rect::new(b.x+30,b.y+230,200,24).contains(x,y){self.administrator=false;}
                if Rect::new(b.x+250,b.y+230,200,24).contains(x,y){self.administrator=true;}
                if Rect::new(b.x+320,b.y+280,140,28).contains(x,y){self.create();}
            },
            Page::Password=>{
                for i in 0..3{if Rect::new(b.x+180,b.y+90+i*42,280,30).contains(x,y){self.field=i as usize;}}
                if Rect::new(b.x+320,b.y+240,140,28).contains(x,y){self.change_password();}
            },
            Page::Pictures=>{
                for i in 0..36 {let r=picture_rect(b,i);if r.contains(x,y){if let Some(a)=self.rows.get(self.selected){let result=crate::platform::services::account_control(&a.name,"picture",alloc::format!("{i}").as_bytes());if result.is_ok(){self.page=Page::Details;}self.result(result);}return true;}}
            }
        }true
    }
}
fn list_capacity(b:Rect)->usize{(b.height.saturating_sub(210)/96).max(1)as usize*2}
fn create_rect(b:Rect)->Rect{Rect::new(b.x+24,b.y+b.height as i32-102,240,28)}
fn list_nav(b:Rect,next:bool)->Rect{Rect::new(b.x+b.width as i32-if next{100}else{210},b.y+b.height as i32-102,95,28)}
fn account_rect(b:Rect,i:usize)->Rect{Rect::new(b.x+24+(i%2)as i32*285,b.y+85+(i/2)as i32*96,270,88)}
fn picture_rect(b:Rect,i:usize)->Rect{Rect::new(b.x+24+(i%9)as i32*58,b.y+85+(i/9)as i32*62,52,52)}
pub fn draw(state:&Accounts,frame:&mut Surface,b:Rect,text:impl Fn(&mut Surface,&str,Rect,u32)){
    frame.fill_rect(b.x,b.y,b.width,b.height,0xFFFFFFFF);
    let blue=0xFF0066BB;let black=0xFF222222;
    text(frame,"User Accounts",Rect::new(b.x+18,b.y+14,300,24),blue);
    text(frame,match state.page{Page::Accounts=>"Choose the account you would like to change",Page::Details=>"Make changes to this account",Page::Create=>"Name the account and choose an account type",Page::Pictures=>"Choose a new picture for your account",Page::Password=>"Change your password"},Rect::new(b.x+24,b.y+50,b.width.saturating_sub(36),24),blue);
    match state.page {
        Page::Accounts=>{
            let capacity=list_capacity(b);let start=state.list_page*capacity;
            for(local,i)in (start..(start+capacity).min(state.rows.len())).enumerate(){let a=&state.rows[i];let r=account_rect(b,local);aero::blend_rounded_rect(frame,r,4,0xFFF3F8FC);if let Some(art)=&state.artwork{art.portrait(frame,Rect::new(r.x+6,r.y+6,70,70),a,false);}text(frame,&a.name,Rect::new(r.x+86,r.y+14,180,24),black);text(frame,if a.administrator{"Administrator"}else if a.rid==501{"Guest"}else{"Standard user"},Rect::new(r.x+86,r.y+39,180,20),black);if !a.enabled{text(frame,"Account is off",Rect::new(r.x+86,r.y+61,180,20),black);}}
            text(frame,"Create a new account",create_rect(b),blue);
            if start>0{text(frame,"< Previous",list_nav(b,false),blue);}
            if start+capacity<state.rows.len(){text(frame,"Next >",list_nav(b,true),blue);}
        },
        Page::Details=>if let Some(a)=state.rows.get(state.selected){if let Some(art)=&state.artwork{art.portrait(frame,Rect::new(b.x+24,b.y+92,100,100),a,false);}text(frame,&a.name,Rect::new(b.x+160,b.y+90,300,24),black);for(i,label)in ["Change the picture",if a.administrator{"Change to Standard user"}else{"Change to Administrator"},if a.enabled{"Turn off this account"}else{"Turn on this account"}].iter().enumerate(){text(frame,label,Rect::new(b.x+160,b.y+132+i as i32*38,330,24),blue);}if state.own_account(){text(frame,"Change your password",Rect::new(b.x+160,b.y+246,330,24),blue);}},
        Page::Password=>{
            for(i,label)in ["Current password","New password","Confirm password"].iter().enumerate(){text(frame,label,Rect::new(b.x+24,b.y+98+i as i32*42,150,22),black);let r=Rect::new(b.x+180,b.y+90+i as i32*42,280,30);frame.fill_rect(r.x,r.y,r.width,r.height,if state.field==i{0xFF3C9CDD}else{0xFFAAAAAA});frame.fill_rect(r.x+1,r.y+1,r.width-2,r.height-2,0xFFFFFFFF);let len=match i{0=>state.current_password.len(),1=>state.password.len(),_=>state.confirm.len()};text(frame,&"*".repeat(len),Rect::new(r.x+5,r.y+6,270,24),black);}
            let r=Rect::new(b.x+320,b.y+240,140,28);aero::blend_rounded_rect(frame,r,3,0xFFE4EEF5);text(frame,"Change password",Rect::new(r.x+8,r.y+6,130,22),black);
        },
        Page::Create=>{
            for(i,label)in ["New account name","Password","Confirm password"].iter().enumerate(){text(frame,label,Rect::new(b.x+24,b.y+98+i as i32*42,150,22),black);let r=Rect::new(b.x+180,b.y+90+i as i32*42,280,30);frame.fill_rect(r.x,r.y,r.width,r.height,if state.field==i{0xFF3C9CDD}else{0xFFAAAAAA});frame.fill_rect(r.x+1,r.y+1,r.width-2,r.height-2,0xFFFFFFFF);let value=match i{0=>state.name.clone(),1=>"*".repeat(state.password.len()),_=>"*".repeat(state.confirm.len())};text(frame,&value,Rect::new(r.x+5,r.y+6,270,24),black);}
            text(frame,if state.administrator{"( ) Standard user"}else{"(*) Standard user"},Rect::new(b.x+30,b.y+230,200,24),black);text(frame,if state.administrator{"(*) Administrator"}else{"( ) Administrator"},Rect::new(b.x+250,b.y+230,200,24),black);
            let r=Rect::new(b.x+320,b.y+280,140,28);aero::blend_rounded_rect(frame,r,3,0xFFE4EEF5);text(frame,"Create Account",Rect::new(r.x+12,r.y+6,125,22),black);
        },
        Page::Pictures=>if let Some(a)=state.rows.get(state.selected){for i in 0..36 {let mut a=a.clone();a.picture=i as u32;if let Some(art)=&state.artwork{art.portrait(frame,picture_rect(b,i),&a,false);}}}
    }
    text(frame,&state.message,Rect::new(b.x+24,b.y+b.height as i32-55,b.width.saturating_sub(48),45),0xFF992222);
}
