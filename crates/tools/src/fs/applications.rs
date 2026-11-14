//! Packages native application images and licensed icons. No fallback/success-stub executables.
#[allow(dead_code)]
#[path="../../../desktop/src/applications/catalog.rs"]
pub mod catalog;
use super::{build::SystemImage,elf_to_pe};
use std::path::Path;
const IDENTITY:&[u8]=b"NT61APP_ID_V1___";
fn u16_at(bytes:&[u8],offset:usize)->usize{u16::from_le_bytes(bytes[offset..offset+2].try_into().unwrap())as usize}
fn u32_at(bytes:&[u8],offset:usize)->usize{u32::from_le_bytes(bytes[offset..offset+4].try_into().unwrap())as usize}
fn put16(bytes:&mut[u8],offset:usize,value:u16){bytes[offset..offset+2].copy_from_slice(&value.to_le_bytes());}
fn put32(bytes:&mut[u8],offset:usize,value:u32){bytes[offset..offset+4].copy_from_slice(&value.to_le_bytes());}
fn align(value:usize,boundary:usize)->usize{value.div_ceil(boundary)*boundary}
fn wide(text:&str)->Vec<u8>{text.encode_utf16().chain(core::iter::once(0)).flat_map(u16::to_le_bytes).collect()}
fn version_block(key:&str,value:&[u8],value_length:u16,kind:u16,children:&[Vec<u8>])->Vec<u8>{let mut b=vec![0;6];put16(&mut b,2,value_length);put16(&mut b,4,kind);b.extend(wide(key));b.resize(align(b.len(),4),0);b.extend(value);for child in children{b.resize(align(b.len(),4),0);b.extend(child);}let len=b.len()as u16;put16(&mut b,0,len);b}
fn version(app:&catalog::Application)->Vec<u8>{let values=[("CompanyName","NT61 Open Source"),("FileDescription",app.title),("FileVersion","6.1.7601.0"),("ProductVersion","6.1.7601.0"),("ProductName","NT61 Desktop Applications"),("OriginalFilename",app.executable)];let children:Vec<_>=values.iter().map(|(k,v)|{let value=wide(v);version_block(k,&value,(value.len()/2)as u16,1,&[])}).collect();let table=version_block("040904B0",&[],0,1,&children);let strings=version_block("StringFileInfo",&[],0,1,&[table]);let translation=version_block("Translation",&[0x09,0x04,0xB0,0x04],4,0,&[]);let var=version_block("VarFileInfo",&[],0,1,&[translation]);let fixed:[u32;13]=[0xFEEF04BD,0x10000,0x00060001,7601<<16,0x00060001,7601<<16,0x3F,0,0x40004,1,0,0,0];let bytes:Vec<u8>=fixed.into_iter().flat_map(u32::to_le_bytes).collect();version_block("VS_VERSION_INFO",&bytes,52,0,&[strings,var])}
/// A numeric resource tree, one language per leaf; all DataRVAs refer to the final section RVA.
fn resources(entries:&[(u32,u32,Vec<u8>)],rva:u32)->Vec<u8>{let mut types=Vec::<u32>::new();for (kind,_,_)in entries{if !types.contains(kind){types.push(*kind);}}types.sort_unstable();let root=16+types.len()*8;let type_bytes:usize=types.iter().map(|kind|16+entries.iter().filter(|e|e.0==*kind).count()*8).sum();let language_start=root+type_bytes;let data_start=language_start+entries.len()*24;let payload_start=data_start+entries.len()*16;let mut b=vec![0;payload_start];put16(&mut b,14,types.len()as u16);let mut type_offset=root;let mut leaf=0;
    for(i,kind)in types.iter().enumerate(){put32(&mut b,16+i*8,*kind);put32(&mut b,20+i*8,type_offset as u32|0x80000000);let mut ids:Vec<_>=entries.iter().filter(|e|e.0==*kind).collect();ids.sort_by_key(|e|e.1);put16(&mut b,type_offset+14,ids.len()as u16);for(j,(_,id,data))in ids.iter().enumerate(){let lang=language_start+leaf*24;let entry=data_start+leaf*16;put32(&mut b,type_offset+16+j*8,*id);put32(&mut b,type_offset+20+j*8,lang as u32|0x80000000);put16(&mut b,lang+14,1);put32(&mut b,lang+16,1033);put32(&mut b,lang+20,entry as u32);let payload=align(b.len(),4);b.resize(payload,0);put32(&mut b,entry,rva+payload as u32);put32(&mut b,entry+4,data.len()as u32);b.extend_from_slice(data);leaf+=1;}type_offset+=16+ids.len()*8;
    }b}
