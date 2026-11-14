//! Editable documents, clipboard operations and a conservative RTF writer.
use alloc::{format,string::String,vec::Vec};
use crate::widgets::textbox::TextBox;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Encoding { Utf8, Utf8Bom, Utf16Le, Utf16Be }
#[derive(Clone,Debug,PartialEq,Eq)]
struct Edit { text:String, cursor:usize, anchor:Option<usize> }
#[derive(Clone,Debug,PartialEq,Eq)]
pub struct Document {
    pub editor:TextBox,pub bold:bool,pub italic:bool,pub dirty:bool,pub filename:String,pub file_id:Option<u32>,
    pub encoding:Encoding,pub crlf:bool,pub anchor:Option<usize>,pub scroll:usize,pub word_wrap:bool,pub drag_anchor:Option<usize>,
    undo:Vec<Edit>,redo:Vec<Edit>,saved:String,
}
impl Document {
    pub fn new(filename:&str)->Self {
        let mut editor=TextBox::new(0);editor.focused=true;
        Self{editor,bold:false,italic:false,dirty:false,filename:filename.into(),file_id:None,
            encoding:Encoding::Utf8,crlf:true,anchor:None,scroll:0,word_wrap:false,drag_anchor:None,undo:Vec::new(),redo:Vec::new(),saved:String::new()}
    }
    fn snapshot(&self)->Edit{Edit{text:self.editor.text.clone(),cursor:self.editor.cursor,anchor:self.anchor}}
    fn checkpoint(&mut self){if self.undo.len()>=64{self.undo.remove(0);}self.undo.push(self.snapshot());self.redo.clear();}
    fn changed(&mut self){self.dirty=self.editor.text!=self.saved;}
    fn byte(&self,ch:usize)->usize{self.editor.text.char_indices().nth(ch).map_or(self.editor.text.len(),|(i,_)|i)}
    pub fn selection(&self)->Option<(usize,usize)>{let a=self.anchor?;let c=self.editor.cursor;(a!=c).then_some((a.min(c),a.max(c)))}
    pub fn selected_text(&self)->String {self.selection().map_or_else(String::new,|(a,b)|self.editor.text[self.byte(a)..self.byte(b)].into())}
    fn delete_selection(&mut self)->bool{
        let Some((a,b))=self.selection() else{self.anchor=None;return false;};
        self.editor.text.replace_range(self.byte(a)..self.byte(b),"");self.editor.cursor=a;self.anchor=None;true
    }
    pub fn select_all(&mut self){self.anchor=Some(0);self.editor.cursor=self.editor.text.chars().count();}
    pub fn cut(&mut self)->String{let text=self.selected_text();if !text.is_empty(){self.checkpoint();self.delete_selection();self.changed();}text}
    pub fn position(&self)->(usize,usize){let prefix=&self.editor.text[..self.byte(self.editor.cursor)];(prefix.bytes().filter(|b|*b==b'\n').count(),prefix.rsplit('\n').next().unwrap_or("").chars().count())}
    pub fn move_to(&mut self,line:usize,column:usize){let mut index=0;for(i,s)in self.editor.text.split('\n').enumerate(){if i==line{self.editor.cursor=index+column.min(s.chars().count());self.anchor=None;return;}index+=s.chars().count()+1;}self.editor.cursor=self.editor.text.chars().count();self.anchor=None;}
    pub fn key(&mut self,key:u8)->bool {
        use super::explorer::input;
        match key {
            input::LEFT=>{self.editor.move_cursor_left();self.anchor=None;return true;},
            input::RIGHT=>{self.editor.move_cursor_right();self.anchor=None;return true;},
            input::HOME|input::END|input::UP|input::DOWN|input::PAGE_UP|input::PAGE_DOWN=>{
                let (line,col)=self.position();match key{
                    input::HOME=>self.move_to(line,0),input::END=>self.move_to(line,usize::MAX),
                    input::UP=>self.move_to(line.saturating_sub(1),col),input::DOWN=>self.move_to(line+1,col),
                    input::PAGE_UP=>self.move_to(line.saturating_sub(12),col),_=>self.move_to(line+12,col)}return true;
            },
            1=>{self.select_all();return true;},
            8|9|10|13|input::DELETE|0x20..=0x7e=>{},_=>return false,
        }
        if key==8&&self.editor.cursor==0&&self.selection().is_none(){return false;}
        if key==input::DELETE&&self.editor.cursor==self.editor.text.chars().count()&&self.selection().is_none(){return false;}
        self.checkpoint();let selected=self.delete_selection();
        match key {
            8=>{if !selected{self.editor.backspace();}},
            input::DELETE=>{if !selected{let a=self.byte(self.editor.cursor);let b=self.byte(self.editor.cursor+1);self.editor.text.replace_range(a..b,"");}},
            10|13=>self.editor.insert('\n'),9=>self.editor.insert('\t'),_=>self.editor.insert(key as char),
        }self.changed();true
    }
    pub fn insert(&mut self,text:&str){self.checkpoint();self.delete_selection();let text=text.replace("\r\n","\n").replace('\r',"\n");let at=self.byte(self.editor.cursor);self.editor.text.insert_str(at,&text);self.editor.cursor+=text.chars().count();self.changed();}
    fn restore(&mut self,edit:Edit){self.editor.text=edit.text;self.editor.cursor=edit.cursor;self.anchor=edit.anchor;self.changed();}
    pub fn undo(&mut self){if let Some(edit)=self.undo.pop(){self.redo.push(self.snapshot());self.restore(edit);}}
    pub fn redo(&mut self){if let Some(edit)=self.redo.pop(){self.undo.push(self.snapshot());self.restore(edit);}}
    pub fn mark_saved(&mut self){self.saved=self.editor.text.clone();self.dirty=false;}
    pub fn encoded(&self)->Vec<u8>{
        let text=if self.crlf{self.editor.text.replace('\n',"\r\n")}else{self.editor.text.clone()};
        match self.encoding {
            Encoding::Utf8=>text.into_bytes(),Encoding::Utf8Bom=>{let mut out=alloc::vec![0xef,0xbb,0xbf];out.extend_from_slice(text.as_bytes());out},
            Encoding::Utf16Le|Encoding::Utf16Be=>{let le=self.encoding==Encoding::Utf16Le;let mut out=if le{alloc::vec![0xff,0xfe]}else{alloc::vec![0xfe,0xff]};for u in text.encode_utf16(){out.extend_from_slice(&if le{u.to_le_bytes()}else{u.to_be_bytes()});}out}
        }
    }
    /// Find from the caret, wrap once, and select the matched UTF-8 range.
    pub fn find(&mut self,needle:&str,case_sensitive:bool)->bool{
        if needle.is_empty(){return false;}
        let start=self.byte(self.editor.cursor);
        let found=if case_sensitive{self.editor.text[start..].find(needle).map(|p|p+start).or_else(||self.editor.text[..start].find(needle))}
        else{self.editor.text.char_indices().filter(|(p,_)|*p>=start).chain(self.editor.text.char_indices().filter(|(p,_)|*p<start))
            .find_map(|(p,_)|{let n=needle.chars().count();let tail=&self.editor.text[p..];let candidate:String=tail.chars().take(n).collect();(candidate.to_lowercase()==needle.to_lowercase()).then_some(p)})};
        let Some(p)=found else{return false;};let a=self.editor.text[..p].chars().count();self.anchor=Some(a);self.editor.cursor=a+needle.chars().count();true
    }
    pub fn replace_selected(&mut self,text:&str){if self.selection().is_some(){self.insert(text);}}
    pub fn rtf(&self)->Vec<u8>{let mut out=String::from("{\\rtf1\\ansi\\deff0{\\fonttbl{\\f0 Segoe UI;}}\\f0\\fs22 ");if self.bold{out.push_str("\\b ");}if self.italic{out.push_str("\\i ");}for ch in self.editor.text.chars(){match ch {'\\'|'{'|'}'=>{out.push('\\');out.push(ch);},'\n'=>out.push_str("\\par\n"),'\r'=>{},c if c.is_ascii()=>out.push(c),c=>{let mut units=[0;2];for unit in c.encode_utf16(&mut units){out.push_str(&format!("\\u{}?",*unit as i16));}}}}out.push('}');out.into_bytes()}
}
#[cfg(test)]mod tests{use super::*;#[test]fn rtf_escapes_syntax_and_utf16_and_undo_restores_text(){let mut d=Document::new("test.rtf");d.insert("A{\\}\n中😀");let r=String::from_utf8(d.rtf()).unwrap();assert!(r.contains("\\{\\\\\\}"));assert!(r.contains("\\u20013?"));assert!(r.contains("\\u-10179?\\u-8704?"));d.undo();assert_eq!(d.editor.text,"");}}

