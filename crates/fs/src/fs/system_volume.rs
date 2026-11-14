//! System-volume operations for the desktop and other VFS clients.
//!
//! Reads dispatch through the mounted filesystem, never through a synthetic
//! directory tree. Writes affect the kernel's system-volume mirror. Flushing
//! that mirror to the host block device is a separate storage-layer operation.
use super::{fat32, ntfs, FsType};
use alloc::{string::String, vec::Vec};
use core::sync::atomic::{AtomicBool, Ordering};

const INVALID: u32 = 0xC000_000D;
const NOT_FOUND: u32 = 0xC000_0034;
const UNSUPPORTED: u32 = 0xC000_00BB;
const IO_ERROR: u32 = 0xC000_0001;
static BUSY: AtomicBool = AtomicBool::new(false);

struct VolumeGuard(Option<*const u8>);
impl VolumeGuard {
    fn acquire() -> Result<Self, u32> {
        BUSY.compare_exchange(false,true,Ordering::Acquire,Ordering::Relaxed).map_err(|_|IO_ERROR)?;
        let previous = super::active_partition_ramdisk();
        if let Some(base) = super::sys_mirror_address() { super::set_active_partition_ramdisk(Some(base)); }
        Ok(Self(previous))
    }
}
impl Drop for VolumeGuard {
    fn drop(&mut self) {
        super::set_active_partition_ramdisk(self.0);
        BUSY.store(false,Ordering::Release);
    }
}

#[derive(Debug)]
pub struct Entry { pub name: String, pub directory: bool, pub size: u64 }

/// Canonical absolute system-volume path. Reject other drives and traversal
/// above the root before a filesystem-specific resolver sees the input.
pub fn normalize(path: &str) -> Result<String,u32> {
    if path.contains('\0') || path.len()>1024 { return Err(INVALID); }
    let path=path.replace('\\',"/");
    let path=if let Some((drive,rest))=path.split_once(':') {
        if !drive.eq_ignore_ascii_case("C") { return Err(NOT_FOUND); }
        rest
    } else {path.as_str()};
    let mut parts=Vec::new();
    for part in path.split('/') {
        match part {""|"."=>{},".."=>{if parts.pop().is_none(){return Err(INVALID);}},_=>parts.push(part)}
    }
    Ok(alloc::format!("C:\\{}",parts.join("\\")))
}
fn utf16(path:&str)->Vec<u16>{path.encode_utf16().chain(core::iter::once(0)).collect()}
fn fat_directory(fs:&fat32::Fat32FileSystem,path:&str)->Result<u32,u32>{
    if path=="C:\\" {return Ok(fs.fat_data.root_cluster);}
    let entry=fat32::find_file_at_path(fs,path).filter(|entry|entry.is_directory()).ok_or(NOT_FOUND)?;
    Ok(entry.first_cluster())
}
fn parent_leaf(path:&str)->Result<(&str,&str),u32>{
    let (parent,leaf)=path.rsplit_once('\\').ok_or(INVALID)?;
    if leaf.is_empty() || leaf.encode_utf16().count()>255
        || leaf.chars().any(|c| c<' ' || matches!(c, ':'|'*'|'?'|'"'|'<'|'>'|'|'))
        || leaf.ends_with(['.',' ']) {return Err(INVALID);}
    // The shared Win32 path boundary must enforce these rules for CMD too,
    // rather than relying on Explorer's rename textbox to reject them.
    let stem=leaf.split('.').next().unwrap_or("").trim_end_matches(' ');
    let reserved=["CON","PRN","AUX","NUL"].iter().any(|v|stem.eq_ignore_ascii_case(v));
    let numbered_device=stem.get(..3).is_some_and(|prefix|prefix.eq_ignore_ascii_case("COM")||prefix.eq_ignore_ascii_case("LPT"))
        && matches!(stem.get(3..),Some("1"|"2"|"3"|"4"|"5"|"6"|"7"|"8"|"9"|"¹"|"²"|"³"));
    if reserved||numbered_device{return Err(INVALID);}
    Ok((if parent=="C:" {"C:\\"}else{parent},leaf))
}