fn append_resources(mut bytes:Vec<u8>,entries:&[(u32,u32,Vec<u8>)])->Vec<u8>{let header=u32_at(&bytes,0x3C);let count=u16_at(&bytes,header+6);let opt=header+24;let table=opt+u16_at(&bytes,header+20);let section=table+count*40;assert!(section+40<=u32_at(&bytes,opt+60),"PE header has no section slot");let rva=align(u32_at(&bytes,opt+56),4096);let raw=align(bytes.len(),u32_at(&bytes,opt+36));let data=resources(entries,rva as u32);let raw_size=align(data.len(),u32_at(&bytes,opt+36));bytes.resize(raw+raw_size,0);bytes[section..section+8].copy_from_slice(b".rsrc\0\0\0");put32(&mut bytes,section+8,data.len()as u32);put32(&mut bytes,section+12,rva as u32);put32(&mut bytes,section+16,raw_size as u32);put32(&mut bytes,section+20,raw as u32);put32(&mut bytes,section+36,0x40000040);put16(&mut bytes,header+6,(count+1)as u16);put32(&mut bytes,opt+56,align(rva+data.len(),4096)as u32);put32(&mut bytes,opt+112+16,rva as u32);put32(&mut bytes,opt+112+20,data.len()as u32);bytes[raw..raw+data.len()].copy_from_slice(&data);bytes}
fn group(icon_size:usize,icon_id:u16)->Vec<u8>{let mut b=vec![0,0,1,0,1,0,0,0,0,0,1,0,32,0];b.extend((icon_size as u32).to_le_bytes());b.extend(icon_id.to_le_bytes());b}
fn manifest(app:&catalog::Application)->Vec<u8>{format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0"><assemblyIdentity name="NT61.{}" version="6.1.7601.0" type="win32"/><trustInfo xmlns="urn:schemas-microsoft-com:asm.v3"><security><requestedPrivileges><requestedExecutionLevel level="asInvoker" uiAccess="false"/></requestedPrivileges></security></trustInfo><compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1"><application><supportedOS Id="{{35138b9a-5d96-4fbd-8e2d-a2440225f93a}}"/></application></compatibility></assembly>"#,app.executable.trim_end_matches(".exe")).into_bytes()}
pub fn package(root:&Path,arch:&str,target:&str)->Vec<SystemImage>{let elf_path=root.join("target").join(target).join("release/application-host");let elf=std::fs::read(&elf_path).unwrap_or_else(|e|panic!("{}: {e}",elf_path.display()));let base=elf_to_pe::elf_to_pe(&elf,arch).expect("native application ELF to PE");let mut images=Vec::new();for app in catalog::APPLICATIONS{
    let icon=std::fs::read(root.join("crates/desktop/assets/applications").join(format!("{}.png",app.icon))).expect("licensed application icon");
    // Explorer is the real session shell, built and installed separately.
    if app.install_path().is_none() || matches!(app.id,catalog::AppId::Explorer|catalog::AppId::CommandPrompt){continue;}
    let mut image=base.clone();let matches:Vec<_>=image.windows(IDENTITY.len()).enumerate().filter(|(_,v)|*v==IDENTITY).map(|(i,_)|i).collect();assert_eq!(matches.len(),1,"application identity must occur exactly once");let pos=matches[0]+16;put32(&mut image,pos,app.id as u32);let header=u32_at(&image,0x3C);put16(&mut image,header+24+68,app.pe_subsystem()); // Windows GUI or console, never EFI.
    let entries=vec![(3,1,icon.clone()),(14,1,group(icon.len(),1)),(16,1,version(app)),(24,1,manifest(app))];image=append_resources(image,&entries);
    images.push(SystemImage{name:app.executable,bytes:image});
}
    images
}
#[cfg(test)]mod tests{use super::*;#[test]fn version_and_resource_addresses_are_well_formed(){let app=&catalog::APPLICATIONS[0];let v=version(app);assert_eq!(u16_at(&v,0),v.len());assert!(v.windows(4).any(|p|p==0xFEEF04BDu32.to_le_bytes()));let data=resources(&[(3,1,vec![1,2,3]),(14,1,vec![4,5])],0x8000);assert_eq!(u16_at(&data,14),2);let type_dir=u32_at(&data,20)&0x7fffffff;let lang=u32_at(&data,type_dir+20)&0x7fffffff;let entry=u32_at(&data,lang+20);let payload=u32_at(&data,entry)-0x8000;assert_eq!(&data[payload..payload+3],&[1,2,3]);}}

/// Resolve filenames generated by this package to their original system locations.
pub fn destination(system32: &Path, name: &str) -> std::path::PathBuf {
    if let Some(path) = catalog::APPLICATIONS.iter()
        .find(|app| app.install_path().is_some() && app.executable == name)
        .and_then(|app| app.install_path()) {
        return system32.parent().and_then(Path::parent).expect("system root").join(path);
    }
    system32.join(name)
}

/// Remove artifacts from the previous package layout before collecting the tree.
pub fn clean_obsolete(system32: &Path) -> std::io::Result<()> {
    for name in ["nt61apps.dll", "nt61run.exe", "nt61games.exe", "GettingStarted.exe", "explorer.exe", "wordpad.exe", "Minesweeper.exe", "HelpPane.exe"] {
        let path = system32.join(name);
        if path.exists() { std::fs::remove_file(path)?; }
    }
    // Windows 7's built-in AHCI miniport is msahci; these were previously
    // copied from later-system/vendor naming or timer placeholders.
    for name in ["storahci.sys", "stornvme.sys", "iastor.sys", "hpet.sys"] {
        let path = system32.join("drivers").join(name);
        if path.is_file() { std::fs::remove_file(path)?; }
    }
    // Older builds installed shell test batches into the shipping volume.
    if let Some(root) = system32.parent().and_then(Path::parent) {
        for name in ["autoexec.bat", "tests/autoexec.bat", "system/tests/autoexec.bat"] {
            let path = root.join(name);
            if path.is_file() { std::fs::remove_file(path)?; }
        }
        for name in ["system/tests", "system", "tests"] {
            let path = root.join(name);
            if path.is_dir() && std::fs::read_dir(&path)?.next().is_none() {
                std::fs::remove_dir(path)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod layout_tests {
    use super::*;
    #[test]
    fn removes_obsolete_files_and_preserves_original_paths() {
        let temp = std::env::temp_dir().join(format!("nt61-app-layout-{}",std::process::id()));
        let system32 = temp.join("Windows/System32");
        std::fs::create_dir_all(&system32).unwrap();
        for name in ["nt61apps.dll","nt61run.exe","nt61games.exe","GettingStarted.exe","explorer.exe","wordpad.exe","Minesweeper.exe","HelpPane.exe","calc.exe"] {
            std::fs::write(system32.join(name),b"MZ").unwrap();
        }
        for path in ["autoexec.bat", "tests/autoexec.bat", "system/tests/autoexec.bat"] {
            let path = temp.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"test batch").unwrap();
        }
        std::fs::create_dir_all(system32.join("drivers")).unwrap();
        for name in ["storahci.sys", "stornvme.sys", "iastor.sys", "hpet.sys", "msahci.sys"] {
            std::fs::write(system32.join("drivers").join(name), b"MZ").unwrap();
        }
        clean_obsolete(&system32).unwrap();
        assert_eq!(std::fs::read_dir(system32.join("drivers")).unwrap().count(), 1);
        assert!(system32.join("drivers/msahci.sys").is_file());
        assert!(!temp.join("autoexec.bat").exists());
        assert!(!temp.join("tests").exists());
        assert!(!temp.join("system").exists());
        assert_eq!(std::fs::read_dir(&system32).unwrap().count(),2);
        assert!(system32.join("calc.exe").is_file());
        for (name,path) in [
            ("explorer.exe","Windows/explorer.exe"),
            ("wordpad.exe","Program Files/Windows NT/Accessories/wordpad.exe"),
            ("Minesweeper.exe","Program Files/Microsoft Games/Minesweeper/Minesweeper.exe"),
            ("HelpPane.exe","Windows/HelpPane.exe"),
            ("taskmgr.exe","Windows/System32/taskmgr.exe"),
        ] { assert_eq!(destination(&system32,name),temp.join(path)); }
        std::fs::remove_dir_all(temp).unwrap();
    }
}
