//! Bounded HWND ownership, copied control messages and guest window procedures.
use super::*;
use crate::arch::common::btl::{dialog_wire as w, ir::X86Reg};

#[path = "user/dialog.rs"]
mod dialog;
#[path = "user/browse.rs"]
pub(super) mod browse;
#[path = "user/resource_handles.rs"]
mod resource_handles;
#[path = "user/system_menu.rs"]
mod system_menu;
#[path = "user/menu.rs"]
mod menu;

pub(super) use dialog::{poll as poll_dialog, GATEWAY as DIALOG_GATEWAY};

const US_KEYBOARD_LAYOUT: u64 = 0x0409_0409;
const STATUS_CLASS: &str = "msctls_statusbar32";
const STATUS_CLASS_FLAG: u32 = 0x04;
const STATUS_HEIGHT: u32 = 22;
struct Class {
    atom: u16,
    name: String,
    procedure: u64,
    extra_size: usize,
    menu: Option<(u64, menu::Resource)>,
}
#[derive(Clone)]
struct Window {
    thread_id: u32,
    hwnd: u32,
    parent: u32,
    style: u32,
    system_menu: Option<system_menu::Menu>,
    menu: u64,
    id: u32,
    kind: u32,
    procedure: u64,
    class: String,
    title: String,
    width: u32,
    height: u32,
    wide:bool,
    ex_style: u32,
    instance: u64,
    user_data: u32,
    extra: Vec<u8>,
    scroll_positions: [i32; 2],
    paint_pending: bool,
}
impl Window {
    fn client_dimensions(&self) -> (u32, u32) {
        if self.parent == 0 {
            (self.width.saturating_sub(12), self.height.saturating_sub(36))
        } else {
            (self.width, self.height)
        }
    }
}
#[derive(Clone, Copy)]
struct Message {
    hwnd: u64,
    message: u32,
    wp: u64,
    lp: u64,
}
pub(super) enum Completion {
    Value, FreeValue(u64), Fixed(u64),
    Create { hwnd: u32, data: u64, procedure: u64, phase: u8 },
    DialogInit { hwnd: u32, focus: u32, show: bool, modal: bool },
    DialogMessage { close: bool, notification: Option<u64> },
    DialogDestroy { hwnd: u32, procedure: u64, final_message: bool },
    BrowseFolder { hwnd: u32, pidl: u64 },
    RunOnce { address: u64, output: u64, native: bool },
    Print { handle: u64, job: u64 },
    EnumFont(font_enum::Enumeration),
    CrtInitializer { next: u64, end: u64 },
}
struct Frame { pc:u64,sp:u64,result_sp:u64,completion:Completion }
pub(super) const CALLBACK_RETURN:u32=0xfffe;
pub(super) struct State {
    pub common_classes: u32,
    dpi_aware: bool,
    classes: Vec<Class>,
    windows: Vec<Window>,
    next: u32,
    next_system_menu: u32,
    messages: Vec<(u32, Message)>,
    trampoline: Option<u64>,
    dialog_trampoline: Option<u64>,
    browse_trampoline: Option<u64>,
    notifications:Vec<u64>,
    resources: resource_handles::State,
    registered_messages: Vec<String>,
    client_dcs: Vec<(u64, u64, u32)>,
    menus: menu::Menus,
}
#[derive(Default)]
pub(super) struct ThreadState {
    pub callback: Option<(u64, [u64; 4])>,
    frames: Vec<Frame>,
    pub completion: Option<Completion>,
    active_window: u32,
    focus_root: u32,
    modals: Vec<dialog::Modal>,
    folders: Vec<browse::Folder>,
}
impl ThreadState {
    pub(super) fn calling_print_callback(&self, handle: u64) -> bool {
        self.frames.iter().any(|frame| matches!(frame.completion, Completion::Print { handle: dc, .. } if dc == handle))
    }
}
impl State {
    pub fn new() -> Self {
        Self {
            common_classes: 0,
            // Native unmanifested EXEs on the SP1 96-DPI reference start
            // aware, including WOW64. A scalable session must initialize
            // this from its actual policy instead of inheriting this default.
            dpi_aware: true,
            classes: Vec::new(),
            windows: Vec::new(),
            next: 0x10000,
            next_system_menu: 0x6000_0000,
            messages: Vec::new(),
            trampoline: None,
            dialog_trampoline: None,
            browse_trampoline: None,
            notifications:Vec::new(),
            resources: resource_handles::State::default(),
            registered_messages: Vec::new(),
            client_dcs: Vec::new(),
            menus: menu::Menus::default(),
        }
    }

    pub(super) fn owns_client_dc(&self, handle: u64) -> bool {
        self.client_dcs.iter().any(|entry| entry.0 == handle)
    }