/// Decode a plain text document without silently corrupting UTF-16 files.
pub fn decode_text(bytes: &[u8]) -> Result<String, &'static str> {
    if let Some(bytes) = bytes.strip_prefix(&[0xff, 0xfe]) {
        return decode_utf16(bytes, true);
    }
    if let Some(bytes) = bytes.strip_prefix(&[0xfe, 0xff]) {
        return decode_utf16(bytes, false);
    }
    let bytes = bytes.strip_prefix(&[0xef,0xbb,0xbf]).unwrap_or(bytes);
    if bytes.contains(&0) { return Err("Binary files cannot be opened as text."); }
    String::from_utf8(bytes.to_vec()).map_err(|_| "Unsupported text encoding.")
}
fn decode_utf16(bytes: &[u8], little_endian: bool) -> Result<String, &'static str> {
    if bytes.len() % 2 != 0 { return Err("Truncated UTF-16 document."); }
    let units: Vec<_> = bytes.chunks_exact(2).map(|p| {
        if little_endian { u16::from_le_bytes([p[0],p[1]]) } else { u16::from_be_bytes([p[0],p[1]]) }
    }).collect();
    String::from_utf16(&units).map_err(|_| "Invalid UTF-16 document.")
}
impl Document {
    pub fn load(&mut self, bytes: &[u8], rich_text: bool) -> Result<(), &'static str> {
        let (text,bold,italic) = if rich_text { decode_rtf(bytes)? } else { (decode_text(bytes)?,false,false) };
        self.encoding=if bytes.starts_with(&[0xff,0xfe]){Encoding::Utf16Le}else if bytes.starts_with(&[0xfe,0xff]){Encoding::Utf16Be}else if bytes.starts_with(&[0xef,0xbb,0xbf]){Encoding::Utf8Bom}else{Encoding::Utf8};
        self.crlf=text.contains("\r\n")||!text.contains('\n');
        self.editor.text = text.replace("\r\n","\n").replace('\r',"\n"); self.editor.cursor = self.editor.text.chars().count();
        self.bold = bold; self.italic = italic; self.dirty = false; self.undo.clear();self.redo.clear();self.anchor=None;self.scroll=0;self.drag_anchor=None;self.mark_saved();
        Ok(())
    }
}

