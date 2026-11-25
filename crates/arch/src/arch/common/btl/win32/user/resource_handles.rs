use super::*;

const MAX_HANDLES: usize = 256;
const IMAGE_BASE: u64 = 0x6100_0000;
const ACCELERATOR_BASE: u64 = 0x6200_0000;
const LR_DEFAULTSIZE: u32 = 0x0040;
const LR_SHARED: u32 = 0x8000;

struct Image {
    module: u64,
    cursor: bool,
    shared: bool,
    stock_id: Option<u16>,
    frame: Option<resources::ImageFrame>,
}

#[derive(Clone, Copy)]
struct Accelerator {
    flags: u16,
    key: u16,
    command: u16,
}

#[derive(Default)]
pub(super) struct State {
    images: Vec<Image>,
    accelerators: Vec<Vec<Accelerator>>,
    cursor: u64,
}

fn stock_image(item: u64, cursor: bool) -> Option<u16> {
    let id = u16::try_from(item).ok()?;
    let known = if cursor {
        matches!(id, 32512..=32516 | 32640..=32646 | 32648..=32651 | 32671 | 32672)
    } else {
        matches!(id, 32512..=32518)
    };
    known.then_some(id)
}

fn load_image(
    image: &mut GuestImage,
    services: &mut dyn GuestServices,
    module: u64,
    item: u64,
    kind: u32,
    dimensions: (u64, u64),
    flags: u32,
) -> Result<u64, ApiError> {
    if !matches!(kind, 1 | 2) || flags & !(LR_DEFAULTSIZE | LR_SHARED) != 0 {
        return Err(120u32.into());
    }
    let dimension = |value| -> Result<u16, ApiError> {
        let value = u16::try_from(value).map_err(|_| 87u32)?;
        Ok(if value == 0 && flags & LR_DEFAULTSIZE != 0 { 32 } else { value })
    };
    let desired = (dimension(dimensions.0)?, dimension(dimensions.1)?);
    let cursor = kind == 2;
    let stock_id = if module == 0 { Some(stock_image(item, cursor).ok_or(1814u32)?) } else { None };
    let frame = if stock_id.is_none() {
        Some(resources::image_frame(image, services, module, item, cursor, desired)?)
    } else {
        None
    };
    let images = &mut image.win32.user.resources.images;
    let shared = stock_id.is_some() || flags & LR_SHARED != 0;
    if shared {
        if let Some(index) = images.iter().position(|loaded| {
            loaded.shared && loaded.module == module && loaded.cursor == cursor && loaded.stock_id == stock_id
                && match (&loaded.frame, &frame) {
                    (None, None) => true,
                    (Some(left), Some(right)) => left.width == right.width
                        && left.height == right.height && left.bytes == right.bytes,
                    _ => false,
                }
        }) {
            return Ok(IMAGE_BASE + index as u64);
        }
    }
    if images.len() == MAX_HANDLES {
        return Err(1816u32.into());
    }
    let handle = IMAGE_BASE + images.len() as u64;
    images.push(Image { module, cursor, shared, stock_id, frame });
    Ok(handle)
}

fn accelerators(bytes: &[u8]) -> Result<Vec<Accelerator>, ApiError> {
    if bytes.is_empty() || bytes.len() % 8 != 0 {
        return Err(13u32.into());
    }
    let mut table = Vec::new();
    let rows = bytes.len() / 8;
    for (index, row) in bytes.chunks_exact(8).enumerate() {
        let flags = u16::from_le_bytes([row[0], row[1]]);
        if flags & !0x9f != 0 || (flags & 0x80 != 0) != (index + 1 == rows) {
            return Err(13u32.into());
        }
        table.push(Accelerator {
            flags,
            key: u16::from_le_bytes([row[2], row[3]]),
            command: u16::from_le_bytes([row[4], row[5]]),
        });
    }
    Ok(table)
}

pub(super) fn invoke(
    image: &mut GuestImage, name: &str, args: &[u64], services: &mut dyn GuestServices,
) -> Result<u64, ApiError> {
    match name {
        "LoadImageW" => load_image(image, services, args[0], args[1], args[2] as u32,
            (args[3] as u32 as u64, args[4] as u32 as u64), args[5] as u32),
        "LoadIconW" | "LoadCursorW" => load_image(image, services, args[0], args[1],
            if name == "LoadIconW" { 1 } else { 2 }, (0, 0), LR_DEFAULTSIZE | LR_SHARED),
        "SetCursor" => {
            let state = &mut image.win32.user.resources;
            if args[0] != 0 && !args[0].checked_sub(IMAGE_BASE)
                .and_then(|handle| state.images.get(handle as usize)).is_some_and(|entry| entry.cursor)
            {
                return Err(1402u32.into());
            }
            Ok(core::mem::replace(&mut state.cursor, args[0]))
        }
        "LoadAcceleratorsW" => {
            let bytes = resources::template(image, services, args[0], 9, args[1], 8 * 512)?;
            let table = accelerators(&bytes)?;
            let tables = &mut image.win32.user.resources.accelerators;
            if tables.len() == MAX_HANDLES {
                return Err(1816u32.into());
            }
            let handle = ACCELERATOR_BASE + tables.len() as u64;
            tables.push(table);
            Ok(handle)
        }
        "TranslateAcceleratorW" => {
            let message = message_read(image, args[2])?;
            let procedure = image.win32.user.windows.iter().find(|window| {
                u64::from(window.hwnd) == args[0] && window.thread_id == image.thread_id()
            }).ok_or(1400u32)?.procedure;
            let table = args[1].checked_sub(ACCELERATOR_BASE)
                .and_then(|handle| image.win32.user.resources.accelerators.get(handle as usize))
                .ok_or(1403u32)?;
            let virtual_key = matches!(message.message, 0x0100 | 0x0104);
            if procedure == 0 || !(virtual_key || matches!(message.message, 0x0102 | 0x0106)) {
                return Ok(0);
            }
            let alt = message.lp & (1 << 29) != 0;
            let entry = table.iter().find(|entry| {
                (entry.flags & 1 != 0) == virtual_key && alt == (entry.flags & 0x10 != 0)
                    && entry.flags & 0x0c == 0 && message.wp == u64::from(entry.key)
            });
            if let Some(entry) = entry {
                image.win32.local.user.completion = Some(Completion::Fixed(1));
                image.win32.local.user.callback = Some((procedure,
                    [args[0], 0x0111, 0x1_0000 | u64::from(entry.command), 0]));
                Ok(1)
            } else {
                Ok(0)
            }
        }
        _ => Err(ApiError::Unsupported),
    }
}
