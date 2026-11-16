//! Bounded HWND ownership, copied control messages and guest window procedures.
use super::*;
use crate::arch::common::btl::{dialog_wire as w, ir::X86Reg};
struct Class {
    atom: u16,
    name: String,
    procedure: u64,
}
#[derive(Clone)]
struct Window {
    hwnd: u32,
    parent: u32,
    id: u32,
    kind: u32,
    procedure: u64,
    title: String,
    width: u32,
    height: u32,
    wide:bool,
}
#[derive(Clone, Copy)]
struct Message {
    hwnd: u64,
    message: u32,
    wp: u64,
    lp: u64,
}
pub(super) enum Completion { Value, FreeValue(u64), Fixed(u64), Create{hwnd:u32,data:u64,procedure:u64,phase:u8} }
struct Frame { pc:u64,sp:u64,completion:Completion }
pub(super) const CALLBACK_RETURN:u32=0xfffe;
pub(super) struct State {
    pub common_classes: u32,
    classes: Vec<Class>,
    windows: Vec<Window>,
    next: u32,
    messages: Vec<Message>,
    pub callback: Option<(u64, [u64; 4])>,
    trampoline: Option<u64>,
    frames:Vec<Frame>,
    notifications:Vec<u64>,
    pub completion:Option<Completion>,
}
impl State {
    pub fn new() -> Self {
        Self {
            common_classes: 0,
            classes: Vec::new(),
            windows: Vec::new(),
            next: 0x10000,
            messages: Vec::new(),
            callback: None,
            trampoline: None,
            frames:Vec::new(),
            notifications:Vec::new(),
            completion:None,
        }
    }
}
fn send(services: &mut dyn GuestServices, mut packet: w::Packet) -> Result<w::Packet, ApiError> {
    packet.values[0] = w::WINDOW;
    let mut bytes = vec![0; w::PACKET_SIZE];
    packet.encode(&mut bytes);
    let response = services
        .desktop_dialog(&bytes)
        .map_err(|_| ApiError::Windows(120))?;
    let reply = w::Packet::decode(&response).ok_or(ApiError::Windows(13))?;
    if reply.values[0] != w::WINDOW || reply.values[1] != w::ACCEPTED {
        return Err(13u32.into());
    }
    if reply.values[19] != 0 {
        return Err(reply.values[19].into());
    }
    Ok(reply)
}
fn packet(action: u32, hwnd: u32) -> w::Packet {
    let mut p = w::EMPTY;
    p.values[4] = action;
    p.values[5] = hwnd;
    p
}
fn scalar(p: &w::Packet) -> u64 {
    p.values[20] as u64 | ((p.values[21] as u64) << 32)
}
fn words(p: &mut w::Packet, index: usize, value: u64) {
    p.values[index] = value as u32;
    p.values[index + 1] = (value >> 32) as u32;
}
fn copied_text(p: &mut w::Packet, text: &str) -> Result<(), ApiError> {
    if p.set_string(0, text) {
        Ok(())
    } else {
        Err(87u32.into())
    }
}
fn message_read(image: &mut GuestImage, p: u64) -> Result<Message, ApiError> {
    let x86 = image.thread.context.mode == DecodeMode::X86;
    let n = if x86 { 4 } else { 8 };
    read(&mut image.memory, p, if x86 { 28 } else { 48 })?;
    Ok(Message {
        hwnd: word(&mut image.memory, p, n)?,
        message: word(&mut image.memory, p + if x86 { 4 } else { 8 }, 4)? as u32,
        wp: word(&mut image.memory, p + if x86 { 8 } else { 16 }, n)?,
        lp: word(&mut image.memory, p + if x86 { 12 } else { 24 }, n)?,
    })
}
fn message_write(image: &mut GuestImage, p: u64, message: Message) -> Result<(), ApiError> {
    let x86 = image.thread.context.mode == DecodeMode::X86;
    let n = if x86 { 4 } else { 8 };
    let size = if x86 { 28 } else { 48 };
    if !image.memory.probe_write(p, size) {
        return Err(87u32.into());
    }
    let bytes = vec![0; size];
    if !image.memory.write_virt(p, &bytes) {
        return Err(87u32.into());
    }
    put(&mut image.memory, p, message.hwnd, n)?;
    put(
        &mut image.memory,
        p + if x86 { 4 } else { 8 },
        message.message as u64,
        4,
    )?;
    put(
        &mut image.memory,
        p + if x86 { 8 } else { 16 },
        message.wp,
        n,
    )?;
    put(
        &mut image.memory,
        p + if x86 { 12 } else { 24 },
        message.lp,
        n,
    )?;
    Ok(())
}
pub(super) fn enter_callback(image:&mut GuestImage,procedure:u64,args:[u64;4])->Result<(),u32>{
    if image.win32.user.frames.len()>=64{return Err(1816);}
    let x86=image.thread.context.mode==DecodeMode::X86;
    let width=if x86{4}else{8};
    let old=image.thread.context.reg(X86Reg::Rsp);let pc=image.thread.context.rip;
    let trampoline=if let Some(pc)=image.win32.user.trampoline{pc}else{
        // Save the callback result before loading the private gateway service.
        let mut code=vec![0x50,0xb8];code.extend_from_slice(&(catalog::GATEWAY_BASE|CALLBACK_RETURN).to_le_bytes());
        code.extend_from_slice(if x86{&[0xcd,0x2e]}else{&[0x0f,0x05]});
        let p=allocate(image,0,16,READ|WRITE|EXECUTE)?;
        if !image.memory.write_virt(p,&code){return Err(87);}image.win32.user.trampoline=Some(p);p
    };
    let sp=old.checked_sub(if x86{20}else{48}).ok_or(87u32)?;
    if !image.memory.probe_write(sp,if x86{20}else{48}){return Err(87);}
    put(&mut image.memory,sp,trampoline,width)?;
    if x86{for(i,arg)in args.iter().enumerate(){put(&mut image.memory,sp+4+i as u64*4,*arg,4)?;}}
    else{for(reg,arg)in [X86Reg::Rcx,X86Reg::Rdx,X86Reg::R8,X86Reg::R9].into_iter().zip(args){image.thread.context.set_reg(reg,arg);}}
    let completion=image.win32.user.completion.take().unwrap_or(Completion::Value);
    image.win32.user.frames.push(Frame{pc,sp:old,completion});
    image.thread.context.set_reg(X86Reg::Rsp,sp);image.thread.context.rip=procedure;Ok(())
}
pub(super) fn callback_return(image:&mut GuestImage,services:&mut dyn GuestServices)->Result<Outcome,StopReason>{
    let failure=||StopReason::DataFault{pc:0};
    let sp=image.thread.context.reg(X86Reg::Rsp);
    let result=word(&mut image.memory,sp,if image.thread.context.mode==DecodeMode::X86{4}else{8}).map_err(|_|failure())?;
    let frame=image.win32.user.frames.pop().ok_or_else(failure)?;
    image.thread.context.rip=frame.pc;image.thread.context.set_reg(X86Reg::Rsp,frame.sp);
    let result=match frame.completion{
        Completion::Value=>result,Completion::FreeValue(pointer)=>{heap_free(image,pointer).map_err(|_|failure())?;result},Completion::Fixed(value)=>value,
        Completion::Create{hwnd,data,procedure,phase}=>{
            if phase==0&&result!=0 {
                image.win32.user.completion=Some(Completion::Create{hwnd,data,procedure,phase:1});
                enter_callback(image,procedure,[hwnd as u64,1,0,data]).map_err(|_|failure())?;
                return Ok(Outcome::Continue);
            }
            heap_free(image,data).map_err(|_|failure())?;
            if (phase==0&&result==0)||(phase==1&&result as u32==u32::MAX){
                let _=send(services,packet(2,hwnd));
                image.win32.user.windows.retain(|w|w.hwnd!=hwnd&&w.parent!=hwnd);0
            }else{hwnd as u64}
        }
    };
    Ok(Outcome::Return(result))
}
fn queue(state: &mut State, message: Message) -> Result<(), ApiError> {
    if state.messages.len() >= 256 {
        return Err(1816u32.into());
    }
    state.messages.push(message);
    Ok(())
}
pub(super) fn invoke(
    image: &mut GuestImage,
    name: &str,
    a: &[u64],
    services: &mut dyn GuestServices,
) -> Result<u64, ApiError> {
    let x86 = image.thread.context.mode == DecodeMode::X86;
    let width = if x86 { 4 } else { 8 };
    let wide = name.ends_with('W');
    match name {
        "RegisterClassExW" | "RegisterClassExA" => {
            let p = a[0];
            if word(&mut image.memory, p, 4)? != if x86 { 48 } else { 80 } {
                return Err(87u32.into());
            }
            read(&mut image.memory, p, if x86 { 48 } else { 80 })?;
            let procedure = word(&mut image.memory, p + 8, width)?;
            let pointer = word(&mut image.memory, p + if x86 { 40 } else { 64 }, width)?;
            let text = string(&mut image.memory, pointer, wide)?;
            if procedure == 0 || text.is_empty() || image.win32.user.classes.len() >= 256 {
                return Err(87u32.into());
            }
            if image
                .win32
                .user
                .classes
                .iter()
                .any(|c| c.name.eq_ignore_ascii_case(&text))
            {
                return Err(1410u32.into());
            }
            let atom = 0xc000 + image.win32.user.classes.len() as u16;
            image.win32.user.classes.push(Class {
                atom,
                name: text,
                procedure,
            });
            Ok(atom as u64)
        }
        "CreateWindowExW" | "CreateWindowExA" => {
            if image.win32.user.windows.len() >= 256 {
                return Err(8u32.into());
            }
            let class = if a[1] <= 0xffff {
                image
                    .win32
                    .user
                    .classes
                    .iter()
                    .find(|c| c.atom as u64 == a[1])
                    .map(|c| c.name.clone())
                    .ok_or(ApiError::Windows(1407))?
            } else {
                string(&mut image.memory, a[1], wide)?
            };
            let title = if a[2] == 0 {
                String::new()
            } else {
                string(&mut image.memory, a[2], wide)?
            };
            let kind = match class.to_ascii_lowercase().as_str() {
                "syslistview32" => 1,
                "systreeview32" => 2,
                "toolbarwindow32" => 3,
                "combobox" => 4,
                "static" => 5,
                "button" => 6,
                _ => 0,
            };
            let procedure = image
                .win32
                .user
                .classes
                .iter()
                .find(|c| c.name.eq_ignore_ascii_case(&class))
                .map_or(0, |c| c.procedure);
            if kind == 0 && procedure == 0 {
                return Err(1407u32.into());
            }
            if matches!(kind, 1..=3)
                && image.win32.user.common_classes
                    & (match kind {
                        1 => 1,
                        2 => 2,
                        _ => 4,
                    })
                    == 0
            {
                return Err(1407u32.into());
            }
            let parent = a[8] as u32;
            if parent != 0 && !image.win32.user.windows.iter().any(|w| w.hwnd == parent) {
                return Err(1400u32.into());
            }
            // The first transport supports common controls parented directly to a root.
            if parent != 0
                && image
                    .win32
                    .user
                    .windows
                    .iter()
                    .any(|w| w.hwnd == parent && w.parent != 0)
            {
                return Err(120u32.into());
            }
            let hwnd = image.win32.user.next;
            let mut p = packet(1, hwnd);
            p.values[6] = parent;
            p.values[7] = kind;
            p.values[18] = a[3] as u32;
            p.values[17] = a[9] as u32;
            p.values[13] = a[4] as u32;
            p.values[14] = a[5] as u32;
            p.values[15] = if a[6] as u32 == 0x80000000 {
                640
            } else {
                a[6] as u32
            };
            p.values[16] = if a[7] as u32 == 0x80000000 {
                460
            } else {
                a[7] as u32
            };
            copied_text(&mut p, &title)?;
            send(services, p)?;
            image.win32.user.next += 1;
            image.win32.user.windows.push(Window {
                hwnd,
                parent,
                id: a[9] as u32,
                kind,
                procedure,
                title,
                width: p.values[15],
                height: p.values[16],
                wide,
            });
            if procedure != 0 {
                let data=heap_allocate(image,if x86{48}else{80})?;
                let fields:Vec<(u64,u64,usize)>=if x86 {
                    vec![(0,a[11],4),(4,a[10],4),(8,a[9],4),(12,a[8],4),(16,p.values[16]as u64,4),(20,p.values[15]as u64,4),(24,a[5],4),(28,a[4],4),(32,a[3],4),(36,a[2],4),(40,a[1],4),(44,a[0],4)]
                }else{
                    vec![(0,a[11],8),(8,a[10],8),(16,a[9],8),(24,a[8],8),(32,p.values[16]as u64,4),(36,p.values[15]as u64,4),(40,a[5],4),(44,a[4],4),(48,a[3],4),(56,a[2],8),(64,a[1],8),(72,a[0],4)]
                };
                for(offset,value,size)in fields{put(&mut image.memory,data+offset,value,size)?;}
                image.win32.user.completion=Some(Completion::Create{hwnd,data,procedure,phase:0});
                image.win32.user.callback=Some((procedure,[hwnd as u64,0x81,0,data]));
            }
            Ok(hwnd as u64)
        }
        "PostQuitMessage" => {
            queue(
                &mut image.win32.user,
                Message {
                    hwnd: 0,
                    message: 0x12,
                    wp: a[0],
                    lp: 0,
                },
            )?;
            Ok(0)
        }
        "PostMessageW" | "PostMessageA" => {
            if a[0] != 0
                && !image
                    .win32
                    .user
                    .windows
                    .iter()
                    .any(|w| w.hwnd as u64 == a[0])
            {
                return Err(1400u32.into());
            }
            queue(
                &mut image.win32.user,
                Message {
                    hwnd: a[0],
                    message: a[1] as u32,
                    wp: a[2],
                    lp: a[3],
                },
            )?;
            Ok(1)
        }
        "TranslateMessage" => {
            message_read(image, a[0])?;
            Ok(0)
        }
        "GetMessageW" | "GetMessageA" | "PeekMessageW" | "PeekMessageA" => {
            if a[2] != 0 || a[3] != 0 {
                return Err(ApiError::Unsupported);
            }
            let peek = name.starts_with("Peek");
            if !image.memory.probe_write(a[0], if x86 { 28 } else { 48 }) {
                return Err(87u32.into());
            }
            loop {
                if let Some(index) = image
                    .win32
                    .user
                    .messages
                    .iter()
                    .position(|m| m.message == 0x12 || a[1] == 0 || m.hwnd == a[1])
                {
                    let message = image.win32.user.messages[index];
                    if !peek || a[4] & 1 != 0 {
                        image.win32.user.messages.remove(index);
                    }
                    message_write(image, a[0], message)?;
                    return Ok(if !peek && message.message == 0x12 {
                        0
                    } else {
                        1
                    });
                }
                let roots: Vec<u32> = image
                    .win32
                    .user
                    .windows
                    .iter()
                    .filter(|w| w.parent == 0)
                    .map(|w| w.hwnd)
                    .collect();
                for root in roots {
                    let mut p = packet(4, root);
                    p.values[9] = 1;
                    let r = send(services, p)?;
                    if r.values[20] == 0 {
                        continue;
                    }
                    let message = r.values[8];
                    let mut lp = r.values[11] as u64 | ((r.values[12] as u64) << 32);
                    if message == 0x4e {
                        if image.win32.user.notifications.len()>=256{return Err(1816u32.into());}
                        let notification = heap_allocate(image, 256)?;
                        image.win32.user.notifications.push(notification);
                        put(&mut image.memory, notification, lp, width)?;
                        put(
                            &mut image.memory,
                            notification + width as u64,
                            r.values[9] as u64,
                            width,
                        )?;
                        put(
                            &mut image.memory,
                            notification + 2 * width as u64,
                            r.values[22] as u64,
                            4,
                        )?;
                        let sender=image.win32.user.windows.iter().find(|w|w.hwnd as u64==lp).cloned();
                        let header=if x86{12}else{24};let code=r.values[22]as i32;
                        if sender.as_ref().is_some_and(|w|w.kind==1){
                            // NMLISTVIEW/NMITEMACTIVATE share this prefix.
                            put(&mut image.memory,notification+header,if code == -108{u32::MAX as u64}else{r.values[24]as u64},4)?;
                            put(&mut image.memory,notification+header+4,if code == -108{r.values[23]as u64}else{0},4)?;
                            put(&mut image.memory,notification+header+8,r.values[27]as u64,4)?;
                            put(&mut image.memory,notification+header+16,8,4)?;
                            put(&mut image.memory,notification+if x86{40}else{56},r.values[25]as u64|((r.values[26]as u64)<<32),width)?;
                        }else if let Some(sender)=sender.filter(|w|w.kind==2){
                            let code=if !sender.wide{match code{-451=>-402,-455=>-406,v=>v}}else{code};
                            put(&mut image.memory,notification+2*width as u64,code as u32 as u64,4)?;
                            // NMTREEVIEW action and complete old/new TVITEM records.
                            put(&mut image.memory,notification+header,if matches!(code,-455|-406){2}else{1},4)?;
                            let first=if x86{16}else{32};let item_size=if x86{40}else{56};
                            for(offset,handle,data,state)in [(first,r.values[28],r.values[29]as u64|((r.values[30]as u64)<<32),0),(first+item_size,r.values[24],r.values[25]as u64|((r.values[26]as u64)<<32),r.values[27])]{
                                let item=notification+offset;put(&mut image.memory,item,0x1c,4)?;put(&mut image.memory,item+if x86{4}else{8},handle as u64,width)?;put(&mut image.memory,item+if x86{8}else{16},state as u64,4)?;put(&mut image.memory,item+if x86{36}else{48},data,width)?;
                            }
                        }else if code == -710{put(&mut image.memory,notification+header,r.values[23]as u64,4)?;}
                        lp = notification;
                    }
                    queue(
                        &mut image.win32.user,
                        Message {
                            hwnd: r.values[5] as u64,
                            message,
                            wp: r.values[9] as u64 | ((r.values[10] as u64) << 32),
                            lp,
                        },
                    )?;
                }
                if peek && image.win32.user.messages.is_empty() {
                    return Ok(0);
                }
                if image.win32.user.windows.is_empty() && image.win32.user.messages.is_empty() {
                    return Err(1400u32.into());
                }
            }
        }
        "DispatchMessageW" | "DispatchMessageA" => {
            let message = message_read(image, a[0])?;
            let procedure = image
                .win32
                .user
                .windows
                .iter()
                .find(|w| w.hwnd as u64 == message.hwnd)
                .map_or(0, |w| w.procedure);
            if let Some(index)=image.win32.user.notifications.iter().position(|p|*p==message.lp){
                let pointer=image.win32.user.notifications.remove(index);
                if procedure!=0{image.win32.user.completion=Some(Completion::FreeValue(pointer));}else{heap_free(image,pointer)?;}
            }
            if procedure != 0 {
                image.win32.user.callback = Some((procedure,[message.hwnd,message.message as u64,message.wp,message.lp]));
            }
            Ok(0)
        }
        "IsWindow" => Ok(u64::from(
            image
                .win32
                .user
                .windows
                .iter()
                .any(|w| w.hwnd as u64 == a[0]),
        )),
        _ => {
            let window = image
                .win32
                .user
                .windows
                .iter()
                .find(|w| w.hwnd as u64 == a[0])
                .cloned()
                .ok_or(ApiError::Windows(1400))?;
            match name {
                "DefWindowProcA" | "DefWindowProcW" => {
                    if a[1] == 0x10 {
                        invoke(image, "DestroyWindow", &a[..1], services)
                    } else {
                        Ok(u64::from(a[1]==0x81))
                    }
                }
                "DestroyWindow" => {
                    send(services, packet(2, window.hwnd))?;
                    image
                        .win32
                        .user
                        .windows
                        .retain(|w| w.hwnd != window.hwnd && w.parent != window.hwnd);
                    if window.procedure != 0 {
                        image.win32.user.completion=Some(Completion::Fixed(1));
                        image.win32.user.callback = Some((window.procedure, [a[0], 2, 0, 0]));
                    }
                    Ok(1)
                }
                "GetDlgItem" => Ok(image
                    .win32
                    .user
                    .windows
                    .iter()
                    .find(|w| w.parent == window.hwnd && w.id == a[1] as u32)
                    .map_or(0, |w| w.hwnd as u64)),
                "GetParent" => Ok(window.parent as u64),
                "GetClientRect" => {
                    if !image.memory.probe_write(a[1], 16) {
                        return Err(87u32.into());
                    }
                    for (i, v) in [
                        0,
                        0,
                        window
                            .width
                            .saturating_sub(if window.parent == 0 { 12 } else { 0 }),
                        window
                            .height
                            .saturating_sub(if window.parent == 0 { 36 } else { 0 }),
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        put(&mut image.memory, a[1] + i as u64 * 4, v as u64, 4)?;
                    }
                    Ok(1)
                }
                "ShowWindow" => {
                    let mut p = packet(5, window.hwnd);
                    p.values[9] = a[1] as u32;
                    send(services, p)?;
                    Ok(1)
                }
                "UpdateWindow" => Ok(1),
                "SetWindowTextW" | "SetWindowTextA" => {
                    let title = string(&mut image.memory, a[1], wide)?;
                    let mut p = packet(6, window.hwnd);
                    copied_text(&mut p, &title)?;
                    send(services, p)?;
                    image
                        .win32
                        .user
                        .windows
                        .iter_mut()
                        .find(|w| w.hwnd == window.hwnd)
                        .unwrap()
                        .title = title;
                    Ok(1)
                }
                "GetWindowTextW" | "GetWindowTextA" => {
                    let text = &window.title;
                    let count = if wide {
                        text.encode_utf16().count() + 1
                    } else {
                        text.len() + 1
                    };
                    if count > a[2] as usize {
                        return Err(122u32.into());
                    }
                    write_string(&mut image.memory, a[1], text, wide)?;
                    Ok(count.saturating_sub(1) as u64)
                }
                "SetWindowPos" => {
                    if a[6] & 3 != 0 {
                        return Err(ApiError::Unsupported);
                    }
                    let mut p = packet(7, window.hwnd);
                    p.values[13] = a[2] as u32;
                    p.values[14] = a[3] as u32;
                    p.values[15] = a[4] as u32;
                    p.values[16] = a[5] as u32;
                    send(services, p)?;
                    Ok(1)
                }
                "SendMessageW" | "SendMessageA" => {
                    if window.procedure != 0 {
                        image.win32.user.callback =
                            Some((window.procedure, [a[0], a[1], a[2], a[3]]));
                        return Ok(0);
                    }
                    control_message(image, &window, a, wide, services)
                }
                _ => Err(ApiError::Unsupported),
            }
        }
    }
}
fn control_message(
    image: &mut GuestImage,
    window: &Window,
    a: &[u64],
    wide: bool,
    services: &mut dyn GuestServices,
) -> Result<u64, ApiError> {
    let x86 = image.thread.context.mode == DecodeMode::X86;
    let width = if x86 { 4 } else { 8 };
    let m = &mut image.memory;
    let msg = a[1] as u32;
    let mut p = packet(3, window.hwnd);
    p.values[8] = msg;
    words(&mut p, 9, a[2]);
    words(&mut p, 11, a[3]);
    let mut output = None;
    match window.kind {
        1 => match msg {
            0x1007 | 0x104d | 0x102e | 0x1074 | 0x102d | 0x1073 => {
                let data = a[3];
                read(m, data, if x86 { 40 } else { 56 })?;
                let mask = word(m, data, 4)?;
                p.values[24] = word(m, data + 4, 4)? as u32;
                p.values[25] = word(m, data + 8, 4)? as u32;
                let text = word(m, data + if x86 { 20 } else { 24 }, width)?;
                let count = word(m, data + if x86 { 24 } else { 32 }, 4)? as usize;
                let is_wide = matches!(msg, 0x104d | 0x1074 | 0x1073);
                if matches!(msg, 0x102d | 0x1073) {
                    if count == 0
                        || count > 1024
                        || !m.probe_write(text, count * if is_wide { 2 } else { 1 })
                    {
                        return Err(87u32.into());
                    }
                    output = Some((text, count, is_wide));
                } else if matches!(msg, 0x102e | 0x1074) || mask & 1 != 0 {
                    if text == u64::MAX || text == u32::MAX as u64 {
                        return Err(ApiError::Unsupported);
                    }
                    copied_text(&mut p, &string(m, text, is_wide)?)?;
                }
                if matches!(msg, 0x1007 | 0x104d) {
                    words(
                        &mut p,
                        11,
                        word(m, data + if x86 { 32 } else { 40 }, width)?,
                    );
                }
            }
            0x101b | 0x1061 => {
                let data = a[3];
                read(m, data, if x86 { 32 } else { 40 })?;
                let mask = word(m, data, 4)?;
                p.values[25] = if mask & 2 != 0 {
                    word(m, data + 8, 4)? as u32
                } else {
                    120
                };
                if mask & 4 != 0 {
                    let text = word(m, data + if x86 { 12 } else { 16 }, width)?;
                    copied_text(&mut p, &string(m, text, msg == 0x1061)?)?;
                }
            }
            0x1004 | 0x1008 | 0x1009 | 0x1032 | 0x1013 => {}
            _ => return Err(ApiError::Unsupported),
        },
        2 => match msg {
            0x1100 | 0x1132 => {
                let data = a[3];
                read(m, data, if x86 { 48 } else { 72 })?;
                let parent = word(m, data, width)?;
                let insert_after = word(m, data + width as u64, width)?;
                // TVI_LAST only; ordering callbacks and sorted insertion need collation.
                if insert_after as u32 != 0xffff0002 && insert_after != 0 {
                    return Err(ApiError::Unsupported);
                }
                words(&mut p, 9, parent);
                let item = data + 2 * width as u64;
                let mask = word(m, item, 4)?;
                if mask & 1 != 0 {
                    let text = word(m, item + if x86 { 16 } else { 24 }, width)?;
                    copied_text(&mut p, &string(m, text, msg == 0x1132)?)?;
                }
                words(
                    &mut p,
                    11,
                    word(m, item + if x86 { 36 } else { 48 }, width)?,
                );
            }
            0x110a | 0x110b if a[2] != 9 => return Err(ApiError::Unsupported),
            0x1101 | 0x1102 | 0x1105 | 0x110a | 0x110b => {}
            _ => return Err(ApiError::Unsupported),
        },
        3 => match msg {
            0x415 | 0x444 => {
                if a[2] != 1 {
                    return Err(ApiError::Unsupported);
                }
                let data = a[3];
                read(m, data, if x86 { 20 } else { 32 })?;
                p.values[24] = word(m, data + 4, 4)? as u32;
                p.values[25] = word(m, data + 9, 1)? as u32;
                p.values[26] = word(m, data + 8, 1)? as u32;
                let text = word(m, data + if x86 { 16 } else { 24 }, width)?;
                if text <= 0xffff {
                    return Err(ApiError::Unsupported);
                }
                copied_text(&mut p, &string(m, text, msg == 0x444)?)?;
            }
            0x421 => {
                if a[2] != if x86 { 20 } else { 32 } {
                    return Err(87u32.into());
                }
            }
            0x401 | 0x402 | 0x404 | 0x414 | 0x41e => {}
            _ => return Err(ApiError::Unsupported),
        },
        4 => match msg {
            0x143 => copied_text(&mut p, &string(m, a[3], wide)?)?,
            0x148 => {
                output = Some((a[3], 1024, wide));
            }
            0x146 | 0x147 | 0x149 | 0x14e | 0x14b => {}
            _ => return Err(ApiError::Unsupported),
        },
        _ => match msg {
            0xc => copied_text(&mut p, &string(m, a[3], wide)?)?,
            0xe => {}
            _ => return Err(ApiError::Unsupported),
        },
    }
    let reply = send(services, p)?;
    if let Some((pointer, count, wide)) = output {
        let text = reply.string(0).ok_or(ApiError::Windows(13))?;
        let needed = if wide {
            text.encode_utf16().count() + 1
        } else {
            text.len() + 1
        };
        if needed > count {
            return Err(122u32.into());
        }
        write_string(&mut image.memory, pointer, text, wide)?;
    }
    Ok(scalar(&reply))
}
