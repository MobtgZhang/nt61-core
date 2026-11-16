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
