//! Read PE icon resources from the mounted System32 DLLs. This is a data-file
//! reader, not executable DLL loading or a claim of Win32 ABI compatibility.
use alloc::{vec, vec::Vec};
use crate::graphics::{Rect,Surface};
#[allow(dead_code)]
#[path="bin/explorer/png.rs"]
mod png;
#[allow(dead_code)]
#[path="bin/explorer/ico.rs"]
mod ico;
const INVALID:u32=0xc000000d;
fn u16_at(b:&[u8],at:usize)->Result<u16,u32>{Ok(u16::from_le_bytes(b.get(at..at+2).ok_or(INVALID)?.try_into().unwrap()))}
fn u32_at(b:&[u8],at:usize)->Result<u32,u32>{Ok(u32::from_le_bytes(b.get(at..at+4).ok_or(INVALID)?.try_into().unwrap()))}
struct Pe<'a,F>{read:&'a mut F,sections:Vec<(u32,u32,u32)>,root:u32,resource_size:u32}
impl<'a,F:FnMut(u64,usize)->Result<Vec<u8>,u32>> Pe<'a,F>{
    fn open(read:&'a mut F)->Result<Self,u32>{
        let dos=read(0,64)?;if dos.get(..2)!=Some(b"MZ"){return Err(INVALID);}
        let at=u32_at(&dos,60)? as u64;if at>1024*1024{return Err(INVALID);}
        let header=read(at,24)?;if header.get(..4)!=Some(b"PE\0\0"){return Err(INVALID);}
        let count=u16_at(&header,6)? as usize;let size=u16_at(&header,20)? as usize;
        if count==0||count>96||size>4096{return Err(INVALID);}
        let opt=read(at+24,size)?;
        let directory=match u16_at(&opt,0)?{0x20b=>112,0x10b=>96,_=>return Err(INVALID)};
        let root=u32_at(&opt,directory+16)?;let resource_size=u32_at(&opt,directory+20)?;
        let raw=read(at+24+size as u64,count*40)?;let mut sections=Vec::new();
        for section in raw.chunks_exact(40){sections.push((u32_at(section,12)?,u32_at(section,16)?,u32_at(section,20)?));}
        if root==0||resource_size<16{return Err(INVALID);}
        Ok(Self{read,sections,root,resource_size})
    }
    fn bytes(&mut self,rva:u32,length:usize)->Result<Vec<u8>,u32>{
        let (base,size,raw)=self.sections.iter().find(|(base,size,_)|rva>=*base&&(rva as u64+length as u64)<=*base as u64+*size as u64).copied().ok_or(INVALID)?;
        let _=size;
        let out=(self.read)(raw as u64+(rva-base)as u64,length)?;
        if out.len()!=length{return Err(INVALID);}Ok(out)
    }
    fn resource(&mut self,offset:u32,length:usize)->Result<Vec<u8>,u32>{
        if offset as u64+length as u64>self.resource_size as u64{return Err(INVALID);}
        self.bytes(self.root.checked_add(offset).ok_or(INVALID)?,length)
    }
    fn child(&mut self,offset:u32,id:Option<u16>,directory:bool)->Result<u32,u32>{
        let h=self.resource(offset,16)?;let count=u16_at(&h,12)? as usize+u16_at(&h,14)?as usize;
        if count>4096{return Err(INVALID);}
        let entries=self.resource(offset.checked_add(16).ok_or(INVALID)?,count*8)?;
        for e in entries.chunks_exact(8){let key=u32_at(e,0)?;let child=u32_at(e,4)?;
            if key&0x80000000!=0 {continue;}
            if id.is_none_or(|id|key==id as u32) && (child&0x80000000!=0)==directory{return Ok(child&0x7fffffff);}
        }Err(INVALID)
    }
    fn payload(&mut self,kind:u16,id:u16)->Result<Vec<u8>,u32>{
        let typ=self.child(0,Some(kind),true)?;let name=self.child(typ,Some(id),true)?;
        let leaf=self.child(name,None,false)?;let data=self.resource(leaf,16)?;
        let size=u32_at(&data,4)?as usize;if size>65536{return Err(INVALID);}
        self.bytes(u32_at(&data,0)?,size)
    }
    fn icon(&mut self,id:u16,preferred:u32)->Result<png::Image,u32>{
        let group=self.payload(14,id)?;
        if u16_at(&group,0)?!=0||u16_at(&group,2)?!=1{return Err(INVALID);}
        let count=u16_at(&group,4)? as usize;if count==0||count>256{return Err(INVALID);}
        let mut candidates=Vec::new();
        for i in 0..count{let at=6+i*14;let e=group.get(at..at+14).ok_or(INVALID)?;
            let w=if e[0]==0{256}else{e[0]as u32};let h=if e[1]==0{256}else{e[1]as u32};
            if w!=h{continue;}
            candidates.push((w.abs_diff(preferred),u16_at(e,6)?,at));
        }
        candidates.sort_by_key(|(distance,bits,_)|(*distance,core::cmp::Reverse(*bits)));
        for (_,_,at) in candidates{
            let e=&group[at..at+14];let payload=self.payload(3,u16_at(e,12)?)?;
            if payload.len()!=u32_at(e,8)?as usize{continue;}
            let mut ico=vec![0,0,1,0,1,0];ico.extend_from_slice(&e[..12]);ico.extend_from_slice(&22u32.to_le_bytes());ico.extend_from_slice(&payload);
            if let Ok(image)=ico::decode_ico_preferred(&ico,preferred){return Ok(image);}
        }Err(INVALID)
    }
}
#[derive(Clone,Debug,PartialEq,Eq)]
pub struct ShellIcons{folder:png::Image,document:png::Image,computer:png::Image}
impl ShellIcons{
    pub fn load(mut read:impl FnMut(&str,u64,usize)->Result<Vec<u8>,u32>)->Result<Self,u32>{
        let mut shell=|offset,length|read("C:\\Windows\\System32\\shell32.dll",offset,length);
        let mut pe=Pe::open(&mut shell)?;
        let folder=pe.icon(4,16)?;let document=pe.icon(2,16)?;
        let mut image=|offset,length|read("C:\\Windows\\System32\\imageres.dll",offset,length);
        let computer=Pe::open(&mut image)?.icon(109,16)?;
        Ok(Self{folder,document,computer})
    }
    pub fn draw(&self,frame:&mut Surface,directory:bool,b:Rect){
        let image=if directory{&self.folder}else{&self.document};
        draw(image,frame,b)
    }
    pub fn draw_computer(&self,frame:&mut Surface,b:Rect){draw(&self.computer,frame,b)}
}
fn draw(image:&png::Image,frame:&mut Surface,b:Rect){
    for y in 0..b.height{for x in 0..b.width{
        let sx=x*image.width/b.width;let sy=y*image.height/b.height;
        let at=((sy*image.width+sx)*4)as usize;let p=&image.pixels[at..at+4];
        frame.blend_pixel((b.x+x as i32)as u32,(b.y+y as i32)as u32,(p[3]as u32)<<24|(p[0]as u32)<<16|(p[1]as u32)<<8|p[2]as u32);
    }}
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]fn reads_actual_generated_dlls_with_bounded_io(){
        use std::{fs,io::{Read,Seek,SeekFrom}};
        let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../build/x86_64/ntfs/system/Windows/System32");
        // Resource generator is tested independently; this also checks the staged
        // on-disk DLLs used by QEMU when they are present.
        if !root.join("shell32.dll").exists(){return;}
        let mut total=0;
        let icons=ShellIcons::load(|path,offset,length|{
            assert!(length<=65536);total+=length;
            let mut file=fs::File::open(root.join(path.rsplit('\\').next().unwrap())).map_err(|_|INVALID)?;
            file.seek(SeekFrom::Start(offset)).map_err(|_|INVALID)?;
            let mut bytes=vec![0;length];file.read_exact(&mut bytes).map_err(|_|INVALID)?;Ok(bytes)
        }).unwrap();
        assert_eq!((icons.folder.width,icons.document.width,icons.computer.width),(16,16,16));
        assert!(total<256*1024,"must not load entire DLLs ({total} bytes)");
        let expected=ico::decode_ico_preferred(include_bytes!("../assets/windows7/icons/shell32/ICON4_1.ico"),16).unwrap();
        assert_eq!(icons.folder.pixels,expected.pixels);
    }
    #[test]fn malformed_pe_is_rejected(){assert!(ShellIcons::load(|_,_,n|Ok(vec![0;n])).is_err());}
}
