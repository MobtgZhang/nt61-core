use super::*;

const RT_DIALOG: u16 = 5;
const WS_VISIBLE: u32 = 0x1000_0000;
const WS_TABSTOP: u32 = 0x0001_0000;
const WS_DISABLED: u32 = 0x0800_0000;
const DS_SETFONT: u32 = 0x40;
const WM_INITDIALOG: u64 = 0x110;
const WM_COMMAND: u64 = 0x111;
const WM_DESTROY: u64 = 2;
const WM_NCDESTROY: u64 = 0x82;
const ERROR_INVALID_WINDOW_HANDLE: u32 = 1400;
const ERROR_INVALID_DATA: u32 = 13;
const ERROR_NOT_SUPPORTED: u32 = 50;
pub(in super::super) const GATEWAY: u32 = 0xfffd;

struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn take<const N: usize>(&mut self) -> Result<[u8; N], ApiError> {
        let (field, rest) = self.0.split_first_chunk().ok_or(ERROR_INVALID_DATA)?;
        self.0 = rest;
        Ok(*field)
    }

    fn skip(&mut self, count: usize) -> Result<(), ApiError> {
        self.0 = self.0.get(count..).ok_or(ERROR_INVALID_DATA)?;
        Ok(())
    }

    fn short(&mut self) -> Result<u16, ApiError> {
        Ok(u16::from_le_bytes(self.take()?))
    }

    fn dword(&mut self) -> Result<u32, ApiError> {
        Ok(u32::from_le_bytes(self.take()?))
    }

    fn rect(&mut self) -> Result<[i16; 4], ApiError> {
        let mut rect = [0; 4];
        for unit in &mut rect {
            *unit = self.short()? as i16;
        }
        if rect[2] < 0 || rect[3] < 0 {
            return Err(ERROR_INVALID_DATA.into());
        }
        Ok(rect)
    }

    fn field(&mut self) -> Result<Field, ApiError> {
        let first = self.short()?;
        if first == 0xffff {
            return Ok(Field::Ordinal(self.short()?));
        }
        let mut text = Vec::new();
        let mut unit = first;
        while unit != 0 {
            if text.len() == 1023 {
                return Err(ERROR_INVALID_DATA.into());
            }
            text.push(unit);
            unit = self.short()?;
        }
        Ok(Field::Text(String::from_utf16_lossy(&text)))
    }
}

enum Field {
    Ordinal(u16),
    Text(String),
}

impl Field {
    fn text(self) -> Result<String, ApiError> {
        match self {
            Self::Text(text) => Ok(text),
            Self::Ordinal(_) => Err(ERROR_NOT_SUPPORTED.into()),
        }
    }

    fn control(&self) -> Result<u32, ApiError> {
        let class = match self {
            Self::Ordinal(0x80) => "button",
            Self::Ordinal(0x81) => "edit",
            Self::Ordinal(0x82) => "static",
            Self::Ordinal(0x85) => "combobox",
            Self::Ordinal(_) => return Err(ERROR_NOT_SUPPORTED.into()),
            Self::Text(text) => text,
        };
        Ok(match class.to_ascii_lowercase().as_str() {
            "button" => 6,
            "edit" => w::EDIT_CONTROL,
            "static" => 5,
            "combobox" => 4,
            _ => return Err(ERROR_NOT_SUPPORTED.into()),
        })
    }
}

struct Control {
    id: u32,
    kind: u32,
    style: u32,
    rect: [i16; 4],
    title: String,
}

struct Template {
    style: u32,
    rect: [i16; 4],
    title: String,
    controls: Vec<Control>,
}