pub fn list(path:&str)->Result<Vec<Entry>,u32>{
    let path=normalize(path)?;
    let _guard=VolumeGuard::acquire()?;
    let entries=match super::detect_system_partition_type(){
        FsType::Ntfs=>{
            let fs=ntfs::get_mounted_fs().ok_or(NOT_FOUND)?;
            let record=if path=="C:\\"{5}else{
                let handle=ntfs::open_file(fs,&utf16(&path),None).filter(|h|h.is_directory).ok_or(NOT_FOUND)?;
                handle.mft_record
            };
            ntfs::list_directory_checked(fs,record).map_err(|e|e as u32)?.into_iter().map(|entry|Entry{name:String::from_utf16_lossy(&entry.name),directory:entry.is_directory,size:entry.file_size}).collect()
        },
        FsType::Fat32=>{
            let fs=fat32::get_mounted_fs().ok_or(NOT_FOUND)?;
            fat32::directory_entries(fs,fat_directory(fs,&path)?).map_err(|e|e as u32)?.into_iter()
                .map(|entry|Entry{name:entry.name,directory:entry.raw.is_directory(),size:entry.raw.file_size()as u64}).collect()
        },
        FsType::Ext2|FsType::Ext3|FsType::Ext4=>{
            super::ext2::extended_api::list_directory(&path).map_err(|e|e as u32)?.into_iter()
                .map(|entry|Entry{name:entry.name,directory:entry.is_directory,size:entry.size}).collect()
        },
        FsType::Refs=>list_refs(&path)?,
        _=>return Err(UNSUPPORTED),
    };
    Ok(entries.into_iter().filter(|entry| !matches!(entry.name.as_str(),"."|"..") && !entry.name.starts_with('$')).collect())
}

fn list_refs(path:&str)->Result<Vec<Entry>,u32>{
    let mut header=[0u8;512];
    if super::sys_ramdisk_read(0x1000,&mut header)!=512 {return Err(IO_ERROR);}
    let size=u64::from_le_bytes(header[88..96].try_into().map_err(|_|IO_ERROR)?)as usize;
    if !(272..=16*1024*1024).contains(&size){return Err(IO_ERROR);}
    let mut table=alloc::vec![0;size];
    if super::sys_ramdisk_read(0x2000,&mut table)!=size{return Err(IO_ERROR);}
    let normalized=path[3..].replace('\\',"/");
    let prefix=if normalized.is_empty(){String::new()}else{alloc::format!("{normalized}/")};
    let mut result:Vec<Entry>=Vec::new();
    for record in table.chunks_exact(272){
        if record[0]==0{break;}
        let n=record[..256].iter().position(|b|*b==0).unwrap_or(256);
        let name=core::str::from_utf8(&record[..n]).map_err(|_|IO_ERROR)?;
        if name.len()<prefix.len() || !name.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes()){continue;}
        let suffix=&name[prefix.len()..];
        if suffix.is_empty(){continue;}
        let (leaf,directory)=suffix.split_once('/').map(|(leaf,_)|(leaf,true)).unwrap_or((suffix,false));
        if result.iter().any(|entry|entry.name.eq_ignore_ascii_case(leaf)){continue;}
        result.push(Entry{name:leaf.into(),directory,size:if directory{0}else{u64::from_le_bytes(record[264..272].try_into().map_err(|_|IO_ERROR)?)}});
    }
    if result.is_empty() && path!="C:\\" {return Err(NOT_FOUND);}
    Ok(result)
}

pub fn read(path:&str)->Result<Vec<u8>,u32>{
    let path=normalize(path)?;
    let _guard=VolumeGuard::acquire()?;
    super::read_file_from_disk_limited(&path,4 * 1024 * 1024).ok_or(NOT_FOUND)
}

/// Read a bounded range from the mounted volume, including RAM-backed images.
pub fn read_at(path:&str,offset:u64,length:usize)->Result<Vec<u8>,u32>{
    if length>65536{return Err(INVALID);}
    let path=normalize(path)?;
    let _guard=VolumeGuard::acquire()?;
    let mut bytes=alloc::vec![0;length];
    if length==0{return Ok(bytes);}
    let count=match super::detect_system_partition_type(){
        FsType::Ntfs=>super::read_ntfs_file_at(&path,offset,&mut bytes).ok_or(NOT_FOUND)?,
        FsType::Fat32=>{
            let fs=fat32::get_mounted_fs().ok_or(NOT_FOUND)?;
            let entry=fat32::find_file_at_path(fs,&path).filter(|e|!e.is_directory()).ok_or(NOT_FOUND)?;
            fat32::read_file_at(fs,entry.first_cluster(),entry.file_size() as u64,offset,&mut bytes).map_err(|e|e as u32)?
        },
        FsType::Ext2|FsType::Ext3|FsType::Ext4=>{
            let fs=super::ext2::get_mounted_fs().ok_or(NOT_FOUND)?;
            // SAFETY: mounted fs and the output slice are live for this call.
            let mut handle=unsafe{super::ext2::open_file(fs,path.as_bytes())}.ok_or(NOT_FOUND)?;
            handle.current_position=offset;
            unsafe{super::ext2::read_file(fs,&mut handle,&mut bytes)}
        },
        FsType::Refs=>{
            let mut header=[0;512];
            if super::sys_ramdisk_read(0x1000,&mut header)!=512{return Err(IO_ERROR);}
            let size=u64::from_le_bytes(header[88..96].try_into().unwrap())as usize;
            if !(272..=16*1024*1024).contains(&size){return Err(IO_ERROR);}
            let mut table=alloc::vec![0;size];
            if super::sys_ramdisk_read(0x2000,&mut table)!=size{return Err(IO_ERROR);}
            let name=path[3..].replace('\\',"/");
            let entry=table.chunks_exact(272).find(|e|{let n=e[..256].iter().position(|b|*b==0).unwrap_or(256);e[..n].eq_ignore_ascii_case(name.as_bytes())}).ok_or(NOT_FOUND)?;
            let size=u64::from_le_bytes(entry[264..272].try_into().unwrap());
            let count=length.min(size.saturating_sub(offset)as usize);
            let start=u64::from_le_bytes(entry[256..264].try_into().unwrap()).checked_mul(512).and_then(|s|s.checked_add(offset)).ok_or(INVALID)?;
            if count!=0 && super::sys_ramdisk_read(start,&mut bytes[..count])!=count{return Err(IO_ERROR);}count
        },
        _=>return Err(UNSUPPORTED),
    };
    bytes.truncate(count);Ok(bytes)
}

