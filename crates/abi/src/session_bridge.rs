//! Private desktop/session protocol. These are not NT system call ordinals.
pub const SESSION: u32 = 64;
pub const HOTKEYS: u32 = 65;
pub const MONITOR: u32 = 66;
pub const TASK_CONTROL: u32 = 67;
pub const SERVICES: u32 = 68;
pub const SERVICE_CONTROL: u32 = 69;
pub const SECURITY: u32 = 1;
pub const CLOSE: u32 = 2;
pub const TASK_MANAGER: u32 = 4;
pub const LOCK: u32 = 8;
/// Fixed monitor header, little endian u64 values: uptime ms, physical total,
/// physical available, thread count, pool bytes, CPU total ticks, CPU idle ticks.
pub const MONITOR_BYTES: usize = 56;
