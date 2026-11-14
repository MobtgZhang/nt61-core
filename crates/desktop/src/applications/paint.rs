//! Raster document and pointer tools. Image dimensions are independent of the viewport.
use alloc::{string::String,vec,vec::Vec};
use crate::graphics::Surface;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Tool { Pencil, Brush, Eraser, Fill, Picker, Line, Rectangle, Ellipse }
#[derive(Clone,Debug,PartialEq,Eq)]
pub struct Paint {
    pub canvas:Surface,pub color:u32,pub background:u32,pub tool:Tool,pub size:u32,
    pub dirty:bool,pub file_id:Option<u32>,pub filename:String,pub zoom:u32,pub scroll:(u32,u32),pub view_tab:bool,pub grid:bool,
    undo:Vec<Surface>,redo:Vec<Surface>,last:Option<(i32,i32)>,origin:Option<(i32,i32)>,stroke_color:u32,saved:u64,
}
pub const MAX_PIXELS:u32=1024*1024;
const HISTORY_BYTES:usize=8*1024*1024;
pub const COLORS:&[u32]=&[0xFF000000,0xFF7F7F7F,0xFF880015,0xFFED1C24,0xFFFF7F27,0xFFFFF200,0xFF22B14C,0xFF00A2E8,0xFF3F48CC,0xFFA349A4,0xFFFFFFFF,0xFFC3C3C3,0xFFB97A57,0xFFFFAEC9,0xFFFFC90E,0xFFEFE4B0,0xFFB5E61D,0xFF99D9EA,0xFF7092BE,0xFFC8BFE7];
impl Paint {
    pub fn new(width:u32,height:u32)->Self{
        let (width,height)=if width==0||height==0||width.checked_mul(height).is_none_or(|n|n>MAX_PIXELS){(480,240)}else{(width,height)};
        let mut canvas=Surface::new(width,height);canvas.fill_rect(0,0,width,height,0xFFFFFFFF);
        let mut p=Self{canvas,color:COLORS[0],background:0xFFFFFFFF,tool:Tool::Pencil,size:2,dirty:false,file_id:None,filename:"Untitled.bmp".into(),zoom:100,scroll:(0,0),view_tab:false,grid:false,undo:Vec::new(),redo:Vec::new(),last:None,origin:None,stroke_color:COLORS[0],saved:0};p.mark_saved();p
    }
    fn fingerprint(&self)->u64{self.canvas.pixels().iter().fold(((self.canvas.width()as u64)<<32)|self.canvas.height()as u64,|h,p|(h^*p as u64).wrapping_mul(0x100000001b3))}
    pub fn mark_saved(&mut self){self.saved=self.fingerprint();self.dirty=false;}
    fn changed(&mut self){self.dirty=self.fingerprint()!=self.saved;}
    fn checkpoint(&mut self){
        self.end();self.redo.clear();
        while !self.undo.is_empty()&&(self.undo.len()>=16||self.undo.iter().map(|s|s.pixels().len()*4).sum::<usize>()+self.canvas.pixels().len()*4>HISTORY_BYTES){self.undo.remove(0);}
        self.undo.push(self.canvas.clone());
    }
    pub fn load_bmp(&mut self,bytes:&[u8])->Result<(),&'static str>{
        // Reject dimensions before allocating the decoder's RGBA buffer.
        if bytes.len()<26{return Err("Invalid bitmap header.");}
        let w=i32::from_le_bytes(bytes[18..22].try_into().unwrap());let h=i32::from_le_bytes(bytes[22..26].try_into().unwrap());
        if w<=0||h==0||(w as u32).checked_mul(h.unsigned_abs()).is_none_or(|n|n>MAX_PIXELS){return Err("Bitmap exceeds the supported 1,048,576 pixels.");}
        let image=crate::bmp::decode_bmp(bytes).map_err(|_|"Invalid or unsupported bitmap.")?;
        let mut canvas=Surface::new(image.width,image.height);
        for(y,row)in image.pixels.chunks_exact(image.width as usize*4).enumerate(){for(x,c)in row.chunks_exact(4).enumerate(){canvas.set_pixel(x as u32,y as u32,u32::from_le_bytes([c[2],c[1],c[0],0xff]));}}
        self.canvas=canvas;self.undo.clear();self.redo.clear();self.last=None;self.origin=None;self.scroll=(0,0);self.mark_saved();Ok(())
    }
    pub fn begin(&mut self,x:i32,y:i32){self.begin_button(x,y,false);}
    pub fn begin_button(&mut self,x:i32,y:i32,right:bool){
        if !self.inside(x,y){return;}
        if self.tool==Tool::Picker{let c=self.canvas.pixel(x as u32,y as u32).unwrap();if right{self.background=c;}else{self.color=c;}return;}
        self.checkpoint();self.stroke_color=if right{self.background}else{self.color};self.origin=Some((x,y));self.last=None;self.stroke(x,y);
    }
    pub fn active(&self)->bool{self.origin.is_some()}
    pub fn end(&mut self){if self.origin.take().is_some(){self.changed();}self.last=None;}
    pub fn undo(&mut self){self.end();if let Some(canvas)=self.undo.pop(){self.redo.push(core::mem::replace(&mut self.canvas,canvas));self.changed();}}
    pub fn redo(&mut self){self.end();if let Some(canvas)=self.redo.pop(){self.undo.push(core::mem::replace(&mut self.canvas,canvas));self.changed();}}
    pub fn clear(&mut self){self.checkpoint();self.canvas.fill_rect(0,0,self.canvas.width(),self.canvas.height(),self.background);self.changed();}
    fn inside(&self,x:i32,y:i32)->bool{x>=0&&y>=0&&x<self.canvas.width()as i32&&y<self.canvas.height()as i32}
    fn line(&mut self,a:(i32,i32),b:(i32,i32),color:u32,size:u32){
        let(mut x,mut y)=a;let dx=(b.0-x).abs();let dy=-(b.1-y).abs();let sx=if x<b.0{1}else{-1};let sy=if y<b.1{1}else{-1};let mut e=dx+dy;
        loop{self.canvas.fill_rect(x-size as i32/2,y-size as i32/2,size,size,color);if (x,y)==b{break;}let twice=2*e;if twice>=dy{e+=dy;x+=sx;}if twice<=dx{e+=dx;y+=sy;}}
    }
    pub fn stroke(&mut self,x:i32,y:i32){
        let Some(origin)=self.origin else{return;};
        let x=x.clamp(0,self.canvas.width()as i32-1);let y=y.clamp(0,self.canvas.height()as i32-1);
        let color=self.stroke_color;
        match self.tool {
            Tool::Fill=>{if self.last.is_none(){self.flood(x as u32,y as u32);}self.last=Some((x,y));},
            Tool::Picker=>{},
            Tool::Line|Tool::Rectangle|Tool::Ellipse=>{
                if let Some(base)=self.undo.last(){self.canvas=base.clone();}
                let (l,r,t,b)=(origin.0.min(x),origin.0.max(x),origin.1.min(y),origin.1.max(y));
                match self.tool{
                    Tool::Line=>self.line(origin,(x,y),color,self.size),
                    Tool::Rectangle=>{for(a,b)in [((l,t),(r,t)),((r,t),(r,b)),((r,b),(l,b)),((l,b),(l,t))]{self.line(a,b,color,self.size);}},
                    _=>{let rx=(r-l)as i64;let ry=(b-t)as i64;if rx==0||ry==0{self.line((l,t),(r,b),color,self.size);}else{
                        // Find the implicit boundary with binary search. Work scales
                        // with the perimeter; large previews must not scan the area.
                        let extent=|axis:i128,other:i128,delta:i128|->i32{
                            let(mut lo,mut hi)=(0i128,axis);
                            let limit=axis*axis*(other*other-delta*delta);
                            while lo<hi{let mid=(lo+hi+1)/2;if mid*mid*other*other<=limit{lo=mid;}else{hi=mid-1;}}
                            lo as i32
                        };
                        for yy in t..=b{let xx=extent(rx as i128,ry as i128,(2*yy-t-b)as i128);
                            for px in [(l+r-xx)/2,(l+r+xx)/2]{self.canvas.fill_rect(px-self.size as i32/2,yy-self.size as i32/2,self.size,self.size,color);}}
                        for xx in l..=r{let yy=extent(ry as i128,rx as i128,(2*xx-l-r)as i128);
                            for py in [(t+b-yy)/2,(t+b+yy)/2]{self.canvas.fill_rect(xx-self.size as i32/2,py-self.size as i32/2,self.size,self.size,color);}}

                    }}
                }
                self.last=Some((x,y));
            },
            _=>{let size=if self.tool==Tool::Eraser{self.size.max(9)}else if self.tool==Tool::Brush{self.size.max(6)}else{self.size};let color=if self.tool==Tool::Eraser{self.background}else{color};self.line(self.last.unwrap_or((x,y)),(x,y),color,size);self.last=Some((x,y));},
        }
        self.dirty=true;
    }
    pub fn rotate(&mut self){self.checkpoint();let(w,h)=(self.canvas.width(),self.canvas.height());let mut out=Surface::new(h,w);for y in 0..h{for x in 0..w{out.set_pixel(h-1-y,x,self.canvas.pixel(x,y).unwrap());}}self.canvas=out;self.scroll=(0,0);self.changed();}
    pub fn flip(&mut self,horizontal:bool){self.checkpoint();let(w,h)=(self.canvas.width(),self.canvas.height());let mut out=Surface::new(w,h);for y in 0..h{for x in 0..w{out.set_pixel(if horizontal{w-1-x}else{x},if horizontal{y}else{h-1-y},self.canvas.pixel(x,y).unwrap());}}self.canvas=out;self.changed();}
    pub fn resize(&mut self,width:u32,height:u32)->Result<(),&'static str>{if width==0||height==0||width.checked_mul(height).is_none_or(|n|n>MAX_PIXELS){return Err("Invalid image dimensions.");}self.checkpoint();let mut out=Surface::new(width,height);for y in 0..height{for x in 0..width{out.set_pixel(x,y,self.canvas.pixel(x*self.canvas.width()/width,y*self.canvas.height()/height).unwrap());}}self.canvas=out;self.scroll=(0,0);self.changed();Ok(())}
    fn flood(&mut self,x:u32,y:u32){let old=self.canvas.pixel(x,y).unwrap();if old==self.stroke_color{return;}let(w,h)=(self.canvas.width(),self.canvas.height());let mut stack=vec![(x,y)];self.canvas.set_pixel(x,y,self.stroke_color);while let Some((x,y))=stack.pop(){for(nx,ny)in [(x.wrapping_sub(1),y),(x+1,y),(x,y.wrapping_sub(1)),(x,y+1)]{if nx<w&&ny<h&&self.canvas.pixel(nx,ny)==Some(old){self.canvas.set_pixel(nx,ny,self.stroke_color);stack.push((nx,ny));}}}}
}
/// Windows-compatible uncompressed bottom-up 32-bit BMP, including full file/DIB headers.
pub fn encode_bmp(surface:&Surface)->Vec<u8> {let size=54+surface.pixels().len()*4;let mut out=vec![0;54];out[..2].copy_from_slice(b"BM");out[2..6].copy_from_slice(&(size as u32).to_le_bytes());out[10..14].copy_from_slice(&54u32.to_le_bytes());out[14..18].copy_from_slice(&40u32.to_le_bytes());out[18..22].copy_from_slice(&surface.width().to_le_bytes());out[22..26].copy_from_slice(&surface.height().to_le_bytes());out[26..28].copy_from_slice(&1u16.to_le_bytes());out[28..30].copy_from_slice(&32u16.to_le_bytes());for y in (0..surface.height()).rev(){for x in 0..surface.width(){out.extend_from_slice(&surface.pixel(x,y).unwrap_or(0).to_le_bytes());}}out}
#[cfg(test)] mod tests {use super::*;#[test] fn strokes_are_continuous_undo_and_bmp_round_trip(){let mut p=Paint::new(20,20);p.begin(1,1);p.stroke(16,16);p.end();assert_eq!(p.canvas.pixel(8,8),Some(COLORS[0]));let b=encode_bmp(&p.canvas);let decoded=crate::bmp::decode_bmp(&b).expect("bmp");assert_eq!(decoded.width,20);p.undo();assert_eq!(p.canvas.pixel(8,8),Some(0xFFFFFFFF));p.tool=Tool::Fill;p.color=COLORS[3];p.begin(0,0);assert_eq!(p.canvas.pixel(19,19),Some(COLORS[3]));}}