pub fn rename(source:&str,destination:&str)->Result<(),u32>{
    let source=normalize(source)?;let destination=normalize(destination)?;
    parent_leaf(&source)?;parent_leaf(&destination)?;
    let _guard=VolumeGuard::acquire()?;
    match super::detect_system_partition_type(){
        FsType::Ntfs=>ntfs::rename(ntfs::get_mounted_fs().ok_or(NOT_FOUND)?,&utf16(&source),&utf16(&destination)).map_err(|e|e as u32),
        FsType::Fat32=>{
            let fs=fat32::get_mounted_fs().ok_or(NOT_FOUND)?;
            let (old_parent,old)=parent_leaf(&source)?;let (new_parent,new)=parent_leaf(&destination)?;
            if !old_parent.eq_ignore_ascii_case(new_parent){return Err(UNSUPPORTED);}
            fat32::rename_file_in_dir_long(fs,fat_directory(fs,old_parent)?,old.as_bytes(),new.as_bytes()).map_err(|e|e as u32)
        },
        _=>Err(UNSUPPORTED),
    }
}

/// Create-new is checked and performed while holding the same volume guard.
pub fn create(path:&str,data:&[u8])->Result<(),u32>{
    let path=normalize(path)?;parent_leaf(&path)?;
    let _guard=VolumeGuard::acquire()?;
    match super::detect_system_partition_type(){
        FsType::Ntfs=>{
            let fs=ntfs::get_mounted_fs().ok_or(NOT_FOUND)?;
            ntfs::create_file(fs,&utf16(&path),&[]).map_err(|e|e as u32)?;
            if data.is_empty(){return Ok(());}
            if let Err(error)=ntfs::replace_file_contents(fs,&utf16(&path),data){
                let _=ntfs::delete_file(fs,&utf16(&path));return Err(error as u32);
            }
            Ok(())
        },
        FsType::Fat32=>{
            let fs=fat32::get_mounted_fs().ok_or(NOT_FOUND)?;
            let (parent,leaf)=parent_leaf(&path)?;let parent=fat_directory(fs,parent)?;
            if fat32::directory_entries(fs,parent).map_err(|e|e as u32)?.iter().any(|e|e.name.eq_ignore_ascii_case(leaf)){return Err(0xc0000035);}
            let cluster=if data.is_empty(){0}else{fat32::allocate_cluster(fs,0).map_err(|e|e as u32)?};
            if !data.is_empty(){
                match fat32::write_file(fs,cluster,0,data){
                    Ok(n)if n==data.len()=>{},
                    result=>{let _=fat32::free_cluster_chain(fs,cluster);return Err(result.err().map(|e|e as u32).unwrap_or(IO_ERROR));}
                }
            }
            match fat32::create_file_in_dir_long(fs,parent,leaf.as_bytes(),cluster,data.len()as u32){
                Ok(())=>Ok(()),Err(e)=>{let _=fat32::free_cluster_chain(fs,cluster);Err(e as u32)}
            }
        },
        _=>Err(UNSUPPORTED),
    }
}

