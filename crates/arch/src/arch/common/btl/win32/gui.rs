//! Checked common-dialog ABI and image-list metadata for isolated guests.
//!
//! Modal requests cross a copied, pointer-free transport after validation.
use super::{put, read, string, word, write_string, ApiError, DecodeMode, GuestImage, GuestMemory};
use alloc::vec::Vec;
use super::dialogs::modal;

const CDERR_STRUCTSIZE: u32 = 1;
const CDERR_INITIALIZATION: u32 = 2;
const CDERR_DIALOGFAILURE: u32 = 0xffff;
const PDERR_NODEFAULTPRN: u32 = 0x1008;
const MAX_IMAGES: u32 = 65_536;

struct ImageList {
    handle: u64,
    width: i32,
    height: i32,
    count: u32,
}

pub(super) struct State {
    dialog_error: u32,
    language: u16,
    image_lists: Vec<ImageList>,
    next_handle: u64,
}
impl State {
    pub(super) fn new() -> Self {
        Self {
            dialog_error: 0,
            language: 0,
            image_lists: Vec::new(),
            next_handle: 0x4c00_0000,
        }
    }
}

fn structure_size(name: &str, x86: bool) -> (usize, Option<usize>) {
    let size = if name.starts_with("GetOpenFileName") || name.starts_with("GetSaveFileName") {
        return if x86 {
            (88, Some(76))
        } else {
            (152, Some(136))
        };
    } else if name.starts_with("ChooseFont") {
        if x86 {
            60
        } else {
            104
        }
    } else if name.starts_with("ChooseColor") {
        if x86 {
            36
        } else {
            72
        }
    } else if name.starts_with("PrintDlg") {
        if x86 {
            66
        } else {
            120
        }
    } else {
        if x86 {
            84
        } else {
            128
        }
    };
    (size, None)
}

fn dialog_error(memory: &mut dyn GuestMemory, name: &str, address: u64, x86: bool) -> u32 {
    if address == 0 {
        return CDERR_INITIALIZATION;
    }
    let Ok(size) = word(memory, address, 4) else {
        return CDERR_INITIALIZATION;
    };
    let (expected, legacy) = structure_size(name, x86);
    if size != expected as u64 && legacy != Some(size as usize) {
        return CDERR_STRUCTSIZE;
    }
    if read(memory, address, size as usize).is_err() || !memory.probe_write(address, size as usize)
    {
        return CDERR_INITIALIZATION;
    }
    let width = if x86 { 4 } else { 8 };
    if name.starts_with("GetOpenFileName") || name.starts_with("GetSaveFileName") {
        let (file_offset, capacity_offset) = if x86 { (28, 32) } else { (48, 56) };
        let Ok(file) = word(memory, address + file_offset, width) else {
            return CDERR_INITIALIZATION;
        };
        let Ok(capacity) = word(memory, address + capacity_offset, 4) else {
            return CDERR_INITIALIZATION;
        };
        if file == 0 || capacity == 0 {
            return CDERR_INITIALIZATION;
        }
        let bytes = capacity.checked_mul(if name.ends_with('W') { 2 } else { 1 });
        if !bytes
            .is_some_and(|n| n <= super::MAX_BUFFER as u64 && memory.probe_write(file, n as usize))
        {
            return CDERR_INITIALIZATION;
        }
    }
    if name.starts_with("ChooseFont") {
        let Ok(font) = word(memory, address + if x86 { 12 } else { 24 }, width) else {
            return CDERR_INITIALIZATION;
        };
        let bytes = if name.ends_with('W') { 92 } else { 60 };
        if font == 0 || read(memory, font, bytes).is_err() || !memory.probe_write(font, bytes) {
            return CDERR_INITIALIZATION;
        }
    }
    if name.starts_with("ChooseColor") {
        let Ok(colors) = word(memory, address + if x86 { 16 } else { 32 }, width) else {
            return CDERR_INITIALIZATION;
        };
        if colors == 0 || read(memory, colors, 64).is_err() || !memory.probe_write(colors, 64) {
            return CDERR_INITIALIZATION;
        }
    }
    if name.starts_with("PrintDlg") || name.starts_with("PageSetupDlg") {
        let flags_offset = if name.starts_with("PrintDlg") {
            if x86 {
                20
            } else {
                40
            }
        } else {
            if x86 {
                16
            } else {
                32
            }
        };
        let flags = word(memory, address + flags_offset, 4).unwrap_or(0);
        // Both PD_RETURNDEFAULT and PSD_RETURNDEFAULT use bit 0x400.
        let defaults = 0x400;
        if flags & defaults != 0 {
            if name.starts_with("PrintDlg") {
                let devmode = word(memory, address + if x86 { 8 } else { 16 }, width).unwrap_or(0);
                let devnames =
                    word(memory, address + if x86 { 12 } else { 24 }, width).unwrap_or(0);
                if devmode != 0 || devnames != 0 {
                    return 0x1003;
                } // PDERR_RETDEFFAILURE
            }
            return PDERR_NODEFAULTPRN;
        }
    }
    CDERR_DIALOGFAILURE
}

fn file_title(
    memory: &mut dyn GuestMemory,
    path: u64,
    output: u64,
    capacity: u16,
    wide: bool,
) -> u64 {
    let Ok(path) = string(memory, path, wide) else {
        return 0xffff;
    };
    if path.is_empty() || path.contains(['*', '?']) {
        return 0xffff;
    }
    let title = path.rsplit(['\\', '/']).next().unwrap_or("");
    if title.is_empty() || title.contains(':') {
        return 0xffff;
    }
    let needed = if wide {
        title.encode_utf16().count() + 1
    } else {
        title.len() + 1
    };
    if needed > i16::MAX as usize {
        return 0xffff;
    }
    if needed > capacity as usize {
        return needed as u64;
    }
    if output == 0 || write_string(memory, output, title, wide).is_err() {
        return 0xffff;
    }
    0
}

