//! Private bring-up services shared by the desktop and kernel on all four ISAs.
//! These are project interfaces, not Windows system call numbers or WDDM APIs.

pub const SYS_DESKTOP_SERVICE: u32 = 0x030A;
pub const VERSION: u32 = 2;
pub const LIST: u32 = 1;
pub const READ: u32 = 2;
pub const WRITE: u32 = 3;
pub const MKDIR: u32 = 4;
pub const REMOVE: u32 = 5;
pub const COMMAND: u32 = 6;
pub const LOGON: u32 = 7;
/// Input: offset u64 and requested count u32 (LE). Output: up to count bytes.
pub const READ_AT: u32 = 8;
pub const RENAME: u32 = 9;
/// Create new, rejecting an existing name under the volume mutation lock.
pub const CREATE: u32 = 10;
/// Read-only process snapshot. Header: CPU count u32, process count u32.
/// Each entry: PID u64, working-set bytes u64, CPU ticks u64, session u32,
/// name length u32, user length u32, then UTF-8 name/user. All integers LE.
/// These are observed kernel counters; no CPU utilization percentage is synthesized.
pub const PROCESSES: u32 = 11;
/// Caller-owned DiskPart focus: disk/partition/volume u32, u32::MAX means none.
/// Reply: the same 12 bytes, Win32 exit code u32, then UTF-8 output.
/// No global interactive state; simultaneous consoles cannot steal one another's focus.
pub const DISKPART:u32=12;
pub const DISKPART_HEADER:usize=16;
pub const PROCESS_HEADER: usize = 36;
pub const MAX_PROCESSES: usize = 4096;
pub const MAX_READ_CHUNK: usize = 65536;
pub const MAX_TRANSFER: usize = 4 * 1024 * 1024;
pub const MAX_PATH: usize = 1024;

/// All pointers are user virtual addresses. Output is copied only after the
/// operation completes. `length` receives the required output size on overflow.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Request {
    pub version: u32,
    pub operation: u32,
    pub path: u64,
    pub path_length: u64,
    pub data: u64,
    pub capacity: u64,
    pub length: u64,
    pub status: u32,
    pub flags: u32,
}

// Every supported ISA uses this exact wire layout, including host tools.
const _: () = assert!(core::mem::size_of::<Request>() == 56);
const _: () = assert!(core::mem::align_of::<Request>() == 8);

/// LIST returns a sequence of these headers followed by `name_length` UTF-8
/// bytes. Headers are explicitly encoded as little endian, without padding.
pub const ENTRY_HEADER: usize = 16;
pub const DIRECTORY: u32 = 1;
pub const STATUS_SUCCESS: u32 = 0;
pub const STATUS_INVALID_PARAMETER: u32 = 0xC000_000D;
pub const STATUS_ACCESS_DENIED: u32 = 0xC000_0022;
pub const STATUS_NOT_SUPPORTED: u32 = 0xC000_00BB;
pub const STATUS_OBJECT_NAME_COLLISION: u32 = 0xC000_0035;
pub const STATUS_NOT_FOUND: u32 = 0xC000_0034;
pub const STATUS_BUFFER_TOO_SMALL: u32 = 0xC000_0023;

/// COMMAND payload is 1024 bytes of NUL-terminated UTF-8 cwd, then console
/// output. The input path is the command line; input data starts with cwd.
pub const CWD_BYTES: usize = MAX_PATH;
/// LOGON path is the account name, input data the password. No credentials are
/// returned. Only the boot LogonUI process may call this operation.
pub const COMMAND_EXIT: u32 = 1;