impl Template {
    fn parse(bytes: &[u8]) -> Result<Self, ApiError> {
        let mut reader = Reader(bytes);
        let extended = bytes.get(2..4) == Some(&[0xff, 0xff]);
        let style = if extended {
            if reader.short()? != 1 {
                return Err(ERROR_INVALID_DATA.into());
            }
            reader.skip(10)?;
            reader.dword()?
        } else {
            let style = reader.dword()?;
            reader.skip(4)?;
            style
        };
        let count = usize::from(reader.short()?);
        let rect = reader.rect()?;
        let menu = reader.field()?.text()?;
        let class = reader.field()?.text()?;
        if !menu.is_empty() || !(class.is_empty() || class == "#32770") {
            return Err(ERROR_NOT_SUPPORTED.into());
        }
        let title = reader.field()?.text()?;
        if style & DS_SETFONT != 0 {
            reader.skip(if extended { 6 } else { 2 })?;
            reader.field()?.text()?;
        }
        if count >= 256 {
            return Err(ERROR_INVALID_DATA.into());
        }
        let mut controls = Vec::with_capacity(count);
        for _ in 0..count {
            let offset = bytes.len() - reader.0.len();
            reader.skip(offset.next_multiple_of(4) - offset)?;
            let style = if extended {
                reader.skip(8)?;
                reader.dword()?
            } else {
                let style = reader.dword()?;
                reader.skip(4)?;
                style
            };
            let rect = reader.rect()?;
            let id = if extended {
                reader.dword()?
            } else {
                u32::from(reader.short()?)
            };
            let kind = reader.field()?.control()?;
            let title = reader.field()?.text()?;
            if reader.short()? != 0 {
                return Err(ERROR_NOT_SUPPORTED.into());
            }
            controls.push(Control {
                id,
                kind,
                style,
                rect,
                title,
            });
        }
        Ok(Self {
            style,
            rect,
            title,
            controls,
        })
    }
}

#[derive(Clone, Copy)]
pub(super) struct Modal {
    hwnd: u32,
    owner: u32,
    restore_owner: bool,
    message: u64,
    return_pc: u64,
    result: Option<u64>,
}

impl Modal {
    pub(super) fn owns(&self, hwnd: u32) -> bool {
        self.hwnd == hwnd
    }
}

fn create_window(
    image: &mut GuestImage,
    services: &mut dyn GuestServices,
    mut request: w::Packet,
    title: String,
    procedure: u64,
) -> Result<u32, ApiError> {
    let hwnd = image.win32.user.next;
    let next = hwnd.checked_add(1).ok_or(8u32)?;
    request.values[5] = hwnd;
    copied_text(&mut request, &title)?;
    send(services, request)?;
    image.win32.user.next = next;
    let thread_id = image.thread_id();
    image.win32.user.windows.push(Window {
        thread_id,
        hwnd,
        parent: request.values[6],
        style: request.values[18],
        system_menu: None,
        menu: 0,
        id: request.values[17],
        kind: request.values[7],
        procedure,
        class: if request.values[6] == 0 { "#32770".into() } else { String::new() },
        title,
        width: request.values[15],
        height: request.values[16],
        wide: true,
        ex_style: 0,
        instance: 0,
        user_data: 0,
        extra: Vec::new(),
        scroll_positions: [0; 2],
        paint_pending: false,
    });
    Ok(hwnd)
}

fn window_request(parent: u32, kind: u32, id: u32, style: u32, rect: [i16; 4]) -> w::Packet {
    let mut request = packet(1, 0);
    request.values[6] = parent;
    request.values[7] = kind;
    request.values[17] = id;
    request.values[18] = style;
    for (index, unit) in rect.into_iter().enumerate() {
        let pixels = if index % 2 == 0 {
            i32::from(unit) * 7 / 4
        } else {
            i32::from(unit) * 15 / 8
        };
        request.values[13 + index] = pixels as u32;
    }
    request
}

fn remove(
    image: &mut GuestImage,
    services: &mut dyn GuestServices,
    hwnd: u32,
) -> Result<(), ApiError> {
    send(services, packet(2, hwnd))?;
    let state = &mut image.win32.user;
    state.messages.retain(|(_, message)| {
        !state.windows.iter().any(|window| {
            u64::from(window.hwnd) == message.hwnd && (window.hwnd == hwnd || window.parent == hwnd)
        })
    });
    state.remove_windows(hwnd);
    Ok(())
}

fn trampoline(image: &mut GuestImage) -> Result<u64, ApiError> {
    if let Some(address) = image.win32.user.dialog_trampoline {
        return Ok(address);
    }
    let address = allocate(image, 0, 16, READ | WRITE | EXECUTE)?;
    let mut code = [0; 7];
    code[0] = 0xb8;
    code[1..5].copy_from_slice(&(catalog::GATEWAY_BASE | GATEWAY).to_le_bytes());
    code[5..].copy_from_slice(if image.thread.context.mode == DecodeMode::X86 {
        &[0xcd, 0x2e]
    } else {
        &[0x0f, 0x05]
    });
    if !image.memory.write_virt(address, &code) {
        return Err(NOACCESS.into());
    }
    image.win32.user.dialog_trampoline = Some(address);
    Ok(address)
}

