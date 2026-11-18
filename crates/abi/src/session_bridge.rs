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
pub const WINDOW_LEFT: u32 = 16;
pub const WINDOW_RIGHT: u32 = 32;
pub const WINDOW_UP: u32 = 64;
pub const WINDOW_DOWN: u32 = 128;
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

/// Independent desktop media stream: stereo PCM16 at 48 kHz. Status is four
/// LE u32 values: queued samples, available, paused, media volume (0..100).
pub const MEDIA: u32 = 76;
pub const MEDIA_STATUS_BYTES: usize = 16;
pub const MEDIA_MAX_SAMPLES: usize = 9600;

/// Read-only PnP snapshot: DEV1, count (u32 LE), at most 64 records of 88 bytes.
/// Record: bus, address, function, state, reserved, driver-bound (u32 LE each),
/// hardware ID and compatible ID (32 ASCII bytes, NUL padded each). No pointers.
pub const DEVICES: u32 = 77;
