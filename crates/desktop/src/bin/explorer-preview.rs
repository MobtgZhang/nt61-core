//! Host pixel oracle for Explorer integration checks; never installed in the image.
use nt61_desktop::{applications::explorer::{self,Explorer},file::FileSystem,graphics::{Rect,Surface,font::FontLib}};
fn main(){
    let args:Vec<_>=std::env::args().collect();
    assert!(matches!(args.len(),5|6),"explorer-preview <width> <height> <empty-folder-name-or-C-path> <output.ppm> [query]");
    let width:u32=args[1].parse().unwrap();let height:u32=args[2].parse().unwrap();
    let mut files=if args[3].starts_with("C:"){FileSystem::mounted(nt61_desktop::file::Backend(staged)).unwrap()}else{
        let mut files=FileSystem::new();let users=files.mkdir(1,"Users");let admin=files.mkdir(users,"Administrator");
        for folder in ["Desktop","Downloads","Documents","Music","Pictures","Videos"]{files.mkdir(admin,folder);}
        files.mkdir(1,&args[3]);files
    };
    let directory=files.resolve(&args[3]).unwrap();let mut state=Explorer::new(1,1);
    state.prepare(&mut files);
    // Load the same staged DLL resources for memory-only pixel oracles too.
    if files.shell_icons.is_none(){
        files.shell_icons=Some(std::sync::Arc::new(nt61_desktop::shell_resources::ShellIcons::load(read_staged).unwrap()));
    }
    state.navigate(&mut files,directory);
    if let Some(query)=args.get(5){for byte in std::iter::once(6).chain(query.bytes()){state.key(&mut files,byte);}while state.work(&mut files){}state.key(&mut files,13);}
    let font=FontLib::ui().unwrap();let size=nt61_desktop::shell::layout::UI_FONT_SIZE;
    let mut frame=Surface::new(width,height);
    explorer::draw(&state,&files,&mut frame,Rect::new(0,0,width,height),|frame,text,b,color|{
        let shortened;
        let value=if font.measure(text,size).is_some_and(|m|m.width<=b.width){text}else{
            let dots=font.measure("...",size).unwrap().width;if dots>b.width{return;}
            let Some(prefix)=font.fitting_prefix(text,size,b.width-dots)else{return;};shortened=format!("{prefix}...");&shortened
        };
        let _=font.draw_text(frame,value,b.x,b.y,size,color);
    });
    let mut bytes=format!("P6\n{width} {height}\n255\n").into_bytes();
    for pixel in frame.pixels(){bytes.extend([(pixel>>16) as u8,(pixel>>8) as u8,*pixel as u8]);}
    std::fs::write(&args[4],bytes).unwrap();
}

fn staged_path(path:&str)->std::path::PathBuf{
    let root=std::env::var_os("NT61_PREVIEW_SYSTEM").map(std::path::PathBuf::from)
        .unwrap_or_else(||std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../build/x86_64/ntfs/system"));
    root.join(path.trim_start_matches("C:").trim_start_matches('\\').replace('\\',"/"))
}
fn read_staged(path:&str,offset:u64,length:usize)->Result<Vec<u8>,u32>{
    use std::io::{Read,Seek,SeekFrom};
    let mut file=std::fs::File::open(staged_path(path)).map_err(|_|0xc0000034u32)?;
    file.seek(SeekFrom::Start(offset)).map_err(|_|0xc0000001u32)?;
    let mut out=vec![0;length];file.read_exact(&mut out).map_err(|_|0xc0000001u32)?;Ok(out)
}
fn staged(operation:u32,path:&str,input:&[u8])->Result<Vec<u8>,u32>{
    use nt61_abi::desktop_bridge as abi;
    match operation{
        abi::LIST=>{
            let mut out=Vec::new();
            for entry in std::fs::read_dir(staged_path(path)).map_err(|_|abi::STATUS_NOT_FOUND)?{
                let entry=entry.map_err(|_|abi::STATUS_NOT_FOUND)?;let name=entry.file_name().to_string_lossy().into_owned();
                if name.starts_with('$'){continue;}
                let metadata=entry.metadata().map_err(|_|abi::STATUS_NOT_FOUND)?;
                out.extend_from_slice(&(name.len()as u32).to_le_bytes());
                out.extend_from_slice(&(if metadata.is_dir(){abi::DIRECTORY}else{0}).to_le_bytes());
                out.extend_from_slice(&(if metadata.is_dir(){0}else{metadata.len()}).to_le_bytes());out.extend_from_slice(name.as_bytes());
            }Ok(out)
        },
        abi::READ_AT if input.len()==12=>read_staged(path,u64::from_le_bytes(input[..8].try_into().unwrap()),u32::from_le_bytes(input[8..].try_into().unwrap())as usize),
        _=>Err(abi::STATUS_NOT_SUPPORTED),
    }
}