/// Conservative RTF text reader. Ignores metadata, pictures and ignorable
/// destinations. Formatting remains document-wide, matching this editor's
/// writer; embedded objects and mixed formatting are not executed or imported.
fn decode_rtf(bytes: &[u8]) -> Result<(String,bool,bool), &'static str> {
    if !bytes.starts_with(b"{\\rtf") { return Err("Not an RTF document."); }
    #[derive(Clone,Copy)] struct Group { skip: bool, uc: usize }
    let mut stack = Vec::new();
    let mut group = Group { skip: false, uc: 1 };
    let mut units = Vec::new();
    let (mut bold,mut italic,mut fallback,mut i) = (false,false,0usize,0);
    while i < bytes.len() {
        match bytes[i] {
            b'{' => { if stack.len() >= 128 { return Err("RTF nesting is too deep."); } stack.push(group); i += 1; }
            b'}' => { group = stack.pop().ok_or("Unbalanced RTF groups.")?; i += 1; }
            b'\\' => {
                i += 1;
                let Some(&symbol) = bytes.get(i) else { return Err("Truncated RTF control."); };
                if matches!(symbol,b'\\'|b'{'|b'}'|b'~'|b'-'|b'_') {
                    let value = match symbol { b'~'=>0xa0, b'-'=>0xad, b'_'=>0x2011, _=>symbol as u16 };
                    if fallback > 0 { fallback -= 1; } else if !group.skip { units.push(value); }
                    i += 1; continue;
                }
                if symbol == b'*' { group.skip = true; i += 1; continue; }
                if symbol == b'\'' {
                    let hex = bytes.get(i+1..i+3).ok_or("Truncated RTF byte.")?;
                    let hex = core::str::from_utf8(hex).map_err(|_|"Invalid RTF byte.")?;
                    let value = u8::from_str_radix(hex,16).map_err(|_|"Invalid RTF byte.")?;
                    if fallback > 0 { fallback -= 1; } else if !group.skip { units.push(cp1252(value)); }
                    i += 3; continue;
                }
                let start = i;
                while bytes.get(i).is_some_and(u8::is_ascii_alphabetic) { i += 1; }
                if start == i { i += 1; continue; }
                let word = &bytes[start..i];
                let number_start = i;
                if bytes.get(i) == Some(&b'-') { i += 1; }
                while bytes.get(i).is_some_and(u8::is_ascii_digit) { i += 1; }
                let number = if i > number_start {
                    Some(core::str::from_utf8(&bytes[number_start..i]).ok().and_then(|s|s.parse::<i32>().ok()).ok_or("Invalid RTF argument.")?)
                } else { None };
                if bytes.get(i) == Some(&b' ') { i += 1; }
                if matches!(word,b"fonttbl"|b"colortbl"|b"stylesheet"|b"info"|b"pict"|b"object"|b"header"|b"footer") { group.skip = true; }
                if word == b"bin" {
                    let length = usize::try_from(number.ok_or("Missing RTF binary length.")?).map_err(|_|"Invalid RTF binary length.")?;
                    i = i.checked_add(length).filter(|end|*end<=bytes.len()).ok_or("Truncated RTF binary data.")?;
                    continue;
                }
                if group.skip { continue; }
                match word {
                    b"uc" => group.uc = usize::try_from(number.unwrap_or(1)).ok().filter(|n|*n<=16).ok_or("Invalid Unicode fallback length.")?,
                    b"u" => { let n=number.ok_or("Missing Unicode value.")?; if !(-32768..=65535).contains(&n) {return Err("Invalid Unicode value.");} units.push(n as u16); fallback = group.uc; }
                    b"par"|b"line" => units.push(b'\n' as u16),
                    b"tab" => units.push(b'\t' as u16),
                    b"b" => bold = number != Some(0), b"i" => italic = number != Some(0),
                    _ => {}
                }
            }
            b'\r'|b'\n' => i += 1,
            byte => { if fallback > 0 { fallback -= 1; } else if !group.skip { units.push(cp1252(byte)); } i += 1; }
        }
    }
    if !stack.is_empty() { return Err("Unbalanced RTF groups."); }
    let text=String::from_utf16(&units).map_err(|_|"Invalid Unicode in RTF document.")?;
    Ok((text,bold,italic))
}
fn cp1252(byte:u8)->u16 {
    const HIGH:[u16;32]=[0x20ac,0x81,0x201a,0x192,0x201e,0x2026,0x2020,0x2021,0x2c6,0x2030,0x160,0x2039,0x152,0x8d,0x17d,0x8f,0x90,0x2018,0x2019,0x201c,0x201d,0x2022,0x2013,0x2014,0x2dc,0x2122,0x161,0x203a,0x153,0x9d,0x17e,0x178];
    if (0x80..0xa0).contains(&byte) {HIGH[(byte-0x80)as usize]} else {byte as u16}
}
#[cfg(test)] mod reading_tests {
    use super::*;
    #[test] fn saved_rtf_reopens_as_text_with_unicode_and_formatting() {
        let mut original=Document::new("test.rtf");original.insert("A{\\}\n中😀");original.bold=true;original.italic=true;
        let mut loaded=Document::new("test.rtf");loaded.load(&original.rtf(),true).unwrap();
        assert_eq!(loaded.editor.text,original.editor.text);assert!(loaded.bold&&loaded.italic);assert!(!loaded.dirty);
        let bytes=br"{\rtf1{\fonttbl{\f0 Arial;}}hello \'93quoted\'94\par world}";
        loaded.load(bytes,true).unwrap();assert_eq!(loaded.editor.text,"hello “quoted”\nworld");
    }
    #[test] fn malformed_documents_fail_without_destroying_current_text() {
        let mut d=Document::new("test.rtf");d.insert("keep");
        for bad in [br"{\rtf1 missing end".as_slice(),br"{\rtf1 \bin100 no}",br"{\rtf1 \u999999?}"] {
            assert!(d.load(bad,true).is_err());assert_eq!(d.editor.text,"keep");
        }
        assert_eq!(decode_text(&[0xff,0xfe,b'A',0]),Ok("A".into()));
        assert!(decode_text(&[0xff,0xfe,b'A']).is_err());assert!(decode_text(b"MZ\0binary").is_err());
    }
}