pub(super) fn invoke(
    image: &mut GuestImage,
    dll: &str,
    name: &str,
    a: &[u64],
    services: &mut dyn super::GuestServices,
) -> Result<u64, ApiError> {
    if dll == "comdlg32.dll" {
        return Ok(match name {
            "CommDlgExtendedError" => image.win32.gui.dialog_error as u64,
            "GetFileTitleA" | "GetFileTitleW" => file_title(
                &mut image.memory,
                a[0],
                a[1],
                a[2] as u16,
                name.ends_with('W'),
            ),
            _ => {
                let x86 = image.thread.context.mode == DecodeMode::X86;
                let error = dialog_error(&mut image.memory, name, a[0], x86);
                if error != CDERR_DIALOGFAILURE && !(error == PDERR_NODEFAULTPRN && services.printer_available()) { image.win32.gui.dialog_error = error; 0 }
                else { match modal(image, name, a[0], services) {
                    Ok(accepted) => {image.win32.gui.dialog_error = 0; u64::from(accepted)},
                    Err(error) => {image.win32.gui.dialog_error = error; 0}
                }}
            }
        });
    }
    let state = &mut image.win32.gui;
    Ok(match name {
        "InitCommonControls" => {image.win32.user.common_classes=0xffff;0}
        "InitCommonControlsEx" => {
            if word(&mut image.memory,a[0],4)?!=8{return Ok(0);}
            let classes=word(&mut image.memory,a[0]+4,4)? as u32;
            if classes&!0xffff!=0{return Ok(0);}
            image.win32.user.common_classes|=classes;1
        }
        "InitMUILanguage" => {
            state.language = a[0] as u16;
            0
        }
        "GetMUILanguage" => state.language as u64,
        "DllGetVersion" => {
            let Ok(size) = word(&mut image.memory, a[0], 4) else {
                return Ok(0x8007_0057);
            };
            if size != 20 && size != 32 {
                return Ok(0x8007_0057);
            }
            let mut data = [0u8; 32];
            data[..4].copy_from_slice(&(size as u32).to_le_bytes());
            data[4..8].copy_from_slice(&6u32.to_le_bytes());
            data[8..12].copy_from_slice(&10u32.to_le_bytes());
            data[12..16].copy_from_slice(&7601u32.to_le_bytes());
            data[16..20].copy_from_slice(&2u32.to_le_bytes());
            if size == 32 {
                let version = (6u64 << 48) | (10u64 << 32) | (7601u64 << 16) | 17514;
                data[24..32].copy_from_slice(&version.to_le_bytes());
            }
            if !image.memory.write_virt(a[0], &data[..size as usize]) {
                return Ok(0x8007_0057);
            }
            0
        }
        "ImageList_Create" => {
            let width = a[0] as u32 as i32;
            let height = a[1] as u32 as i32;
            if width <= 0
                || height <= 0
                || width > 32_768
                || height > 32_768
                || a[3] as u32 > MAX_IMAGES
                || a[4] as u32 > MAX_IMAGES
                || state.image_lists.len() >= 4096
                || a[2] as u32 & !0x0003_a0ff != 0
            {
                return Ok(0);
            }
            let handle = state.next_handle;
            state.next_handle += 1;
            state.image_lists.push(ImageList {
                handle,
                width,
                height,
                count: 0,
            });
            handle
        }
        "ImageList_Destroy" => {
            let Some(index) = state
                .image_lists
                .iter()
                .position(|list| list.handle == a[0])
            else {
                return Ok(0);
            };
            state.image_lists.remove(index);
            1
        }
        "ImageList_GetImageCount"
        | "ImageList_GetIconSize"
        | "ImageList_SetIconSize"
        | "ImageList_SetImageCount"
        | "ImageList_Remove" => {
            let Some(list) = state
                .image_lists
                .iter_mut()
                .find(|list| list.handle == a[0])
            else {
                return Ok(0);
            };
            match name {
                "ImageList_GetImageCount" => list.count as u64,
                "ImageList_GetIconSize" => {
                    if !image.memory.probe_write(a[1], 4) || !image.memory.probe_write(a[2], 4) {
                        return Ok(0);
                    }
                    put(&mut image.memory, a[1], list.width as u64, 4)?;
                    put(&mut image.memory, a[2], list.height as u64, 4)?;
                    1
                }
                "ImageList_SetIconSize" => {
                    let width = a[1] as u32 as i32;
                    let height = a[2] as u32 as i32;
                    if width <= 0 || height <= 0 || width > 32_768 || height > 32_768 {
                        return Ok(0);
                    }
                    list.width = width;
                    list.height = height;
                    list.count = 0;
                    1
                }
                "ImageList_SetImageCount" => {
                    let count = a[1] as u32;
                    if count > MAX_IMAGES {
                        return Ok(0);
                    }
                    list.count = count;
                    1
                }
                _ => {
                    let index = a[1] as u32;
                    if index == u32::MAX {
                        list.count = 0;
                    } else if index < list.count {
                        list.count -= 1;
                    } else {
                        return Ok(0);
                    }
                    1
                }
            }
        }
        // Registering a class requires USER state and cannot be simulated by
        // returning TRUE here. Rendering and bitmap APIs use the same boundary.
        _ => return Err(ApiError::Unsupported),
    })
}
