//! Private copied protocol between isolated PE guests and the desktop broker.
//! These are not Windows structures. No member contains a process address.
pub const POLL: u32 = 14;
pub const COMPLETE: u32 = 15;
pub const COMMAND_START: u32 = 16;
pub const COMMAND_POLL: u32 = 17;
pub const PRINT_WRITE: u32 = 18;
pub const OPEN: u32 = 1;
pub const SAVE: u32 = 2;
pub const FONT: u32 = 3;
pub const COLOR: u32 = 4;
pub const PRINT: u32 = 5;
pub const PAGE: u32 = 6;
pub const WINDOW: u32 = 7;
pub const PENDING: u32 = 0;
pub const CANCELLED: u32 = 1;
pub const ACCEPTED: u32 = 2;
pub const FAILED: u32 = 3;
/// values: kind, result, extended error, flags; then operation-specific values.
/// text: filename/directory, title, font/filter, default extension.
/// Strings are terminated UTF-8. The filter field is a double-NUL string list.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Packet {
    pub values: [u32; 64],
    pub text: [[u8; 1024]; 4],
}
pub const EMPTY: Packet = Packet { values: [0; 64], text: [[0; 1024]; 4] };
pub const PACKET_SIZE: usize = 4352;
const _: () = assert!(core::mem::size_of::<Packet>() == PACKET_SIZE);

impl Packet {
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != PACKET_SIZE { return None; }
        let mut packet = EMPTY;
        for (value, bytes) in packet.values.iter_mut().zip(bytes[..256].chunks_exact(4)) {
            *value = u32::from_le_bytes(bytes.try_into().ok()?);
        }
        for (text, bytes) in packet.text.iter_mut().zip(bytes[256..].chunks_exact(1024)) { text.copy_from_slice(bytes); }
        Some(packet)
    }
    pub fn encode(&self, bytes: &mut [u8]) -> bool {
        if bytes.len() != PACKET_SIZE { return false; }
        for (value, bytes) in self.values.iter().zip(bytes[..256].chunks_exact_mut(4)) { bytes.copy_from_slice(&value.to_le_bytes()); }
        for (text, bytes) in self.text.iter().zip(bytes[256..].chunks_exact_mut(1024)) { bytes.copy_from_slice(text); }
        true
    }
    pub fn string(&self, index: usize) -> Option<&str> {
        let bytes = self.text.get(index)?;
        core::str::from_utf8(&bytes[..bytes.iter().position(|b| *b == 0)?]).ok()
    }
    pub fn set_string(&mut self, index: usize, text: &str) -> bool {
        let Some(bytes) = self.text.get_mut(index) else { return false; };
        if text.len() >= bytes.len() || text.contains('\0') { return false; }
        bytes.fill(0); bytes[..text.len()].copy_from_slice(text.as_bytes()); true
    }
}

/// WINDOW action for a read-only system metric query (index in value 6).
pub const SYSTEM_METRICS: u32 = 10;

/// WINDOW action for a read-only default system color (signed index in value 6).
pub const SYSTEM_COLOR: u32 = 11;

pub const ACTIVATE_WINDOW: u32 = 12;
pub const FOREGROUND_WINDOW: u32 = 13;
pub const IS_ICONIC: u32 = 14;
pub const FOCUS: u32 = 15;
pub const ENABLE_WINDOW: u32 = 16;
pub const GET_WINDOW_PLACEMENT: u32 = 17;
pub const SET_WINDOW_PLACEMENT: u32 = 18;
pub const CURSOR_POSITION: u32 = 19;
pub const SCREEN_TO_CLIENT: u32 = 20;
pub const CHILD_WINDOW_FROM_POINT: u32 = 21;
pub const SET_SYSTEM_CLOSE_ENABLED: u32 = 22;
pub const OPEN_CLIPBOARD: u32 = 23;
pub const CLOSE_CLIPBOARD: u32 = 24;
pub const CLIPBOARD_FORMAT_AVAILABLE: u32 = 25;
pub const WINDOW_PLACEMENT_FIELDS: core::ops::Range<usize> = 20..30;
pub const EDIT_CONTROL: u32 = 7;
pub const STATUS_CONTROL: u32 = 8;
pub const FOLDER_TREE_CONTROL: u32 = 9;
pub const FOLDER_PICKER_WINDOW: u32 = 2;
pub const FOLDER_PICKER_OK: u32 = 1;
pub const FOLDER_PICKER_CANCEL: u32 = 2;
pub const FOLDER_PICKER_PATH_LABEL: u32 = 102;
pub const FOLDER_TREE_SELECTED: u32 = 0x8001;
pub const FOLDER_TREE_EXPANDED: u32 = 0x8002;
pub const FOLDER_TREE_ACTIVATED: u32 = 0x8003;
pub const FOLDER_TREE_SET_EXPANDABLE: u32 = 0x8004;
pub const FOLDER_PATH_CHANGED: u32 = 0x8005;
pub const FOLDER_ICON: u64 = 0;
pub const DESKTOP_ICON: u64 = 1;
pub const COMPUTER_ICON: u64 = 2;
pub const DRIVE_ICON: u64 = 3;
pub const PROFILE_ICON: u64 = 4;
pub const LIBRARIES_ICON: u64 = 5;