#[cfg(test)] mod editing_tests {
    use super::*;
    #[test]fn encoding_and_newlines_survive_edits_and_reload(){
        for encoding in [Encoding::Utf8,Encoding::Utf8Bom,Encoding::Utf16Le,Encoding::Utf16Be]{
            let mut d=Document::new("text.txt");d.encoding=encoding;d.insert("中😀\nsecond");let bytes=d.encoded();
            let mut reopened=Document::new("text.txt");reopened.load(&bytes,false).unwrap();assert_eq!(reopened.encoding,encoding);assert!(reopened.crlf);assert_eq!(reopened.editor.text,"中😀\nsecond");assert_eq!(reopened.encoded(),bytes);
            reopened.insert("!");reopened.undo();assert!(!reopened.dirty);reopened.redo();assert!(reopened.dirty);
        }
    }
    #[test]fn selection_find_replace_and_unicode_undo_are_char_based(){
        let mut d=Document::new("text.txt");d.insert("one 中😀 TWO one");d.move_to(0,0);assert!(d.find("中😀",true));assert_eq!(d.selected_text(),"中😀");d.replace_selected("三");assert_eq!(d.editor.text,"one 三 TWO one");d.undo();assert_eq!(d.editor.text,"one 中😀 TWO one");
        assert!(d.find("two",false));assert_eq!(d.selected_text(),"TWO");d.select_all();assert_eq!(d.cut(),"one 中😀 TWO one");assert_eq!(d.editor.text,"");d.undo();assert_eq!(d.editor.text,"one 中😀 TWO one");
    }
}