    fn remove_windows(&mut self, hwnd: u32) {
        self.windows.retain(|window| window.hwnd != hwnd && window.parent != hwnd);
        self.menus.release_window(hwnd);
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
    if image.win32.local.user.frames.len()>=64{return Err(1816);}
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
    let completion=image.win32.local.user.completion.take().unwrap_or(Completion::Value);
    let count = match completion {
        Completion::RunOnce { .. } => 3,
        Completion::Print { .. } => 2,
        Completion::CrtInitializer { .. } => 0,
        _ => 4,
    };
    // x86 callbacks clean exactly their argument count. x64 entry RSP must
    // be 8 modulo 16, with all 32 bytes of shadow space below the old frame.
    let sp=if x86{old.checked_sub(4+count as u64*4).ok_or(87u32)?}
        else{(old.checked_sub(40).ok_or(87u32)?&!15).checked_sub(8).ok_or(87u32)?};
    if !image.memory.probe_write(sp,(old-sp)as usize){return Err(87);}
    put(&mut image.memory,sp,trampoline,width)?;
    if x86{for(i,arg)in args.iter().take(count).enumerate(){put(&mut image.memory,sp+4+i as u64*4,*arg,4)?;}}
    else{for(reg,arg)in [X86Reg::Rcx,X86Reg::Rdx,X86Reg::R8,X86Reg::R9].into_iter().zip(args){image.thread.context.set_reg(reg,arg);}}
    let result_sp=if x86{sp+count as u64*4}else{sp};
    image.win32.local.user.frames.push(Frame{pc,sp:old,result_sp,completion});
    image.thread.context.set_reg(X86Reg::Rsp,sp);image.thread.context.rip=procedure;Ok(())
}
pub(super) fn callback_return(image:&mut GuestImage,services:&mut dyn GuestServices)->Result<Outcome,StopReason>{
    let failure=||StopReason::DataFault{pc:0};
    let sp=image.thread.context.reg(X86Reg::Rsp);
    let result=word(&mut image.memory,sp,if image.thread.context.mode==DecodeMode::X86{4}else{8}).map_err(|_|failure())?;
    let frame=image.win32.local.user.frames.pop().ok_or_else(failure)?;
    if sp!=frame.result_sp{return Err(failure());}
    image.thread.context.rip=frame.pc;image.thread.context.set_reg(X86Reg::Rsp,frame.sp);
    let result=match frame.completion{
        Completion::Value=>result,Completion::FreeValue(pointer)=>{heap_free(image,pointer).map_err(|_|failure())?;result},Completion::Fixed(value)=>value,
        Completion::Print { handle, job } => super::printing::complete(image, handle, job, result as u32 != 0, services),
        Completion::EnumFont(enumeration) => {
            return font_enum::resume(image, enumeration, result as u32).map_err(|_| failure());
        }
        Completion::CrtInitializer { next, end } => {
            return super::crt_startup::resume(image, next, end).map_err(|_| failure());
        }
        Completion::DialogInit { hwnd, focus, show, modal } => {
            return dialog::initialized(image, services, hwnd, focus, show, modal, result != 0).map_err(|_| failure());
        }
        Completion::DialogMessage { close, notification } => {
            if let Some(pointer) = notification { heap_free(image, pointer).map_err(|_| failure())?; }
            return dialog::message_complete(image, close, result != 0).map_err(|_| failure());
        }
        Completion::DialogDestroy { hwnd, procedure, final_message } => {
            return dialog::destroy_complete(image, services, hwnd, procedure, final_message).map_err(|_| failure());
        }
        Completion::BrowseFolder { hwnd, pidl } => {
            return browse::callback_complete(image, services, hwnd, pidl).map_err(|_| failure());
        }
        Completion::RunOnce{address,output,native}=>super::runonce::callback_complete(image,address,output,native,result).map_err(|error| match error {
            super::ApiError::Unsupported => StopReason::UnsupportedApi { id: if native { 1048 } else { 139 } },
            _ => failure(),
        })?,
        Completion::Create{hwnd,data,procedure,phase}=>{
            if phase==0&&result!=0 {
                image.win32.local.user.completion=Some(Completion::Create{hwnd,data,procedure,phase:1});
                enter_callback(image,procedure,[hwnd as u64,1,0,data]).map_err(|_|failure())?;
                return Ok(Outcome::Continue);
            }
            heap_free(image,data).map_err(|_|failure())?;
            if (phase==0&&result==0)||(phase==1&&result as u32==u32::MAX){
                let _=send(services,packet(2,hwnd));
                image.win32.user.remove_windows(hwnd);
                0
            }else{hwnd as u64}
        }
    };
    Ok(Outcome::Return(result))
}
fn queue(image: &mut GuestImage, message: Message) -> Result<(), ApiError> {
    let owner = if message.hwnd == 0 { image.thread_id() } else {
        image.win32.user.windows.iter().find(|w| u64::from(w.hwnd) == message.hwnd)
            .map(|w| w.thread_id).ok_or(ApiError::Windows(1400))?
    };
    let state = &mut image.win32.user;
    if state.messages.len() >= 256 {
        return Err(1816u32.into());
    }
    state.messages.push((owner, message));
    Ok(())
}
fn receive_message(
    image: &mut GuestImage,
    services: &mut dyn GuestServices,
    a: &[u64],
    peek: bool,
) -> Result<u64, ApiError> {
    let x86 = image.thread.context.mode == DecodeMode::X86;
    let width = if x86 { 4 } else { 8 };
    if a[2] != 0 || a[3] != 0 {
        return Err(ApiError::Unsupported);
    }
    if !image.memory.probe_write(a[0], if x86 { 28 } else { 48 }) {
        return Err(87u32.into());
    }
    let mut polled = false;
    loop {
        if let Some(index) = image
            .win32
            .user
            .messages
            .iter()
            .position(|(owner,m)| *owner == image.thread_id() && (m.message == 0x12 || a[1] == 0 || m.hwnd == a[1]))
        {
            let message = image.win32.user.messages[index].1;
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
        if polled {
            let thread = image.thread_id();
            if let Some(window) = image.win32.user.windows.iter_mut().find(|window| {
                window.thread_id == thread && window.paint_pending
                    && (a[1] == 0 || a[1] == u64::from(window.hwnd))
            }) {
                if !peek || a[4] & 1 != 0 {
                    window.paint_pending = false;
                }
                let message = Message { hwnd: u64::from(window.hwnd), message: 0x000f, wp: 0, lp: 0 };
                message_write(image, a[0], message)?;
                return Ok(1);
            }
            return if peek { Ok(0) } else { Err(ApiError::Retry) };
        }
        polled = true;
        let roots: Vec<u32> = image
            .win32
            .user
            .windows
            .iter()
            .filter(|w| w.parent == 0 && w.thread_id == image.thread_id() && (a[1] == 0 || a[1] == u64::from(w.hwnd)))
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
                image,
                Message {
                    hwnd: r.values[5] as u64,
                    message,
                    wp: r.values[9] as u64 | ((r.values[10] as u64) << 32),
                    lp,
                },
            )?;
        }
    }
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
        "OpenClipboard" | "CloseClipboard" | "IsClipboardFormatAvailable" => {
            let mut request = match name {
                "OpenClipboard" => packet(w::OPEN_CLIPBOARD, u32::try_from(a[0]).map_err(|_| 1400u32)?),
                "CloseClipboard" => packet(w::CLOSE_CLIPBOARD, 0),
                _ => packet(w::CLIPBOARD_FORMAT_AVAILABLE, 0),
            };
            request.values[6] = if name == "IsClipboardFormatAvailable" { a[0] as u32 } else { image.thread_id() };
            Ok(scalar(&send(services, request)?))
        }
        "CharUpperW" => nls::uppercase_in_place(image, a[0]),
        "CharNextW" => {
            let pointer = a[0];
            if word(&mut image.memory, pointer, 2)? == 0 {
                return Ok(pointer);
            }
            pointer.checked_add(2).ok_or(NOACCESS.into())
        }
        "SetScrollPos" => {
            let bar = a[1] as i32;
            let style = match bar {
                0 => 0x0010_0000,
                1 => 0x0020_0000,
                _ => return Err(87u32.into()),
            };
            let window = image.win32.user.windows.iter_mut()
                .find(|window| u64::from(window.hwnd) == a[0])
                .ok_or(1400u32)?;
            if window.style & style == 0 {
                return Err(1447u32.into());
            }
            let previous = core::mem::replace(
                &mut window.scroll_positions[bar as usize],
                (a[2] as i32).clamp(0, 100),
            );
            Ok(previous as u64)
        }
        "InvalidateRect" => {
            if a[1] != 0 {
                read(&mut image.memory, a[1], 16)?;
            }
            let mut found = a[0] == 0;
            for window in &mut image.win32.user.windows {
                if a[0] == 0 || u64::from(window.hwnd) == a[0] {
                    found = true;
                    window.paint_pending = window.procedure != 0;
                }
            }
            if found { Ok(1) } else { Err(1400u32.into()) }
        }
        "WinHelpW" => Err(120u32.into()),
        "GetCursorPos" | "ScreenToClient" => {
            let convert = name == "ScreenToClient";
            let output = a[usize::from(convert)];
            if !image.memory.probe_write(output, 8) {
                return Err(NOACCESS.into());
            }
            let mut request = if convert {
                packet(w::SCREEN_TO_CLIENT, u32::try_from(a[0]).map_err(|_| 1400u32)?)
            } else {
                packet(w::CURSOR_POSITION, 0)
            };
            if convert {
                words(&mut request, 9, word(&mut image.memory, output, 8)?);
            }
            put(&mut image.memory, output, scalar(&send(services, request)?), 8)?;
            Ok(1)
        }
        "ChildWindowFromPoint" => {
            let hwnd = u32::try_from(a[0]).map_err(|_| 1400u32)?;
            let mut request = packet(w::CHILD_WINDOW_FROM_POINT, hwnd);
            words(&mut request, 9, a[1]);
            if x86 {
                request.values[10] = a[2] as u32;
            }
            Ok(scalar(&send(services, request)?))
        }
        "GetMenu" => {
            let window = image.win32.user.windows.iter().find(|window| u64::from(window.hwnd) == a[0]);
            Ok(window.map_or(0, |window| window.menu))
        }
        "GetSubMenu" => Ok(image.win32.user.menus.submenu(a[0], a[1] as u32)),
        "GetMenuState" | "EnableMenuItem" | "CheckMenuItem" if image.win32.user.menus.contains(a[0]) => {
            image.win32.user.menus.update(name, a[0], a[1] as u32, a[2] as u32)
        }
        "GetSystemMenu" | "GetMenuState" | "EnableMenuItem" | "CheckMenuItem" => {
            system_menu::invoke(&mut image.win32.user, name, a, services)
        }
        "MessageBeep" => Ok(u64::from(services.message_beep(a[0] as u32))),
        "SetWinEventHook" => Err(120u32.into()),
        "UnhookWinEvent" => Err(1404u32.into()),
        "MessageBoxW" => Err(120u32.into()),
        "IsDialogMessageW" => {
            let Some(dialog) = image.win32.user.windows.iter().find(|window| u64::from(window.hwnd) == a[0]) else {
                return Ok(0);
            };
            let hwnd = dialog.hwnd;
            let message = message_read(image, a[1])?;
            let mut target = message.hwnd;
            while target != 0 {
                if target == u64::from(hwnd) { return Err(120u32.into()); }
                target = image.win32.user.windows.iter().find(|window| u64::from(window.hwnd) == target)
                    .map_or(0, |window| u64::from(window.parent));
            }
            Ok(0)
        }
        "DialogBoxParamW" | "CreateDialogParamW" => dialog::create(image, services, a, name == "DialogBoxParamW"),
        "EndDialog" => dialog::end(image, a[0], a[1]),
        "GetKeyboardLayout" => {
            let thread = a[0] as u32;
            Ok(if thread == 0 || image.has_live_thread(thread) { US_KEYBOARD_LAYOUT } else { 0 })
        }
        "GetKeyboardLayoutList" => {
            let count = a[0] as i32;
            if count < 0 {
                return Err(87u32.into());
            }
            if count != 0 && a[1] != 0 {
                put(&mut image.memory, a[1], US_KEYBOARD_LAYOUT, width)?;
            }
            Ok(1)
        }
        "GetKeyboardLayoutNameA" | "GetKeyboardLayoutNameW" => {
            let bytes = if wide { b"0\x000\x000\x000\x000\x004\x000\x009\x00\x00\x00".as_slice() }
                else { b"00000409\0".as_slice() };
            if !image.memory.probe_write(a[0], bytes.len()) || !image.memory.write_virt(a[0], bytes) {
                return Err(NOACCESS.into());
            }
            Ok(1)
        }
        "GetDC" => {
            let hwnd = a[0];
            let thread = image.thread_id();
            let state = &mut image.win32.user;
            if hwnd != 0 && !state.windows.iter().any(|window| u64::from(window.hwnd) == hwnd) {
                return Ok(0);
            }
            if state.client_dcs.len() >= 256 || state.client_dcs.try_reserve(1).is_err() {
                return Ok(0);
            }
            let Some(handle) = services.create_memory_dc(0) else { return Ok(0); };
            state.client_dcs.push((handle, hwnd, thread));
            Ok(handle)
        }
        "ReleaseDC" => {
            let entry = (a[1], a[0], image.thread_id());
            let state = &mut image.win32.user;
            let Some(index) = state.client_dcs.iter().position(|candidate| *candidate == entry) else {
                return Ok(0);
            };
            if !services.delete_memory_dc(a[1]) {
                return Ok(0);
            }
            state.client_dcs.swap_remove(index);
            Ok(1)
        }
        "DrawTextExW" => text_layout::invoke(image, a, services),
        "GetForegroundWindow" => Ok(scalar(&send(services, packet(w::FOREGROUND_WINDOW, 0))?)),
        "SetForegroundWindow" => {
            let target = image.win32.user.windows.iter().find(|window| {
                u64::from(window.hwnd) == a[0] && window.parent == 0
            });
            let Some(target) = target else { return Ok(0); };
            Ok(scalar(&send(services, packet(w::ACTIVATE_WINDOW, target.hwnd))?))
        }
        "FindWindowW" => {
            let state = &image.win32.user;
            let class = match a[0] {
                0 => None,
                1..=0xffff => {
                    let Some(class) = state.classes.iter().find(|class| u64::from(class.atom) == a[0]) else {
                        return Ok(0);
                    };
                    Some(class.name.to_lowercase())
                }
                pointer => Some(string(&mut image.memory, pointer, true)?.to_lowercase()),
            };
            let title = if a[1] == 0 { None } else {
                Some(string(&mut image.memory, a[1], true)?.to_lowercase())
            };
            let found = state.windows.iter().rev().find(|window| {
                window.parent == 0
                    && class.as_ref().is_none_or(|class| &window.class.to_lowercase() == class)
                    && title.as_ref().is_none_or(|title| &window.title.to_lowercase() == title)
            });
            Ok(found.map_or(0, |window| u64::from(window.hwnd)))
        }
        "GetFocus" | "SetFocus" => {
            let thread = image.thread_id();
            let current_root = image.win32.local.user.focus_root;
            let previous_root = image.win32.user.windows.iter().rev().find(|window| {
                window.thread_id == thread && window.hwnd == current_root && window.parent == 0
            }).or_else(|| image.win32.user.windows.iter().rev().find(|window| {
                window.thread_id == thread && window.parent == 0
            })).map_or(0, |window| window.hwnd);
            if name == "GetFocus" {
                return if previous_root == 0 { Ok(0) }
                else { Ok(scalar(&send(services, packet(w::FOCUS, previous_root))?)) };
            }
            let target = a[0];
            let root = if target == 0 {
                previous_root
            } else {
                let window = image.win32.user.windows.iter().find(|window| {
                    u64::from(window.hwnd) == target && window.thread_id == thread
                }).ok_or(ApiError::Windows(1400))?;
                if window.parent == 0 { window.hwnd } else { window.parent }
            };
            if root == 0 { return Ok(0); }
            let mut request = packet(w::FOCUS, root);
            request.values[6] = target as u32;
            request.values[9] = 1;
            let previous = if previous_root != 0 && previous_root != root {
                Some(scalar(&send(services, packet(w::FOCUS, previous_root))?))
            } else { None };
            let replaced = scalar(&send(services, request)?);
            if previous.is_some() {
                let mut clear = packet(w::FOCUS, previous_root);
                clear.values[9] = 1;
                send(services, clear)?;
            }
            image.win32.local.user.focus_root = root;
            Ok(previous.unwrap_or(replaced))
        }
        "GetActiveWindow" | "SetActiveWindow" => {
            let thread = image.thread_id();
            let active = image.win32.local.user.active_window;
            let previous = image.win32.user.windows.iter()
                .find(|window| window.hwnd == active && window.thread_id == thread)
                .map_or(0, |window| u64::from(window.hwnd));
            if name == "GetActiveWindow" || a[0] == previous {
                return Ok(previous);
            }
            if a[0] != 0 {
                let target = image.win32.user.windows.iter().find(|window| {
                    u64::from(window.hwnd) == a[0] && window.thread_id == thread && window.parent == 0
                }).ok_or(ApiError::Windows(1400))?;
                send(services, packet(w::ACTIVATE_WINDOW, target.hwnd))?;
            }
            image.win32.local.user.active_window = a[0] as u32;
            Ok(previous)
        }
        "SetProcessDPIAware" => {
            image.win32.user.dpi_aware = true;
            Ok(1)
        }
        "IsProcessDPIAware" => Ok(u64::from(image.win32.user.dpi_aware)),
        "LoadStringW" => resources::load_string_w(image, a, services),
        "LoadImageW" | "LoadIconW" | "LoadCursorW" | "SetCursor" |
        "LoadAcceleratorsW" | "TranslateAcceleratorW" => resource_handles::invoke(image, name, a, services),
        "RegisterWindowMessageW" => {
            let text = string(&mut image.memory, a[0], true)?;
            if text.is_empty() || text.encode_utf16().count() > 255 {
                return Err(87u32.into());
            }
            let messages = &mut image.win32.user.registered_messages;
            if let Some(index) = messages.iter().position(|existing| existing.eq_ignore_ascii_case(&text)) {
                return Ok(0xc000 + index as u64);
            }
            if messages.len() == 0x4000 {
                return Err(8u32.into());
            }
            let identifier = 0xc000 + messages.len() as u64;
            messages.push(text);
            Ok(identifier)
        }
        "GetAncestor" => {
            let windows = &image.win32.user.windows;
            let lookup = |handle| windows.iter().find(|window| u64::from(window.hwnd) == handle);
            let mut window = lookup(a[0]).ok_or(1400u32)?;
            match a[1] as u32 {
                1 => Ok(u64::from(window.parent)),
                2 | 3 => {
                    while window.parent != 0 {
                        window = lookup(u64::from(window.parent)).ok_or(1400u32)?;
                    }
                    Ok(u64::from(window.hwnd))
                }
                _ => Err(87u32.into()),
            }
        }
        "GetWindowLongW" | "SetWindowLongW" => {
            let window = image.win32.user.windows.iter_mut().find(|entry| u64::from(entry.hwnd) == a[0])
                .ok_or(1400u32)?;
            let index = a[1] as i32;
            let set = name == "SetWindowLongW";
            let value = if set { a[2] as u32 } else { 0 };
            let result = match index {
                -4 if x86 && !set => window.procedure as u32,
                -6 if x86 && !set => window.instance as u32,
                -8 if !set => window.parent,
                -12 if !set => window.id,
                -16 if !set => window.style,
                -20 if !set => window.ex_style,
                -21 => {
                    if set { core::mem::replace(&mut window.user_data, value) } else { window.user_data }
                }
                0.. => {
                    let offset = index as usize;
                    let field = window.extra.get_mut(offset..offset + 4).ok_or(1413u32)?;
                    let old = u32::from_le_bytes(field.try_into().unwrap());
                    if set { field.copy_from_slice(&value.to_le_bytes()); }
                    old
                }
                -21..=-4 if set => return Err(120u32.into()),
                _ => return Err(1413u32.into()),
            };
            Ok(u64::from(result))
        }
        "GetSystemMetrics" | "GetSysColor" => {
            let action = if name == "GetSysColor" { w::SYSTEM_COLOR } else { w::SYSTEM_METRICS };
            let mut request = packet(action, 0);
            request.values[6] = a[0] as u32;
            let reply = send(services, request)?;
            Ok(reply.values[20] as u64)
        }
        "RegisterClassExW" | "RegisterClassExA" => {
            let p = a[0];
            if word(&mut image.memory, p, 4)? != if x86 { 48 } else { 80 } {
                return Err(87u32.into());
            }
            read(&mut image.memory, p, if x86 { 48 } else { 80 })?;
            let procedure = word(&mut image.memory, p + 8, width)?;
            let extra_size = word(&mut image.memory, p + if x86 { 16 } else { 20 }, 4)? as usize;
            let instance = word(&mut image.memory, p + if x86 { 20 } else { 24 }, width)?;
            let menu_pointer = word(&mut image.memory, p + if x86 { 36 } else { 56 }, width)?;
            let menu = match menu_pointer {
                0 => None,
                1..=0xffff => Some((instance, menu::Resource::Id(menu_pointer as u16))),
                pointer => Some((instance, menu::Resource::Name(string(&mut image.memory, pointer, wide)?))),
            };
            let pointer = word(&mut image.memory, p + if x86 { 40 } else { 64 }, width)?;
            let text = string(&mut image.memory, pointer, wide)?;
            if procedure == 0 || text.is_empty() || image.win32.user.classes.len() >= 256 || extra_size > 4096 {
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
                extra_size,
                atom,
                name: text,
                procedure,
                menu,
            });
            Ok(atom as u64)
        }
        "CreateWindowExW" | "CreateWindowExA" | "CreateStatusWindowW" => {
            if image.win32.user.windows.len() >= 256 {
                return Err(8u32.into());
            }
            let status_args;
            let create_status = name == "CreateStatusWindowW";
            let a = if create_status {
                let parent = image.win32.user.windows.iter().find(|window| u64::from(window.hwnd) == a[2])
                    .ok_or(ApiError::Windows(1400))?;
                let (width, height) = parent.client_dimensions();
                status_args = [0, 0, a[1], a[0] | 0x4000_0000, 0,
                    u64::from(height.saturating_sub(STATUS_HEIGHT)), u64::from(width),
                    u64::from(STATUS_HEIGHT), a[2], a[3], 0, 0];
                image.win32.user.common_classes |= STATUS_CLASS_FLAG;
                &status_args[..]
            } else {
                a
            };
            let class = if create_status {
                String::from(STATUS_CLASS)
            } else if a[1] <= 0xffff {
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
                "edit" => w::EDIT_CONTROL,
                STATUS_CLASS => w::STATUS_CONTROL,
                _ => 0,
            };
            let (procedure, extra_size, class_menu) = image
                .win32
                .user
                .classes
                .iter()
                .find(|c| c.name.eq_ignore_ascii_case(&class))
                .map_or((0, 0, None), |class| (class.procedure, class.extra_size, class.menu.clone()));
            if kind == 0 && procedure == 0 {
                return Err(1407u32.into());
            }
            let class_flag = match kind {
                1 => 1,
                2 => 2,
                3 => 4,
                w::STATUS_CONTROL => STATUS_CLASS_FLAG,
                _ => 0,
            };
            if class_flag != 0 && image.win32.user.common_classes & class_flag == 0 {
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
            let child = a[3] as u32 & 0x4000_0000 != 0;
            let mut menu_handle = 0;
            if !child && a[9] != 0 {
                if !image.win32.user.menus.contains(a[9]) {
                    return Err(1401u32.into());
                }
                menu_handle = a[9];
            } else if !child {
                if let Some((module, resource)) = class_menu {
                    let bytes = resources::menu_template(image, services, module, resource.as_name())?;
                    menu_handle = image.win32.user.menus.load(hwnd, &bytes)?;
                }
            }
            let mut p = packet(1, hwnd);
            p.values[6] = parent;
            p.values[7] = kind;
            p.values[18] = a[3] as u32;
            p.values[17] = if child { a[9] as u32 } else { 0 };
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
            if let Err(error) = copied_text(&mut p, &title).and_then(|()| send(services, p)) {
                image.win32.user.menus.release_window(hwnd);
                return Err(error);
            }
            image.win32.user.next += 1;
            let thread_id = image.thread_id();
            image.win32.user.windows.push(Window {
                thread_id,
                hwnd,
                parent,
                style: p.values[18],
                system_menu: None,
                menu: menu_handle,
                id: p.values[17],
                kind,
                procedure,
                class,
                title,
                width: p.values[15],
                height: p.values[16],
                wide,
                ex_style: a[0] as u32,
                instance: a[10],
                user_data: 0,
                extra: vec![0; extra_size],
                scroll_positions: [0; 2],
                paint_pending: false,
            });
            if procedure != 0 {
                let data=heap_allocate(image,if x86{48}else{80})?;
                let menu_or_id = if child { a[9] } else { menu_handle };
                let fields:Vec<(u64,u64,usize)>=if x86 {
                    vec![(0,a[11],4),(4,a[10],4),(8,menu_or_id,4),(12,a[8],4),(16,p.values[16]as u64,4),(20,p.values[15]as u64,4),(24,a[5],4),(28,a[4],4),(32,a[3],4),(36,a[2],4),(40,a[1],4),(44,a[0],4)]
                }else{
                    vec![(0,a[11],8),(8,a[10],8),(16,menu_or_id,8),(24,a[8],8),(32,p.values[16]as u64,4),(36,p.values[15]as u64,4),(40,a[5],4),(44,a[4],4),(48,a[3],4),(56,a[2],8),(64,a[1],8),(72,a[0],4)]
                };
                for(offset,value,size)in fields{put(&mut image.memory,data+offset,value,size)?;}
                image.win32.local.user.completion=Some(Completion::Create{hwnd,data,procedure,phase:0});
                image.win32.local.user.callback=Some((procedure,[hwnd as u64,0x81,0,data]));
            }
            Ok(hwnd as u64)
        }
        "PostQuitMessage" => {
            queue(
                image,
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
                image,
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
            receive_message(image, services, a, name.starts_with("Peek"))
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
                if procedure!=0{image.win32.local.user.completion=Some(Completion::FreeValue(pointer));}else{heap_free(image,pointer)?;}
            }
            if procedure != 0 {
                image.win32.local.user.callback = Some((procedure,[message.hwnd,message.message as u64,message.wp,message.lp]));
            }
            Ok(0)
        }
        "IsWindow" => Ok(u64::from(image.win32.user.windows.iter().any(|w| u64::from(w.hwnd) == a[0]))),
        "IsIconic" => {
            let Some(window) = image.win32.user.windows.iter().find(|w| u64::from(w.hwnd) == a[0]) else {
                return Ok(0);
            };
            if window.parent != 0 {
                return Ok(0);
            }
            Ok(u64::from(scalar(&send(services, packet(w::IS_ICONIC, window.hwnd))?) != 0))
        }
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
                    if image.win32.local.user.modals.iter().any(|modal| modal.owns(window.hwnd)) {
                        return dialog::end(image, a[0], 0);
                    }
                    send(services, packet(2, window.hwnd))?;
                    image.win32.user.remove_windows(window.hwnd);
                    if window.procedure != 0 {
                        image.win32.local.user.completion=Some(Completion::Fixed(1));
                        image.win32.local.user.callback = Some((window.procedure, [a[0], 2, 0, 0]));
                    }
                    Ok(1)
                }
                "GetDlgCtrlID" => Ok(if window.parent == 0 { 0 } else { u64::from(window.id) }),
                "GetDlgItem" | "GetDlgItemTextW" | "SendDlgItemMessageW" | "SetDlgItemTextW" => {
                    let Some(child) = image.win32.user.windows.iter().find(|child| {
                        child.parent == window.hwnd && child.id == a[1] as u32
                    }) else {
                        return Ok(0);
                    };
                    let hwnd = u64::from(child.hwnd);
                    if name == "GetDlgItem" { return Ok(hwnd); }
                    if name == "SetDlgItemTextW" {
                        return invoke(image, "SetWindowTextW", &[hwnd, a[2]], services);
                    }
                    if name == "SendDlgItemMessageW" {
                        return invoke(image, "SendMessageW", &[hwnd, a[2], a[3], a[4]], services);
                    }
                    let capacity = a[3] as i32;
                    if capacity <= 0 { return Ok(0); }
                    invoke(image, "SendMessageW", &[hwnd, 0xd, capacity as u64, a[2]], services)
                }
                "GetParent" => Ok(window.parent as u64),
                "GetWindowPlacement" | "SetWindowPlacement" => {
                    const SIZE: usize = 44;
                    let address = a[1];
                    if word(&mut image.memory, address, 4)? as usize != SIZE {
                        return Err(87u32.into());
                    }
                    if name == "SetWindowPlacement" {
                        let bytes = read(&mut image.memory, address, SIZE)?;
                        let mut request = packet(w::SET_WINDOW_PLACEMENT, window.hwnd);
                        for (field, bytes) in request.values[w::WINDOW_PLACEMENT_FIELDS]
                            .iter_mut().zip(bytes[4..].chunks_exact(4))
                        {
                            *field = u32::from_le_bytes(bytes.try_into().unwrap());
                        }
                        let response = send(services, request)?;
                        let window = image.win32.user.windows.iter_mut()
                            .find(|item| item.hwnd == window.hwnd).unwrap();
                        window.width = response.values[30];
                        window.height = response.values[31];
                    } else {
                        if !image.memory.probe_write(address, SIZE) {
                            return Err(NOACCESS.into());
                        }
                        let response = send(services, packet(w::GET_WINDOW_PLACEMENT, window.hwnd))?;
                        let mut bytes = [0; SIZE];
                        bytes[..4].copy_from_slice(&(SIZE as u32).to_le_bytes());
                        for (field, bytes) in response.values[w::WINDOW_PLACEMENT_FIELDS]
                            .iter().zip(bytes[4..].chunks_exact_mut(4))
                        {
                            bytes.copy_from_slice(&field.to_le_bytes());
                        }
                        if !image.memory.write_virt(address, &bytes) {
                            return Err(NOACCESS.into());
                        }
                    }
                    Ok(1)
                }
                "GetClientRect" => {
                    if !image.memory.probe_write(a[1], 16) {
                        return Err(87u32.into());
                    }
                    let (width, height) = window.client_dimensions();
                    for (i, v) in [0, 0, width, height].into_iter().enumerate() {
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
                "EnableWindow" => {
                    let mut request = packet(w::ENABLE_WINDOW, window.hwnd);
                    request.values[9] = u32::from(a[1] as u32 != 0);
                    Ok(scalar(&send(services, request)?))
                }
                "UpdateWindow" => {
                    if window.paint_pending && window.procedure != 0 {
                        if window.thread_id != image.thread_id() {
                            return Err(120u32.into());
                        }
                        if let Some(entry) = image.win32.user.windows.iter_mut().find(|entry| entry.hwnd == window.hwnd) {
                            entry.paint_pending = false;
                        }
                        image.win32.local.user.completion = Some(Completion::Fixed(1));
                        image.win32.local.user.callback = Some((window.procedure, [a[0], 0x000f, 0, 0]));
                    }
                    Ok(1)
                }
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
                    if window.kind == w::EDIT_CONTROL {
                        return control_message(image, &window, &[a[0], 0x000d, a[2], a[1]], wide, services);
                    }
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
                "GetWindowTextLengthW" => {
                    if window.kind == w::EDIT_CONTROL {
                        return control_message(image, &window, &[a[0], 0x000e, 0, 0], true, services);
                    }
                    Ok(window.title.encode_utf16().count() as u64)
                }
                "SetWindowPos" | "MoveWindow" => {
                    let index = if name == "MoveWindow" { 1 } else { 2 };
                    if name == "SetWindowPos" && a[6] & 3 != 0 {
                        return Err(ApiError::Unsupported);
                    }
                    let width = (a[index + 2] as i32).max(0) as u32;
                    let height = (a[index + 3] as i32).max(0) as u32;
                    let mut p = packet(7, window.hwnd);
                    p.values[13] = a[index] as u32;
                    p.values[14] = a[index + 1] as u32;
                    p.values[15] = width;
                    p.values[16] = height;
                    send(services, p)?;
                    if let Some(window) = image.win32.user.windows.iter_mut().find(|item| item.hwnd == window.hwnd) {
                        window.width = width;
                        window.height = height;
                    }
                    Ok(1)
                }
                "SendMessageW" | "SendMessageA" => {
                    if window.procedure != 0 && window.thread_id != image.thread_id() {
                        return Err(ApiError::Unsupported);
                    }
                    if window.procedure != 0 {
                        image.win32.local.user.callback =
                            Some((window.procedure, [a[0], a[1], a[2], a[3]]));
                        return Ok(0);
                    }
                    if window.kind != w::EDIT_CONTROL && name == "SendMessageW" && a[1] as u32 == 0xd {
                        let capacity = a[2] as usize;
                        if capacity == 0 { return Ok(0); }
                        let mut bytes: Vec<_> = window.title.encode_utf16().take(capacity - 1)
                            .flat_map(u16::to_le_bytes).collect();
                        let copied = bytes.len() / 2;
                        bytes.extend_from_slice(&0u16.to_le_bytes());
                        if !image.memory.probe_write(a[3], bytes.len())
                            || !image.memory.write_virt(a[3], &bytes) {
                            return Err(87u32.into());
                        }
                        return Ok(copied as u64);
                    }
                    control_message(image, &window, a, wide, services)
                }
                _ => Err(ApiError::Unsupported),
            }
        }
    }
}

