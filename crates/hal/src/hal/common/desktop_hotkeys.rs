//! Out-of-band desktop shortcuts. Never mix privileged shortcuts with text bytes.
use core::sync::atomic::{AtomicU32, Ordering};
static PENDING: AtomicU32 = AtomicU32::new(0);
/// HID usage and USB modifier bitmap; shared by PS/2, USB and virtio decoders.
pub fn intercept(usage: u8, modifiers: u8) -> bool {
    let ctrl = modifiers & 0x11 != 0;
    let alt = modifiers & 0x44 != 0;
    let shift = modifiers & 0x22 != 0;
    let gui = modifiers & 0x88 != 0;
    let event = match usage {
        0x4c if ctrl && alt => nt61_abi::session_bridge::SECURITY,
        0x3d if alt => nt61_abi::session_bridge::CLOSE,
        0x29 if ctrl && shift => nt61_abi::session_bridge::TASK_MANAGER,
        0x0f if gui => nt61_abi::session_bridge::LOCK,
        _ => return false,
    };
    PENDING.fetch_or(event, Ordering::Release);
    true
}
pub fn take() -> u32 { PENDING.swap(0, Ordering::AcqRel) }