pub(super) fn create(
    image: &mut GuestImage,
    services: &mut dyn GuestServices,
    args: &[u64],
    modal: bool,
) -> Result<u64, ApiError> {
    let owner = args[2] as u32;
    let procedure = args[3];
    let thread = image.thread_id();
    if procedure == 0
        || owner != 0
            && !image.win32.user.windows.iter().any(|window| {
                window.hwnd == owner && window.parent == 0 && window.thread_id == thread
            })
    {
        return Err(ERROR_INVALID_WINDOW_HANDLE.into());
    }
    let template = Template::parse(&resources::template(
        image, services, args[0], RT_DIALOG, args[1], 65536,
    )?)?;
    if image.win32.user.windows.len() + template.controls.len() + 1 > 256 {
        return Err(8u32.into());
    }
    let message = if modal {
        trampoline(image)?;
        heap_allocate(image, 48)?
    } else {
        0
    };
    let mut request = window_request(0, 0, 0, template.style & !WS_VISIBLE, template.rect);
    request.values[23] = 1;
    let root = create_window(image, services, request, template.title, procedure);
    let hwnd = match root {
        Ok(hwnd) => hwnd,
        Err(error) => {
            if modal {
                heap_free(image, message)?;
            }
            return Err(error);
        }
    };
    let mut focus = 0;
    for control in template.controls {
        let request = window_request(hwnd, control.kind, control.id, control.style, control.rect);
        match create_window(image, services, request, control.title, 0) {
            Ok(child)
                if focus == 0
                    && control.style & (WS_TABSTOP | WS_VISIBLE | WS_DISABLED)
                        == (WS_TABSTOP | WS_VISIBLE) =>
            {
                focus = child
            }
            Ok(_) => {}
            Err(error) => {
                let _ = remove(image, services, hwnd);
                if modal {
                    heap_free(image, message)?;
                }
                return Err(error);
            }
        }
    }
    if modal {
        let restore_owner = if owner == 0 {
            false
        } else {
            match invoke(image, "EnableWindow", &[u64::from(owner), 0], services) {
                Ok(was_disabled) => was_disabled == 0,
                Err(error) => {
                    let _ = remove(image, services, hwnd);
                    heap_free(image, message)?;
                    return Err(error);
                }
            }
        };
        image.win32.local.user.modals.push(Modal {
            hwnd,
            owner,
            restore_owner,
            message,
            return_pc: image.thread.context.rip,
            result: None,
        });
    }
    image.win32.local.user.completion = Some(Completion::DialogInit {
        hwnd,
        focus,
        show: modal || template.style & WS_VISIBLE != 0,
        modal,
    });
    image.win32.local.user.callback = Some((
        procedure,
        [u64::from(hwnd), WM_INITDIALOG, u64::from(focus), args[4]],
    ));
    Ok(u64::from(hwnd))
}

pub(super) fn end(image: &mut GuestImage, hwnd: u64, result: u64) -> Result<u64, ApiError> {
    let modal = image
        .win32
        .local
        .user
        .modals
        .iter_mut()
        .rev()
        .find(|modal| u64::from(modal.hwnd) == hwnd)
        .ok_or(ERROR_INVALID_WINDOW_HANDLE)?;
    modal.result.get_or_insert(result);
    Ok(1)
}

pub(super) fn initialized(
    image: &mut GuestImage,
    services: &mut dyn GuestServices,
    hwnd: u32,
    focus: u32,
    show: bool,
    modal: bool,
    default_focus: bool,
) -> Result<Outcome, ApiError> {
    let exists = image
        .win32
        .user
        .windows
        .iter()
        .any(|window| window.hwnd == hwnd);
    let ended = modal
        && image
            .win32
            .local
            .user
            .modals
            .last()
            .is_some_and(|current| current.result.is_some());
    if exists && !ended {
        if show {
            invoke(image, "ShowWindow", &[u64::from(hwnd), 5], services)?;
            if modal {
                invoke(image, "SetActiveWindow", &[u64::from(hwnd)], services)?;
            }
        }
        if default_focus && focus != 0 {
            invoke(image, "SetFocus", &[u64::from(focus)], services)?;
        }
    }
    if modal {
        resume(image)
    } else {
        Ok(Outcome::Return(if exists { u64::from(hwnd) } else { 0 }))
    }
}

