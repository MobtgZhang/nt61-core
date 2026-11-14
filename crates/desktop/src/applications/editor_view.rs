//! Text viewport and modeless Find/Replace controls, shared by native and hosted editors.
use alloc::{format,string::String};
use crate::graphics::{Rect,Surface};
use super::document::Document;
#[derive(Clone,Debug,PartialEq,Eq)]
pub struct Search{pub query:String,pub replacement:String,pub replace:bool,pub replacement_focus:bool,pub case_sensitive:bool,pub message:String}
impl Search{
    pub fn new(replace:bool)->Self{Self{query:String::new(),replacement:String::new(),replace,replacement_focus:false,case_sensitive:false,message:String::new()}}
    pub fn key(&mut self,d:&mut Document,key:u8){match key{
        9=>self.replacement_focus=!self.replacement_focus&&self.replace,
        13=>{if !d.find(&self.query,self.case_sensitive){self.message="Cannot find the text.".into();}else{self.message.clear();}},
        18 if self.replace=>{let selected=d.selected_text();let matches=if self.case_sensitive{selected==self.query}else{selected.to_lowercase()==self.query.to_lowercase()};if matches&&!selected.is_empty(){d.replace_selected(&self.replacement);}if !d.find(&self.query,self.case_sensitive){self.message="Cannot find the text.".into();}},
        8|0x20..=0x7e=>{let field=if self.replacement_focus{&mut self.replacement}else{&mut self.query};if key==8{field.pop();}else if field.len()<1024{field.push(key as char);}},_=>{}
    }}
}
pub fn bounds(b:Rect)->Rect{Rect::new(b.x+4,b.y+36,b.width.saturating_sub(8),b.height.saturating_sub(64))}
fn cell_width(ch:char,col:usize)->usize{if ch=='\t'{4-col%4}else if ch>='\u{1100}'{2}else{1}}
/// Walk character positions without allocating a second copy of the document.
fn layout(d:&Document,cols:usize,mut visit:impl FnMut(usize,usize,usize,char)->bool)->(usize,usize,usize){
    let(mut row,mut col,mut index)=(0,0,0);
    for ch in d.editor.text.chars(){
        let width=cell_width(ch,col);
        if d.word_wrap&&ch!='\n'&&col>0&&col+width>cols{row+=1;col=0;}
        if !visit(row,col,index,ch){return(row,col,index);}
        if ch=='\n'{row+=1;col=0;}else{col+=cell_width(ch,col);}index+=1;
    }(row,col,index)
}
fn caret(d:&Document,cols:usize)->(usize,usize){let(r,c,_)=layout(d,cols,|_,_,index,_|index<d.editor.cursor);(r,c)}
fn viewport(d:&Document,r:Rect)->(usize,usize,usize){
    let rows=(r.height/21).max(1)as usize;let cols=(r.width.saturating_sub(8)/9).max(1)as usize;
    let(row,col)=caret(d,cols);let first=if row<d.scroll{row}else if row>=d.scroll+rows{row-rows+1}else{d.scroll};
    (first,if d.word_wrap{0}else{col.saturating_sub(cols-1)},cols)
}
pub fn draw<F:Fn(&mut Surface,&str,Rect,u32)>(d:&Document,frame:&mut Surface,b:Rect,text:&F){
    let r=bounds(b);frame.fill_rect(r.x,r.y,r.width,r.height,0xFFFFFFFF);
    let(first,left,cols)=viewport(d,r);let rows=(r.height/21).max(1)as usize;let selection=d.selection();
    layout(d,cols,|row,col,index,ch|{
        if row>=first+rows{return false;}
        if row>=first&&col>=left&&col<left+cols{
            let width=cell_width(ch,col).min(left+cols-col);
            let cell=Rect::new(r.x+4+(col-left)as i32*9,r.y+3+(row-first)as i32*21,width as u32*9,21);
            let selected=selection.is_some_and(|(a,b)|index>=a&&index<b);
            if selected{frame.fill_rect(cell.x,cell.y,cell.width,cell.height,0xFF3399FF);}
            if ch!='\t'&&ch!='\n'{let mut bytes=[0;4];text(frame,ch.encode_utf8(&mut bytes),cell,if selected{0xFFFFFFFF}else{0xFF182534});}
        }true
    });
    let(row,col)=caret(d,cols);if row>=first&&row<first+rows&&col>=left{frame.fill_rect(r.x+4+(col-left)as i32*9,r.y+3+(row-first)as i32*21,1,19,0xFF182534);}
    let(line,col)=d.position();text(frame,&format!("Ln {}, Col {}   {:?}   {}",line+1,col+1,d.encoding,if d.crlf{"Windows (CRLF)"}else{"Unix (LF)"}),Rect::new(b.x+6,b.y+b.height as i32-25,b.width.saturating_sub(12),21),0xFF526578);
}
pub fn click(d:&mut Document,b:Rect,x:i32,y:i32){pointer(d,b,x,y,true,true);d.drag_anchor=None;}
pub fn pointer(d:&mut Document,b:Rect,x:i32,y:i32,down:bool,held:bool)->bool{
    if !held&&!down{d.drag_anchor=None;return false;}
    let r=bounds(b);if !r.contains(x,y){return false;}
    if !down&&d.drag_anchor.is_none(){return false;}
    let(first,left,cols)=viewport(d,r);let row=((y-r.y-3).max(0)/21)as usize+first;let col=((x-r.x-4).max(0)/9)as usize+left;
    let mut target=0;layout(d,cols,|r,c,i,ch|{if r>row{return false;}target=i;if r==row&&c>=col{return false;}if ch!='\n'||r<row{target=i+1;}true});
    if down{d.drag_anchor=Some(target);d.anchor=None;}else{d.anchor=d.drag_anchor;}
    let changed=d.editor.cursor!=target;d.editor.cursor=target;down||changed
}
pub fn search_draw<F:Fn(&mut Surface,&str,Rect,u32)>(s:&Search,frame:&mut Surface,b:Rect,text:&F){
    frame.fill_rect(b.x+10,b.y+42,b.width.saturating_sub(20),140,0xFFF0F0F0);
    for (i,(label,value))in [("Find what:",&s.query),("Replace:",&s.replacement)].iter().enumerate(){if i==1&&!s.replace{continue;}let y=b.y+50+i as i32*32;text(frame,label,Rect::new(b.x+18,y,96,24),0xFF182534);frame.fill_rect(b.x+120,y,b.width.saturating_sub(140),26,0xFFFFFFFF);text(frame,value,Rect::new(b.x+125,y+3,b.width.saturating_sub(150),22),0xFF182534);}
    for (i,label) in ["Find next","Replace","Match case","Close"].iter().enumerate(){if i==1&&!s.replace{continue;}let r=search_buttons(b)[i];crate::compositor::aero::menu_item(frame,r,i==2&&s.case_sensitive);text(frame,label,Rect::new(r.x+5,r.y+4,r.width.saturating_sub(10),22),0xFF182534);}
    text(frame,&s.message,Rect::new(b.x+18,b.y+150,b.width.saturating_sub(36),22),0xFF974321);
}


