//! Bounded printer DCs and session PostScript jobs for the virtual printer.
use super::*;
use crate::arch::common::btl::dialog_wire as w;
use core::fmt::Write as _;
struct Dc {
    handle: u64,
    document: Option<String>,
    page: bool,
    pages: u32,
    color: u32,
    x: i32,
    y: i32,
    settings:Settings,
}
pub(super) struct State {
    dcs: Vec<Dc>,
    next: u64,
}
impl State {
    pub fn new() -> Self {
        Self {
            dcs: Vec::new(),
            next: 0x50000000,
        }
    }
}
#[derive(Clone,Copy)]
pub(super) struct Settings{pub orientation:u16,pub paper:u16,pub copies:u16,pub collate:bool}
impl Settings{fn new()->Self{Self{orientation:1,paper:9,copies:1,collate:false}}fn mm(self)->(f32,f32){let(mut w,mut h)=if self.paper==1{(215.9,279.4)}else{(210.0,297.0)};if self.orientation==2{core::mem::swap(&mut w,&mut h);}(w,h)}}
fn read_settings(image:&mut GuestImage,p:u64,wide:bool)->Result<Settings,u32>{
    let mut result=Settings::new();if p==0{return Ok(result);}
    let size=if wide{220}else{156};let offset=if wide{64}else{32};
    if !image.win32.heap_allocations.iter().any(|a|a.0==p&&a.1>=size){return Err(0x100a);}
    let m=&mut image.memory;
    if word(m,p+offset+4,2)?!=size as u64||word(m,p+offset+6,2)?!=0{return Err(0x100a);}
    let fields=word(m,p+offset+8,4)?;
    if fields&!0x8103!=0{return Err(0x100a);}
    if fields&1!=0{result.orientation=word(m,p+offset+12,2)?as u16;}
    if fields&2!=0{result.paper=word(m,p+offset+14,2)?as u16;}
    if fields&0x100!=0{result.copies=word(m,p+offset+22,2)?as u16;}
    if fields&0x8000!=0{result.collate=word(m,p+offset+36,2)?!=0;}
    if !matches!(result.orientation,1|2)||!matches!(result.paper,1|9)||!(1..=9999).contains(&result.copies){return Err(0x100a);}
    Ok(result)
}
pub(super) fn inputs(image:&mut GuestImage,address:u64,x86:bool,wide:bool)->Result<Settings,u32>{
    let width=if x86{4}else{8};let mode=word(&mut image.memory,address+if x86{8}else{16},width)?;
    let names=word(&mut image.memory,address+if x86{12}else{24},width)?;
    if names!=0{
        let size=image.win32.heap_allocations.iter().find(|a|a.0==names).map(|a|a.1).ok_or(0x100au32)?;
        let offset=word(&mut image.memory,names+2,2)?as usize*if wide{2}else{1};
        let len=("NT61 PostScript Document Writer".len()+1)*if wide{2}else{1};
        if offset<8||offset+len>size||string(&mut image.memory,names+offset as u64,wide)?!="NT61 PostScript Document Writer"{return Err(0x100a);}
    }
    read_settings(image,mode,wide)
}
fn create(image:&mut GuestImage,settings:Settings)->Result<u64,u32>{
    let state=&mut image.win32.printing;if state.dcs.len()>=64{return Err(8);}let handle=state.next;state.next+=1;
    state.dcs.push(Dc{handle,document:None,page:false,pages:0,color:0,x:0,y:0,settings});Ok(handle)
}
pub(super) fn dialog_result(image:&mut GuestImage,address:u64,x86:bool,wide:bool,p:&w::Packet)->Result<(),u32>{
    let mut settings=inputs(image,address,x86,wide)?;
    let page=p.values[0]==w::PAGE;
    if page{settings.orientation=if p.values[4]>p.values[5]{2}else{1};let scale=if p.values[3]&8!=0{100.0}else{1000.0/25.4};let shorter=p.values[4].min(p.values[5])as f32/scale;settings.paper=if shorter>213.0{1}else{9};}
    else{settings.copies=if p.values[3]&0x40000!=0{p.values[6].max(1).min(9999)as u16}else{1};settings.collate=p.values[3]&0x10!=0;}
    let width=if x86{4}else{8};let mode_size=if wide{220}else{156};
    let existing_mode=word(&mut image.memory,address+if x86{8}else{16},width)?;
    let existing_names=word(&mut image.memory,address+if x86{12}else{24},width)?;
    let name="NT61 PostScript Document Writer";let driver="NT61PS";let port="Documents";
    let unit=if wide{2}else{1};let first=8/unit;let device=first+driver.len()+1;let output=device+name.len()+1;
    // Validate every existing output allocation before allocating or changing
    // DEVMODE. A rejected DEVNAMES must leave both caller handles untouched.
    if existing_names!=0 && !image.win32.heap_allocations.iter().any(|a|a.0==existing_names&&a.1>=(output+port.len()+1)*unit){return Err(0x100a);}
    if !page&&p.values[3]&0x300!=0&&image.win32.printing.dcs.len()>=64{return Err(8);}
    let devmode=if existing_mode!=0{existing_mode}else{heap_allocate(image,mode_size)?};
    let names=if existing_names!=0{existing_names}else{match heap_allocate(image,256){Ok(n)=>n,Err(e)=>{if existing_mode==0{let _=heap_free(image,devmode);}return Err(e);}}};
    let offset=if wide{64}else{32};
    write_string(&mut image.memory,devmode,name,wide)?;
    for(off,value,size)in [(offset,0x401,2),(offset+2,0x601,2),(offset+4,mode_size,2),(offset+8,0x8103,4),(offset+12,settings.orientation as u64,2),(offset+14,settings.paper as u64,2),(offset+22,settings.copies as u64,2),(offset+36,u64::from(settings.collate),2)]{put(&mut image.memory,devmode+off,value,size)?;}
    for(off,value)in [(0,first),(2,device),(4,output),(6,1)]{put(&mut image.memory,names+off,value as u64,2)?;}
    write_string(&mut image.memory,names+8,driver,wide)?;write_string(&mut image.memory,names+(device*unit)as u64,name,wide)?;write_string(&mut image.memory,names+(output*unit)as u64,port,wide)?;
    let dc=if !page&&p.values[3]&0x300!=0{create(image,settings)?}else{0};
    put(&mut image.memory,address+if x86{8}else{16},devmode,width)?;put(&mut image.memory,address+if x86{12}else{24},names,width)?;
    if !page{
        put(&mut image.memory,address+if x86{16}else{32},dc,width)?;put(&mut image.memory,address+if x86{20}else{40},p.values[3]as u64,4)?;
        for(off,value)in [(if x86{24}else{44},p.values[4]),(if x86{26}else{46},p.values[5]),(if x86{32}else{52},if p.values[3]&0x40000!=0{1}else{p.values[6].max(1)})]{put(&mut image.memory,address+off,value as u64,2)?;}
    }Ok(())
}
pub(super) fn invoke(
    image: &mut GuestImage,
    name: &str,
    a: &[u64],
    services: &mut dyn GuestServices,
) -> Result<u64, ApiError> {
    if matches!(name, "CreateDCW" | "CreateDCA") {
        if !services.printer_available() {
            return Err(1801u32.into());
        }
        let device = string(&mut image.memory, a[1], name.ends_with('W'))?;
        if device != "NT61 PostScript Document Writer" {
            return Err(1801u32.into());
        }
        let settings=read_settings(image,a[3],name.ends_with('W'))?;
        return Ok(create(image,settings)?);
    }
    if name == "DeleteDC" {
        let state = &mut image.win32.printing;
        let Some(i) = state.dcs.iter().position(|d| d.handle == a[0]) else {
            return Err(6u32.into());
        };
        state.dcs.remove(i);
        return Ok(1);
    }
    let text = if matches!(name, "TextOutW" | "TextOutA") {
        let count = usize::try_from(a[4]).map_err(|_| ApiError::Windows(87))?;
        if count > 16384 {
            return Err(87u32.into());
        }
        let bytes = read(
            &mut image.memory,
            a[3],
            count * if name.ends_with('W') { 2 } else { 1 },
        )?;
        Some(if name.ends_with('W') {
            String::from_utf16(
                &bytes
                    .chunks_exact(2)
                    .map(|b| u16::from_le_bytes([b[0], b[1]]))
                    .collect::<Vec<_>>(),
            )
            .map_err(|_| ApiError::Windows(87))?
        } else {
            if !bytes.is_ascii() {
                return Err(120u32.into());
            }
            String::from_utf8(bytes).map_err(|_| ApiError::Windows(87))?
        })
    } else {
        None
    };
    if matches!(name, "StartDocW" | "StartDocA") {
        let size = if image.thread.context.mode == DecodeMode::X86 {
            20
        } else {
            40
        };
        if word(&mut image.memory, a[1], 4)? != size {
            return Err(87u32.into());
        }
        read(&mut image.memory, a[1], size as usize)?;
    }
    if name == "MoveToEx" && a[3] != 0 && !image.memory.probe_write(a[3], 8) {
        return Err(87u32.into());
    }
    let dc = image
        .win32
        .printing
        .dcs
        .iter_mut()
        .find(|d| d.handle == a[0])
        .ok_or(ApiError::Windows(6))?;
    let result = match name {
        "GetDeviceCaps" => match a[1] as u32 {
            2 => 2,
            4 => dc.settings.mm().0 as u64,
            6 => dc.settings.mm().1 as u64,
            8|112 => (dc.settings.mm().0*96.0/25.4+0.5)as u64,
            10|113 => (dc.settings.mm().1*96.0/25.4+0.5)as u64,
            12 => 24,
            14 => 1,
            88 | 90 => 96,
            110 | 111 => 0,
            _ => return Err(ApiError::Unsupported),
        },
        "StartDocA" | "StartDocW" => {
            if dc.document.is_some() {
                return Err(1906u32.into());
            }
            let(w,h)=dc.settings.mm();dc.document=Some(alloc::format!("%!PS-Adobe-3.0\n%%Creator: NT61 guest printer\n%%Pages: (atend)\n%%EndComments\n<< /PageSize [{:.2} {:.2}] /NumCopies {} /Collate {} >> setpagedevice\n",w*72.0/25.4,h*72.0/25.4,dc.settings.copies,dc.settings.collate));
            dc.pages = 0;
            1
        }
        "AbortDoc" => {
            dc.document = None;
            dc.page = false;
            1
        }
        "StartPage" => {
            if dc.page {
                return Err(1906u32.into());
            }
            let doc = dc.document.as_mut().ok_or(ApiError::Windows(1906))?;
            dc.pages += 1;
            let _=writeln!(doc,"%%Page: {} {}\ngsave\n0 {:.2} translate 0.75 -0.75 scale\n/Courier findfont 16 scalefont setfont",dc.pages,dc.pages,dc.settings.mm().1*72.0/25.4);
            dc.page = true;
            1
        }
        "EndPage" => {
            if !dc.page {
                return Err(1906u32.into());
            }
            dc.document
                .as_mut()
                .ok_or(ApiError::Windows(1906))?
                .push_str("grestore\nshowpage\n");
            dc.page = false;
            1
        }
        "EndDoc" => {
            if dc.page || dc.pages == 0 {
                return Err(1906u32.into());
            }
            let doc = dc.document.as_mut().ok_or(ApiError::Windows(1906))?;
            let length = doc.len();
            let _ = write!(doc, "%%Trailer\n%%Pages: {}\n%%EOF\n", dc.pages);
            match services.print_document(doc.as_bytes()) {
                Ok(id) => {
                    dc.document = None;
                    id as u64
                }
                Err(e) => {
                    doc.truncate(length);
                    return Err(e.into());
                }
            }
        }
        "SetTextColor" => {
            let old = dc.color;
            dc.color = a[1] as u32;
            old as u64
        }
        "MoveToEx" => {
            if a[3] != 0 {
                put(&mut image.memory, a[3], dc.x as u32 as u64, 4)?;
                put(&mut image.memory, a[3] + 4, dc.y as u32 as u64, 4)?;
            }
            dc.x = a[1] as i32;
            dc.y = a[2] as i32;
            1
        }
        "TextOutW" | "TextOutA" | "LineTo" | "Rectangle" => {
            if !dc.page {
                return Err(1906u32.into());
            }
            let doc = dc.document.as_mut().ok_or(ApiError::Windows(1906))?;
            if doc.len() > 1024 * 1024 - 128 * 1024 {
                return Err(8u32.into());
            }
            if let Some(text) = text {
                let _ = write!(
                    doc,
                    "{:.4} {:.4} {:.4} setrgbcolor\n{} {} moveto (",
                    (dc.color & 255) as f32 / 255.0,
                    ((dc.color >> 8) & 255) as f32 / 255.0,
                    ((dc.color >> 16) & 255) as f32 / 255.0,
                    a[1] as i32,
                    a[2] as i32
                );
                for byte in text.bytes() {
                    match byte {
                        b'(' | b')' | b'\\' => {
                            doc.push('\\');
                            doc.push(char::from(byte));
                        }
                        32..=126 => doc.push(char::from(byte)),
                        _ => {
                            let _ = write!(doc, "\\{byte:03o}");
                        }
                    }
                }
                doc.push_str(") show\n");
            } else if name == "LineTo" {
                let _ = writeln!(
                    doc,
                    "newpath {} {} moveto {} {} lineto stroke",
                    dc.x, dc.y, a[1] as i32, a[2] as i32
                );
                dc.x = a[1] as i32;
                dc.y = a[2] as i32;
            } else {
                let _ = writeln!(
                    doc,
                    "newpath {} {} moveto {} {} lineto {} {} lineto {} {} lineto closepath stroke",
                    a[1] as i32,
                    a[2] as i32,
                    a[3] as i32,
                    a[2] as i32,
                    a[3] as i32,
                    a[4] as i32,
                    a[1] as i32,
                    a[4] as i32
                );
            }
            1
        }
        _ => return Err(ApiError::Unsupported),
    };
    Ok(result)
}