pub(super) fn accept_file_drops(image: &mut GuestImage, hwnd: u64, accept: bool) -> Result<u64, ApiError> {
    let window = image.win32.user.windows.iter_mut()
        .find(|window| u64::from(window.hwnd) == hwnd)
        .ok_or(1400u32)?;
    const ACCEPT_FILES: u32 = 0x10;
    if accept { window.ex_style |= ACCEPT_FILES; } else { window.ex_style &= !ACCEPT_FILES; }
    Ok(0)
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
        w::STATUS_CONTROL => match msg {
            0x0005 => {
                let parent = image.win32.user.windows.iter().find(|entry| entry.hwnd == window.parent)
                    .ok_or(ApiError::Windows(1400))?;
                let (width, height) = parent.client_dimensions();
                invoke(image, "MoveWindow", &[a[0], 0, u64::from(height.saturating_sub(STATUS_HEIGHT)),
                    u64::from(width), u64::from(STATUS_HEIGHT), 1], services)?;
                return Ok(0);
            }
            0x000c | 0x0401 | 0x040b => {
                if msg != 0x000c && a[2] != 0 { return Err(ApiError::Unsupported); }
                let text = if a[3] == 0 { String::new() }
                    else { string(m, a[3], msg == 0x040b || msg == 0x000c && wide)? };
                let mut update = packet(6, window.hwnd);
                copied_text(&mut update, &text)?;
                send(services, update)?;
                if let Some(entry) = image.win32.user.windows.iter_mut().find(|entry| entry.hwnd == window.hwnd) {
                    entry.title = text;
                }
                return Ok(1);
            }
            0x000e => return Ok(window.title.encode_utf16().count() as u64),
            _ => return Err(ApiError::Unsupported),
        },
        w::EDIT_CONTROL => match msg {
            0x000c => {
                if a[3] != 0 { copied_text(&mut p, &string(m, a[3], wide)?)?; }
            }
            0x000d => {
                let capacity = a[2] as usize;
                if capacity == 0 { return Ok(0); }
                let reply = send(services, p)?;
                let text = reply.string(0).ok_or(ApiError::Windows(13))?;
                if wide {
                    let mut bytes: Vec<u8> = text.encode_utf16().take(capacity - 1)
                        .flat_map(u16::to_le_bytes).collect();
                    let count = bytes.len() / 2;
                    bytes.extend_from_slice(&[0, 0]);
                    if !m.probe_write(a[3], bytes.len()) || !m.write_virt(a[3], &bytes) {
                        return Err(NOACCESS.into());
                    }
                    return Ok(count as u64);
                }
                if !text.is_ascii() { return Err(ApiError::Unsupported); }
                let clipped = &text[..text.len().min(capacity - 1)];
                write_string(m, a[3], clipped, false)?;
                return Ok(clipped.len() as u64);
            }
            0x00b0 => {
                let (start_out, end_out) = (a[2], a[3]);
                for address in [start_out, end_out] {
                    if address != 0 && !m.probe_write(address, 4) {
                        return Err(NOACCESS.into());
                    }
                }
                p.values[9..13].fill(0);
                let reply = send(services, p)?;
                let (start, end) = (reply.values[22], reply.values[23]);
                for (address, index) in [(start_out, start), (end_out, end)] {
                    if address != 0 && !m.write_virt(address, &index.to_le_bytes()) {
                        return Err(NOACCESS.into());
                    }
                }
                return Ok(u64::from(if start > 0xffff || end > 0xffff {
                    u32::MAX
                } else {
                    (end << 16) | start
                }));
            }
            0x00c9 if a[2] as u32 == 0 => return Ok(0),
            0x000e | 0x00b8 | 0x00b9 | 0x00bb | 0x00cf => {}
            _ => return Err(ApiError::Unsupported),
        },
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
