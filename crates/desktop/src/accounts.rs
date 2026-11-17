//! Local SAM account client, shared by secure user tiles and Control Panel.
use alloc::{string::String,vec::Vec,rc::Rc};
use crate::{bmp,graphics::{Rect,Surface,blit::blit_nine_slice}};
use nt61_abi::desktop_bridge::STATUS_INVALID_PARAMETER;
#[derive(Clone,Debug,PartialEq,Eq)]
pub struct Account {pub rid:u32,pub name:String,pub enabled:bool,pub administrator:bool,pub picture:u32}
pub fn decode(bytes:&[u8])->Result<Vec<Account>,u32>{
    if bytes.len()<8||&bytes[..4]!=b"USR1"{return Err(STATUS_INVALID_PARAMETER);}
    let count=u32::from_le_bytes(bytes[4..8].try_into().unwrap())as usize;
    if count>1024{return Err(STATUS_INVALID_PARAMETER);}
    let mut cursor=8;let mut rows=Vec::new();
    for _ in 0..count {
        let header=bytes.get(cursor..cursor+16).ok_or(STATUS_INVALID_PARAMETER)?;
        let word=|i|u32::from_le_bytes(header[i..i+4].try_into().unwrap());
        let (rid,flags,picture,length)=(word(0),word(4),word(8),word(12)as usize);
        if length==0||length>64||picture>35||flags&!3!=0{return Err(STATUS_INVALID_PARAMETER);}
        cursor+=16;let name=core::str::from_utf8(bytes.get(cursor..cursor+length).ok_or(STATUS_INVALID_PARAMETER)?).map_err(|_|STATUS_INVALID_PARAMETER)?;
        cursor+=length;
        if rows.iter().any(|a:&Account|a.rid==rid||a.name.eq_ignore_ascii_case(name)){return Err(STATUS_INVALID_PARAMETER);}
        rows.push(Account{rid,name:name.into(),enabled:flags&1!=0,administrator:flags&2!=0,picture});
    }
    if cursor!=bytes.len(){return Err(STATUS_INVALID_PARAMETER);}Ok(rows)
}
/// Decoded once on entering account selection, retained across mouse redraws.
pub struct Artwork {frame:bmp::Image,default:bmp::Image,guest:bmp::Image,pictures:Vec<bmp::Image>,pub power:crate::logon_power::LogonPowerButtons}
impl Artwork {
    pub fn load()->Option<Rc<Self>> {
        Some(Rc::new(Self{frame:bmp::decode_bmp(include_bytes!("../assets/windows7/logon-buttons/user-tile.bmp")).ok()?,default:bmp::decode_bmp(crate::user_account_pictures::USER.bytes).ok()?,guest:bmp::decode_bmp(crate::user_account_pictures::GUEST.bytes).ok()?,pictures:crate::user_account_pictures::DEFAULT_PICTURES.iter().filter_map(|p|bmp::decode_bmp(p.bytes).ok()).collect(),power:crate::logon_power::LogonPowerButtons::load()?}))
    }
    pub fn portrait(&self,frame:&mut Surface,r:Rect,account:&Account,glass:bool){
        let picture=if account.picture==0{if account.rid==501{&self.guest}else{&self.default}}else{self.pictures.get(account.picture as usize-1).unwrap_or(&self.default)};
        // The original 190px glass tile has a centered 126px portrait.
        let inset=if glass {r.width*32/190}else{3};
        let target=Rect::new(r.x+inset as i32,r.y+inset as i32,r.width.saturating_sub(inset*2),r.height.saturating_sub(inset*2));
        blit_nine_slice(frame,&picture.pixels,(picture.width,picture.height),target,0);
        if glass {blit_nine_slice(frame,&self.frame.pixels,(self.frame.width,self.frame.height),r,8);}else{frame.stroke_rounded_rect(r,0,1,0xFFAAAAAA);}
    }
}
pub fn tile(w:u32,h:u32,index:usize,count:usize)->Rect {
    let columns=((w.saturating_sub(80)/150).max(1)as usize).min(count.max(1));
    let rows=count.div_ceil(columns);let row=index/columns;let in_row=(count-row*columns).min(columns);
    Rect::new(w as i32/2-(in_row as i32*150)/2+(index%columns)as i32*150,h as i32/2-(rows as i32*154)/2+row as i32*154,150,150)
}
/// Reserve room for the heading, branding and power controls at every size.
pub fn page_capacity(w:u32,h:u32)->usize {
    (w.saturating_sub(80)/150).max(1) as usize * (h.saturating_sub(260)/154).max(1) as usize
}
pub fn visible(w:u32,h:u32,count:usize,selected:usize)->core::ops::Range<usize> {
    let capacity=page_capacity(w,h);let start=selected.min(count.saturating_sub(1))/capacity*capacity;
    start..(start+capacity).min(count)
}
pub fn page_button(w:u32,h:u32,next:bool)->Rect {
    Rect::new(w as i32/2+if next{70}else{-170},h as i32-145,100,28)
}
#[cfg(test)]mod tests {
    use super::*;
    #[test]fn truncated_duplicate_and_oversized_accounts_are_rejected(){
        let mut b=b"USR1".to_vec();b.extend_from_slice(&1u32.to_le_bytes());
        for w in [500u32,3,0,13]{b.extend_from_slice(&w.to_le_bytes());}b.extend_from_slice(b"Administrator");
        assert_eq!(decode(&b).unwrap()[0].name,"Administrator");
        assert!(decode(&b[..b.len()-1]).is_err());let record=b[8..].to_vec();b[4..8].copy_from_slice(&2u32.to_le_bytes());b.extend(record);assert!(decode(&b).is_err());
    }
    #[test]fn gallery_wraps_without_overlap(){for count in 1..20 {for i in 0..count{let a=tile(1024,768,i,count);assert!(a.x>=0);assert!(a.x+a.width as i32<=1024);for j in 0..i{assert!(a.intersection(&tile(1024,768,j,count)).is_none());}}}}
    #[test]fn all_users_are_reachable_inside_the_visible_gallery(){
        for (w,h) in [(640,480),(800,600),(1024,768)] {for selected in 0..73 {
            let range=visible(w,h,73,selected);assert!(range.contains(&selected));
            for i in 0..range.len(){let r=tile(w,h,i,range.len());assert!(r.y>=80);assert!(r.y+r.height as i32<=h as i32-120);}
        }}
    }
}
