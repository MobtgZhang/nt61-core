use super::{packet, send, w, ApiError, GuestServices, State};

const WS_SYSMENU: u32 = 0x0008_0000;
const WS_CHILD: u32 = 0x4000_0000;
const WS_THICKFRAME: u32 = 0x0004_0000;
const WS_MINIMIZEBOX: u32 = 0x0002_0000;
const WS_MAXIMIZEBOX: u32 = 0x0001_0000;
const BY_POSITION: u32 = 0x400;
const DISABLED: u32 = 0x3;
const CHECKED: u32 = 0x8;
const CLOSE: u32 = 0xf060;
const COMMANDS: [u32; 7] = [0xf120, 0xf010, 0xf000, 0xf020, 0xf030, 0, CLOSE];

#[derive(Clone)]
pub(super) struct Menu {
    handle: u32,
    states: [u32; COMMANDS.len()],
}

impl Menu {
    fn new(handle: u32, style: u32) -> Self {
        Self {
            handle,
            states: [
                DISABLED,
                0,
                if style & WS_THICKFRAME == 0 {
                    DISABLED
                } else {
                    0
                },
                if style & WS_MINIMIZEBOX == 0 {
                    DISABLED
                } else {
                    0
                },
                if style & WS_MAXIMIZEBOX == 0 {
                    DISABLED
                } else {
                    0
                },
                0x800,
                0,
            ],
        }
    }
}

fn set_close_enabled(
    services: &mut dyn GuestServices,
    hwnd: u32,
    enabled: bool,
) -> Result<(), ApiError> {
    let mut request = packet(w::SET_SYSTEM_CLOSE_ENABLED, hwnd);
    request.values[9] = u32::from(enabled);
    send(services, request)?;
    Ok(())
}

pub(super) fn invoke(
    state: &mut State,
    name: &str,
    arguments: &[u64],
    services: &mut dyn GuestServices,
) -> Result<u64, ApiError> {
    if name == "GetSystemMenu" {
        let window = state
            .windows
            .iter_mut()
            .find(|window| u64::from(window.hwnd) == arguments[0])
            .ok_or(1400u32)?;
        if window.style & (WS_SYSMENU | WS_CHILD) != WS_SYSMENU || window.parent != 0 {
            return Ok(0);
        }
        if arguments[1] as u32 != 0 {
            if window
                .system_menu
                .as_ref()
                .is_some_and(|menu| menu.states[COMMANDS.len() - 1] & DISABLED != 0)
            {
                set_close_enabled(services, window.hwnd, true)?;
            }
            window.system_menu = None;
            return Ok(0);
        }
        if let Some(menu) = &window.system_menu {
            return Ok(u64::from(menu.handle));
        }
        let handle = state.next_system_menu;
        state.next_system_menu = handle.checked_add(1).ok_or(8u32)?;
        window.system_menu = Some(Menu::new(handle, window.style));
        return Ok(u64::from(handle));
    }

    let (hwnd, menu) = state
        .windows
        .iter_mut()
        .find_map(|window| {
            window
                .system_menu
                .as_mut()
                .filter(|menu| u64::from(menu.handle) == arguments[0])
                .map(|menu| (window.hwnd, menu))
        })
        .ok_or(1401u32)?;
    let item = arguments[1] as u32;
    let flags = arguments[2] as u32;
    let index = if flags & BY_POSITION != 0 {
        ((item as usize) < COMMANDS.len()).then_some(item as usize)
    } else {
        COMMANDS
            .iter()
            .position(|command| *command == item && item != 0)
    }
    .ok_or(1456u32)?;
    let old = menu.states[index];
    let mask = match name {
        "GetMenuState" => return Ok(u64::from(old)),
        "EnableMenuItem" => DISABLED,
        "CheckMenuItem" => CHECKED,
        _ => return Err(ApiError::Unsupported),
    };
    let new = (old & !mask) | (flags & mask);
    if COMMANDS[index] == CLOSE && (new ^ old) & DISABLED != 0 {
        set_close_enabled(services, hwnd, new & DISABLED == 0)?;
    }
    menu.states[index] = new;
    Ok(u64::from(old & mask))
}
