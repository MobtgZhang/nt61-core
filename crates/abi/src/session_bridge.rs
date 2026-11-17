//! Private desktop/session protocol. These are not NT system call ordinals.
pub const SESSION: u32 = 64;
pub const HOTKEYS: u32 = 65;
pub const MONITOR: u32 = 66;
pub const TASK_CONTROL: u32 = 67;
pub const SERVICES: u32 = 68;
pub const SERVICE_CONTROL: u32 = 69;
pub const DISPLAY_MODES: u32 = 70;
pub const DISPLAY_SET: u32 = 71;
pub const SECURITY: u32 = 1;
pub const CLOSE: u32 = 2;
pub const TASK_MANAGER: u32 = 4;
pub const LOCK: u32 = 8;
/// Fixed monitor header, little endian u64 values: uptime ms, physical total,
/// physical available, thread count, pool bytes, CPU total ticks, CPU idle ticks.
pub const MONITOR_BYTES: usize = 56;
/// Bounded local SAM account enumeration and authorized account management.
pub const ACCOUNTS: u32 = 72;
pub const ACCOUNT_CONTROL: u32 = 73;

/// Play an installed default-scheme sound through the real audio device.
pub const SOUND: u32 = 74;
/// NET1 header + bounded physical-adapter records (50 bytes each).
/// Each record: index, flags, MTU, IPv4, mask, gateway, DNS (u32 LE),
/// MAC (6 bytes), kernel interface name (16 bytes, zero padded).
pub const NETWORK: u32 = 75;