pub(super) fn resume(image: &mut GuestImage) -> Result<Outcome, ApiError> {
    image.thread.context.rip = image
        .win32
        .user
        .dialog_trampoline
        .ok_or(ERROR_INVALID_DATA)?;
    Ok(Outcome::Continue)
}

pub(in super::super) fn poll(
    image: &mut GuestImage,
    services: &mut dyn GuestServices,
) -> Result<Outcome, StopReason> {
    let pc = image.thread.context.rip;
    poll_inner(image, services).map_err(|_| StopReason::DataFault { pc })
}

fn poll_inner(
    image: &mut GuestImage,
    services: &mut dyn GuestServices,
) -> Result<Outcome, ApiError> {
    let modal = image
        .win32
        .local
        .user
        .modals
        .last()
        .copied()
        .ok_or(ERROR_INVALID_DATA)?;
    let hwnd = u64::from(modal.hwnd);
    let procedure = image
        .win32
        .user
        .windows
        .iter()
        .find(|window| window.hwnd == modal.hwnd)
        .map(|window| window.procedure);
    if let Some(procedure) = procedure {
        if modal.result.is_some() {
            image.win32.local.user.completion = Some(Completion::DialogDestroy {
                hwnd: modal.hwnd,
                procedure,
                final_message: false,
            });
            enter_callback(image, procedure, [hwnd, WM_DESTROY, 0, 0])?;
            return Ok(Outcome::Continue);
        }
        let polled = invoke(image, "GetMessageW", &[modal.message, hwnd, 0, 0], services);
        match polled {
            Err(ApiError::Retry) => return Ok(Outcome::Retry),
            Ok(0) => {
                let message = message_read(image, modal.message)?;
                queue(image, message)?;
                end(image, hwnd, 0)?;
                return resume(image);
            }
            Ok(_) => {}
            Err(error) => return Err(error),
        }
        let message = message_read(image, modal.message)?;
        let notifications = &mut image.win32.user.notifications;
        let notification = notifications
            .iter()
            .position(|pointer| *pointer == message.lp)
            .map(|index| notifications.remove(index));
        let cancel = message.message == 0x102 && message.wp == 27;
        image.win32.local.user.completion = Some(Completion::DialogMessage {
            close: message.message == 0x10 || cancel,
            notification,
        });
        let args = if cancel {
            [hwnd, WM_COMMAND, 2, 0]
        } else {
            [hwnd, u64::from(message.message), message.wp, message.lp]
        };
        enter_callback(image, procedure, args)?;
        Ok(Outcome::Continue)
    } else {
        finish(image, services)
    }
}

pub(super) fn message_complete(
    image: &mut GuestImage,
    close: bool,
    handled: bool,
) -> Result<Outcome, ApiError> {
    if close && !handled {
        let hwnd = image
            .win32
            .local
            .user
            .modals
            .last()
            .ok_or(ERROR_INVALID_DATA)?
            .hwnd;
        end(image, u64::from(hwnd), 2)?;
    }
    resume(image)
}

pub(super) fn destroy_complete(
    image: &mut GuestImage,
    services: &mut dyn GuestServices,
    hwnd: u32,
    procedure: u64,
    final_message: bool,
) -> Result<Outcome, ApiError> {
    if final_message {
        remove(image, services, hwnd)?;
        finish(image, services)
    } else {
        image.win32.local.user.completion = Some(Completion::DialogDestroy {
            hwnd,
            procedure,
            final_message: true,
        });
        enter_callback(image, procedure, [u64::from(hwnd), WM_NCDESTROY, 0, 0])?;
        Ok(Outcome::Continue)
    }
}

fn finish(image: &mut GuestImage, services: &mut dyn GuestServices) -> Result<Outcome, ApiError> {
    let modal = image
        .win32
        .local
        .user
        .modals
        .pop()
        .ok_or(ERROR_INVALID_DATA)?;
    heap_free(image, modal.message)?;
    if modal.owner != 0
        && image
            .win32
            .user
            .windows
            .iter()
            .any(|window| window.hwnd == modal.owner)
    {
        if modal.restore_owner {
            invoke(
                image,
                "EnableWindow",
                &[u64::from(modal.owner), 1],
                services,
            )?;
            let thread = &mut image.win32.local.user;
            thread.focus_root = modal.owner;
            invoke(
                image,
                "SetActiveWindow",
                &[u64::from(modal.owner)],
                services,
            )?;
        }
    }
    image.thread.context.rip = modal.return_pc;
    Ok(Outcome::Return(modal.result.unwrap_or(0)))
}
