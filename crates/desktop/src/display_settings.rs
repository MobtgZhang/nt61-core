//! Display settings backed by the active display driver, with timed rollback.
use alloc::{format, string::String, vec::Vec};
use crate::{graphics::{Rect,Surface},shell::ShellModel};
#[derive(Clone,Debug,Default,PartialEq,Eq)]
pub struct Settings {
    pub now: u64, pub modes: Vec<(u32,u32)>, pub selected: usize, pub open: bool, pub first_visible: usize,
    pub previous: Option<(u32,u32)>, pub deadline: u64, pub error: String,
}
impl Settings {
    pub fn choose(&mut self, index:usize) {
        if index < self.modes.len() {self.selected=index;self.open=false;}
    }
    pub fn reveal_selection(&mut self) {
        if self.selected < self.first_visible { self.first_visible = self.selected; }
        if self.selected >= self.first_visible + 8 { self.first_visible = self.selected - 7; }
    }
    pub fn expired(&self,ticks:u64)->bool {self.previous.is_some() && ticks>=self.deadline}
}
impl ShellModel {
    pub fn load_display_modes(&mut self) {
        if self.display.modes.is_empty() {
            self.display.modes=crate::platform::services::display_modes().unwrap_or_else(|_|alloc::vec![self.viewport]);
            self.display.selected=self.display.modes.iter().position(|m|*m==self.viewport).unwrap_or(0);
        }
    }
    pub fn display_tick(&mut self,ticks:u64)->bool {
        self.display.now=ticks;
        if self.display.expired(ticks) {self.revert_display();return true;} false
    }
    pub fn revert_display(&mut self) {
        if let Some((w,h))=self.display.previous {
            match crate::platform::services::display_set(w,h) {
                Ok(())=>{self.display.previous=None;self.display.selected=self.display.modes.iter().position(|m|*m==(w,h)).unwrap_or(0);self.display.error.clear();}
                Err(s)=>{self.display.error=format!("Could not restore the display (0x{s:08X}).");}
            }
        }
    }
    pub fn display_key(&mut self,key:u8)->bool {
        self.load_display_modes();
        match key {
            27 => { self.display.open=false; self.revert_display(); }
            0x26 | 0x28 if self.display.previous.is_none() => {
                if key==0x26 {self.display.selected=self.display.selected.saturating_sub(1);}
                else {self.display.selected=(self.display.selected+1).min(self.display.modes.len().saturating_sub(1));}
                self.display.open=true; self.display.reveal_selection();
            }
            13 | 10 if self.display.open => {self.display.open=false;}
            _=>return false,
        }
        true
    }
    pub fn display_click(&mut self,b:Rect,x:i32,y:i32,ticks:u64)->bool {
        self.load_display_modes();
        if self.display.previous.is_some() {
            if Rect::new(b.x+20,b.y+210,140,28).contains(x,y) {
                match crate::platform::services::display_preference("keep") {
                    Ok(())=>{self.display.previous=None;self.display.error.clear();}
                    Err(s)=>self.display.error=format!("Could not save this setting (0x{s:08X})."),
                }
            }
            else if Rect::new(b.x+174,b.y+210,140,28).contains(x,y){self.revert_display();}
            return true;
        }
        let dropdown=Rect::new(b.x+126,b.y+112,240,28);
        if self.display.open {
            if x>=dropdown.x && x<dropdown.x+dropdown.width as i32 && y>=dropdown.y+28 {
                let row=((y-dropdown.y-28)/22)as usize;
                if row == 0 { self.display.first_visible = self.display.first_visible.saturating_sub(1); }
                else if row == 9 { self.display.first_visible = (self.display.first_visible+1).min(self.display.modes.len().saturating_sub(8)); }
                else if row <= 8 { self.display.choose(self.display.first_visible + row - 1); }
                else { self.display.open=false; }
            } else {self.display.open=false;}
            return true;
        }
        if dropdown.contains(x,y){self.display.open=true;self.display.reveal_selection();return true;}
        if Rect::new(b.x+20,b.y+210,100,28).contains(x,y) {
            if let Some(&(w,h))=self.display.modes.get(self.display.selected) {
                if (w,h)==self.viewport{return true;}
                match crate::platform::services::display_set(w,h) {
                    Ok(())=>{self.display.previous=Some(self.viewport);self.display.deadline=ticks.saturating_add(15000);self.display.error.clear();}
                    Err(s)=>self.display.error=format!("The display driver rejected this mode (0x{s:08X})."),
                }
            }
            return true;
        }
        false
    }
}
pub fn draw(s:&Settings,f:&mut Surface,b:Rect,ticks:u64,mut text:impl FnMut(&mut Surface,&str,Rect,u32)) {
    text(f,"Display:",Rect::new(b.x+20,b.y+70,100,22),0xFF111111);
    text(f,"Primary monitor",Rect::new(b.x+126,b.y+70,240,22),0xFF111111);
    text(f,"Resolution:",Rect::new(b.x+20,b.y+118,100,22),0xFF111111);
    let d=Rect::new(b.x+126,b.y+112,240,28);
    f.fill_rect(d.x,d.y,d.width,d.height,0xFF7A9CBF);f.fill_rect(d.x+1,d.y+1,d.width-2,d.height-2,0xFFFFFFFF);
    if let Some((w,h))=s.modes.get(s.selected) {text(f,&format!("{w} x {h}"),Rect::new(d.x+8,d.y+6,200,20),0xFF111111);}
    text(f,"v",Rect::new(d.x+220,d.y+6,18,20),0xFF111111);
    text(f,"Orientation:     Landscape",Rect::new(b.x+20,b.y+166,b.width-40,22),0xFF111111);
    let buttons=if s.previous.is_some(){[(20,"Keep changes",140),(174,"Revert",140)]}else{[(20,"Apply",100),(134,"",0)]};
    for (x,label,w) in buttons {if w!=0 {
        f.fill_rect(b.x+x,b.y+210,w,28,0xFF7A9CBF);f.fill_rect(b.x+x+1,b.y+211,w-2,26,0xFFF0F0F0);
        text(f,label,Rect::new(b.x+x+10,b.y+215,w-16,22),0xFF111111);
    }}
    if s.previous.is_some(){text(f,&format!("Reverting in {} seconds",s.deadline.saturating_sub(ticks).div_ceil(1000)),Rect::new(b.x+20,b.y+252,b.width-40,24),0xFF173F67);}
    text(f,&s.error,Rect::new(b.x+20,b.y+284,b.width.saturating_sub(40),44),0xFF991111);
    // An ordinary opaque ComboBox list. Shadow and border must cover the page.
    if s.open {
        let height=220;
        f.shadow_rect(Rect::new(d.x,d.y+28,d.width,height),3);
        f.fill_rect(d.x,d.y+28,d.width,height,0xFF7A9CBF);
        f.fill_rect(d.x+1,d.y+29,d.width-2,height-2,0xFFFFFFFF);
        text(f,"^",Rect::new(d.x+112,d.y+31,18,20),0xFF111111);
        text(f,"v",Rect::new(d.x+112,d.y+229,18,20),0xFF111111);
        for (i,(w,h)) in s.modes.iter().enumerate().skip(s.first_visible).take(8) {
            let y=d.y+50+(i-s.first_visible) as i32*22;
            if i==s.selected{f.fill_rect(d.x+1,y,d.width-2,22,0xFF3399FF);}
            text(f,&format!("{w} x {h}"),Rect::new(d.x+8,y+3,d.width-16,20),if i==s.selected{0xFFFFFFFF}else{0xFF111111});
        }
    }
}
#[cfg(test)] mod tests {
    use super::*;
    #[test]fn confirmation_timeout_and_mode_selection(){
        let mut s=Settings{modes:alloc::vec![(800,600),(1024,768)],..Default::default()};
        s.choose(1);assert_eq!(s.selected,1);s.choose(99);assert_eq!(s.selected,1);
        s.previous=Some((800,600));s.deadline=15000;
        assert!(!s.expired(14999));assert!(s.expired(15000));
        s.previous=None;assert!(!s.expired(20000));
    }
}