pub fn write(path:&str,data:&[u8])->Result<(),u32>{
    let path=normalize(path)?;
    parent_leaf(&path)?;

    let _guard=VolumeGuard::acquire()?;
    match super::detect_system_partition_type(){
        FsType::Ntfs=>ntfs::replace_file_contents(ntfs::get_mounted_fs().ok_or(NOT_FOUND)?,&utf16(&path),data).map_err(|e|e as u32),
        FsType::Fat32=>{
            let fs=fat32::get_mounted_fs().ok_or(NOT_FOUND)?;
            let (parent,leaf)=parent_leaf(&path)?;
            let parent=fat_directory(fs,parent)?;
            let existing=fat32::directory_entries(fs,parent).map_err(|e|e as u32)?.into_iter().find(|entry|entry.name.eq_ignore_ascii_case(leaf));
            if existing.as_ref().is_some_and(|entry|entry.raw.is_directory()){return Err(INVALID);}
            if data.len()>u32::MAX as usize{return Err(INVALID);}
            // Publish the new size and chain only after all bytes are written.
            // This also releases the tail when a save shrinks or empties a file.
            let cluster=if data.is_empty(){0}else{fat32::allocate_cluster(fs,0).map_err(|e|e as u32)?};
            if !data.is_empty(){
                match fat32::write_file(fs,cluster,0,data){
                    Ok(n)if n==data.len()=>{},
                    result=>{let _=fat32::free_cluster_chain(fs,cluster);return Err(result.err().map(|e|e as u32).unwrap_or(IO_ERROR));}
                }
            }
            if let Some(mut entry)=existing{
                let old=entry.raw.first_cluster();
                entry.raw.first_cluster_high=(cluster>>16)as u16;entry.raw.first_cluster_low=cluster as u16;entry.raw.file_size=data.len()as u32;
                fat32::update_dir_entry(fs,entry.directory_cluster,entry.byte_offset,&entry.raw).map_err(|e|e as u32)?;
                fat32::free_cluster_chain(fs,old).map_err(|e|e as u32)
            }else{
                let result=fat32::create_file_in_dir_long(fs,parent,leaf.as_bytes(),cluster,data.len()as u32).map_err(|e|e as u32);
                if result.is_err(){let _=fat32::free_cluster_chain(fs,cluster);}result
            }
        },
        _=>Err(UNSUPPORTED),
    }
}

pub fn mkdir(path:&str)->Result<(),u32>{
    let path=normalize(path)?; let (parent,leaf)=parent_leaf(&path)?;
    let _guard=VolumeGuard::acquire()?;
    match super::detect_system_partition_type(){
        FsType::Ntfs=>ntfs::create_directory(ntfs::get_mounted_fs().ok_or(NOT_FOUND)?,&utf16(&path)).map(|_|()).map_err(|e|e as u32),
        FsType::Fat32=>{
            let fs=fat32::get_mounted_fs().ok_or(NOT_FOUND)?;
            let parent=fat_directory(fs,parent)?;
            if fat32::directory_entries(fs,parent).map_err(|e|e as u32)?.iter().any(|e|e.name.eq_ignore_ascii_case(leaf)){return Err(0xc0000035);}
            let cluster=fat32::allocate_cluster(fs,0).map_err(|e|e as u32)?;
            let mut bytes=alloc::vec![0;fs.base.cluster_size as usize];
            for (offset,name,first) in [(0,".",cluster),(32,"..",parent)]{
                bytes[offset..offset+11].fill(b' ');bytes[offset..offset+name.len()].copy_from_slice(name.as_bytes());bytes[offset+11]=0x10;
                bytes[offset+20..offset+22].copy_from_slice(&((first>>16)as u16).to_le_bytes());bytes[offset+26..offset+28].copy_from_slice(&(first as u16).to_le_bytes());
            }
            let result=fat32::write_cluster(fs,cluster,&bytes).and_then(|_|fat32::create_dir_in_dir_long(fs,parent,leaf.as_bytes(),cluster)).map_err(|e|e as u32);
            if result.is_err(){let _=fat32::free_cluster_chain(fs,cluster);}result
        },
        _=>Err(UNSUPPORTED),
    }
}

pub fn remove(path:&str)->Result<(),u32>{
    let path=normalize(path)?;parent_leaf(&path)?;
    let _guard=VolumeGuard::acquire()?;
    match super::detect_system_partition_type(){
        FsType::Ntfs=>{
            let fs=ntfs::get_mounted_fs().ok_or(NOT_FOUND)?;
            let path=utf16(&path);let handle=ntfs::open_file(fs,&path,None).ok_or(NOT_FOUND)?;
            if handle.is_directory{ntfs::delete_directory(fs,&path)}else{ntfs::delete_file(fs,&path)}.map_err(|e|e as u32)
        },
        FsType::Fat32=>{
            let fs=fat32::get_mounted_fs().ok_or(NOT_FOUND)?;
            let (parent,leaf)=parent_leaf(&path)?;let parent=fat_directory(fs,parent)?;
            fat32::delete_file_in_dir_long(fs,parent,leaf.as_bytes()).map_err(|e|e as u32)
        },
        _=>Err(UNSUPPORTED),
    }
}