pub fn search_buttons(b:Rect)->[Rect;4]{core::array::from_fn(|i|Rect::new(b.x+18+i as i32*112,b.y+118,106,28))}
pub fn search_click(s:&mut Search,d:&mut Document,b:Rect,x:i32,y:i32)->bool{
    for (i,r) in search_buttons(b).iter().enumerate(){if r.contains(x,y){match i{0=>s.key(d,13),1 if s.replace=>s.key(d,18),2=>s.case_sensitive=!s.case_sensitive,3=>return true,_=>{}}return false;}}
    if y>=b.y+50&&y<b.y+76{s.replacement_focus=false;}else if s.replace&&y>=b.y+82&&y<b.y+108{s.replacement_focus=true;}false
}

#[cfg(test)]
mod viewport_tests {
    use super::*;
    #[test]
    fn long_lines_scroll_and_clicks_use_visible_columns() {
        let mut d=Document::new("long.txt");d.insert("abcdefghijklmnopqrstuvwx");
        let b=Rect::new(0,0,116,180);
        let (_,left,cols)=viewport(&d,bounds(b));assert!(left>0);assert_eq!(left+cols-1,24);
        click(&mut d,b,8,42);assert_eq!(d.editor.cursor,left);
    }
    #[test]
    fn wrap_and_drag_select_character_indices() {
        let mut d=Document::new("wrap.txt");d.insert("abcd中efghijkl");d.word_wrap=true;
        let b=Rect::new(0,0,80,180);assert!(caret(&d,7).0>0);
        pointer(&mut d,b,8,42,true,true);pointer(&mut d,b,35,42,false,true);
        assert_eq!(d.selected_text(),"abc");pointer(&mut d,b,35,42,false,false);assert_eq!(d.drag_anchor,None);
        d.insert("x\n");click(&mut d,b,8,145);assert_eq!(d.editor.cursor,d.editor.text.chars().count());
    }
}
