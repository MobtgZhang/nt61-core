//! Retain the composed scene and the layers below the foremost window. A
//! pointer hover can repaint that window without rerasterizing every covered
//! application's text, controls and Aero shadows.
use crate::graphics::Surface;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Repaint { None, All, Window(u32) }

pub struct WindowScene {
    composed: Surface,
    beneath: Surface,
    foremost: Option<u32>,
    valid: bool,
}
impl WindowScene {
    pub fn new(width:u32, height:u32) -> Self {
        Self { composed:Surface::new(width,height), beneath:Surface::new(width,height), foremost:None, valid:false }
    }
    pub fn begin(&mut self, frame:&mut Surface, all:bool, window:Option<u32>) -> Repaint {
        if all || !self.valid || window.is_some_and(|id|Some(id)!=self.foremost) {
            self.foremost=None;
            return Repaint::All;
        }
        if let Some(id)=window {
            frame.copy_from(&self.beneath);
            Repaint::Window(id)
        } else {
            frame.copy_from(&self.composed);
            Repaint::None
        }
    }
    /// Called immediately before the last visible window is drawn, including
    /// its translucent shadow. Restoring this avoids accumulating alpha.
    pub fn save_beneath(&mut self, frame:&Surface, window:u32) {
        self.beneath.copy_from(frame);self.foremost=Some(window);
    }
    pub fn finish(&mut self, frame:&Surface) {
        self.composed.copy_from(frame);self.valid=true;
    }
}

#[cfg(test)] mod tests {
    use super::*;
    fn draw(cache:&mut WindowScene,frame:&mut Surface,full:bool,window:Option<u32>,color:u32)->usize {
        let plan=cache.begin(frame,full,window);
        if plan==Repaint::None {return 0;}
        let mut draws=0;
        if plan==Repaint::All {frame.fill_rect(0,0,32,24,0xff102030);}
        for id in 1..=6 {
            if matches!(plan,Repaint::Window(w) if w!=id) {continue;}
            if id==6 && plan==Repaint::All {cache.save_beneath(frame,id);}
            frame.shadow_rect(crate::graphics::Rect::new(id as i32,2,18,14),2);
            frame.fill_rect(id as i32,2,18,14,if id==6 {color}else{0xff405060+id});
            draws+=1;
        }
        cache.finish(frame);draws
    }
    #[test] fn six_overlapping_windows_repaint_only_changed_foremost_layer() {
        let mut cached=WindowScene::new(32,24);let mut frame=Surface::new(32,24);
        assert_eq!(draw(&mut cached,&mut frame,true,None,0xffabcdef),6);
        for color in [0xff123456,0xffabcdef,0xff123456] {
            assert_eq!(draw(&mut cached,&mut frame,false,Some(6),color),1);
            let mut oracle=Surface::new(32,24);let mut full=WindowScene::new(32,24);
            draw(&mut full,&mut oracle,true,None,color);
            assert_eq!(frame,oracle,"cached alpha/shadows must match complete composition");
        }
        assert_eq!(draw(&mut cached,&mut frame,false,None,0),0);
        assert_eq!(draw(&mut cached,&mut frame,false,Some(2),0xff123456),6,"a change beneath the foremost layer invalidates the stack");
    }
    #[test] fn topology_changes_require_fresh_beneath_even_when_window_ids_recur() {
        let mut cached=WindowScene::new(32,24);let mut frame=Surface::new(32,24);
        draw(&mut cached,&mut frame,true,None,0xffabcdef);
        assert_eq!(cached.begin(&mut frame,true,Some(6)),Repaint::All);
        frame.fill_rect(0,0,32,24,0xff999999);cached.finish(&frame);
        assert_eq!(cached.begin(&mut frame,false,Some(6)),Repaint::All,"removed window has no valid retained backdrop");
    }
}
