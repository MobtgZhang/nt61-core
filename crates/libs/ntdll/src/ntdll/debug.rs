//! ntdll - kernel debugger support (dbg_print)
//
//! `dbg_print` / `dbg_print_ex` write a formatted message to the
//! kernel debugger (kd). They route to the kernel logging system
//! which dispatches to serial and optionally to KdCom.
//
//! References: MSDN Library "Windows 7" - dbg_print / dbg_print_ex
//! in `ntddk.h` / `wdbgexts.h`. The DPFLTR_* component IDs are
//! from `wdm.h`.

use super::types::PVOID;

/// Component identifiers from the Windows 7 WDK dpfilter.h.
pub const DPFLTR_SYSTEM: u32 = 0;
pub const DPFLTR_SMSS: u32 = 1;
pub const DPFLTR_SETUP: u32 = 2;
pub const DPFLTR_NTFS: u32 = 3;
pub const DPFLTR_FSTUB: u32 = 4;
pub const DPFLTR_CRASHDUMP: u32 = 5;
pub const DPFLTR_CDAUDIO: u32 = 6;
pub const DPFLTR_CDROM: u32 = 7;
pub const DPFLTR_CLASSPNP: u32 = 8;
pub const DPFLTR_DISK: u32 = 9;
pub const DPFLTR_REDBOOK: u32 = 10;
pub const DPFLTR_STORPROP: u32 = 11;
pub const DPFLTR_SCSIPORT: u32 = 12;
pub const DPFLTR_SCSIMINIPORT: u32 = 13;
pub const DPFLTR_CONFIG: u32 = 14;
pub const DPFLTR_I8042PRT: u32 = 15;
pub const DPFLTR_SERMOUSE: u32 = 16;
pub const DPFLTR_LSERMOUS: u32 = 17;
pub const DPFLTR_KBDHID: u32 = 18;
pub const DPFLTR_MOUHID: u32 = 19;
pub const DPFLTR_KBDCLASS: u32 = 20;
pub const DPFLTR_MOUCLASS: u32 = 21;
pub const DPFLTR_TWOTRACK: u32 = 22;
pub const DPFLTR_WMILIB: u32 = 23;
pub const DPFLTR_ACPI: u32 = 24;
pub const DPFLTR_AMLI: u32 = 25;
pub const DPFLTR_HALIA64: u32 = 26;
pub const DPFLTR_VIDEO: u32 = 27;
pub const DPFLTR_SVCHOST: u32 = 28;
pub const DPFLTR_VIDEOPRT: u32 = 29;
pub const DPFLTR_TCPIP: u32 = 30;
pub const DPFLTR_DMSYNTH: u32 = 31;
pub const DPFLTR_NTOSPNP: u32 = 32;
pub const DPFLTR_FASTFAT: u32 = 33;
pub const DPFLTR_SAMSS: u32 = 34;
pub const DPFLTR_PNPMGR: u32 = 35;
pub const DPFLTR_NETAPI: u32 = 36;
pub const DPFLTR_SCSERVER: u32 = 37;
pub const DPFLTR_SCCLIENT: u32 = 38;
pub const DPFLTR_SERIAL: u32 = 39;
pub const DPFLTR_SERENUM: u32 = 40;
pub const DPFLTR_UHCD: u32 = 41;
pub const DPFLTR_RPCPROXY: u32 = 42;
pub const DPFLTR_AUTOCHK: u32 = 43;
pub const DPFLTR_DCOMSS: u32 = 44;
pub const DPFLTR_UNIMODEM: u32 = 45;
pub const DPFLTR_SIS: u32 = 46;
pub const DPFLTR_FLTMGR: u32 = 47;
pub const DPFLTR_WMICORE: u32 = 48;
pub const DPFLTR_BURNENG: u32 = 49;
pub const DPFLTR_IMAPI: u32 = 50;
pub const DPFLTR_SXS: u32 = 51;
pub const DPFLTR_FUSION: u32 = 52;
pub const DPFLTR_IDLETASK: u32 = 53;
pub const DPFLTR_SOFTPCI: u32 = 54;
pub const DPFLTR_TAPE: u32 = 55;
pub const DPFLTR_MCHGR: u32 = 56;
pub const DPFLTR_IDEP: u32 = 57;
pub const DPFLTR_PCIIDE: u32 = 58;
pub const DPFLTR_FLOPPY: u32 = 59;
pub const DPFLTR_FDC: u32 = 60;
pub const DPFLTR_TERMSRV: u32 = 61;
pub const DPFLTR_W32TIME: u32 = 62;
pub const DPFLTR_PREFETCHER: u32 = 63;
pub const DPFLTR_RSFILTER: u32 = 64;
pub const DPFLTR_FCPORT: u32 = 65;
pub const DPFLTR_PCI: u32 = 66;
pub const DPFLTR_DMIO: u32 = 67;
pub const DPFLTR_DMCONFIG: u32 = 68;
pub const DPFLTR_DMADMIN: u32 = 69;
pub const DPFLTR_WSOCKTRANSPORT: u32 = 70;
pub const DPFLTR_VSS: u32 = 71;
pub const DPFLTR_PNPMEM: u32 = 72;
pub const DPFLTR_PROCESSOR: u32 = 73;
pub const DPFLTR_DMSERVER: u32 = 74;
pub const DPFLTR_SR: u32 = 75;
pub const DPFLTR_INFINIBAND: u32 = 76;
pub const DPFLTR_IHVDRIVER: u32 = 77;
pub const DPFLTR_IHVVIDEO: u32 = 78;
pub const DPFLTR_IHVAUDIO: u32 = 79;
pub const DPFLTR_IHVNETWORK: u32 = 80;
pub const DPFLTR_IHVSTREAMING: u32 = 81;
pub const DPFLTR_IHVBUS: u32 = 82;
pub const DPFLTR_HPS: u32 = 83;
pub const DPFLTR_RTLTHREADPOOL: u32 = 84;
pub const DPFLTR_LDR: u32 = 85;
pub const DPFLTR_TCPIP6: u32 = 86;
pub const DPFLTR_ISAPNP: u32 = 87;
pub const DPFLTR_SHPC: u32 = 88;
pub const DPFLTR_STORPORT: u32 = 89;
pub const DPFLTR_STORMINIPORT: u32 = 90;
pub const DPFLTR_PRINTSPOOLER: u32 = 91;
pub const DPFLTR_VSSDYNDISK: u32 = 92;
pub const DPFLTR_VERIFIER: u32 = 93;
pub const DPFLTR_VDS: u32 = 94;
pub const DPFLTR_VDSBAS: u32 = 95;
pub const DPFLTR_VDSDYN: u32 = 96;
pub const DPFLTR_VDSDYNDR: u32 = 97;
pub const DPFLTR_VDSLDR: u32 = 98;
pub const DPFLTR_VDSUTIL: u32 = 99;
pub const DPFLTR_DFRGIFC: u32 = 100;
pub const DPFLTR_DEFAULT: u32 = 101;
pub const DPFLTR_MM: u32 = 102;
pub const DPFLTR_DFSC: u32 = 103;
pub const DPFLTR_WOW64: u32 = 104;
pub const DPFLTR_ALPC: u32 = 105;
pub const DPFLTR_WDI: u32 = 106;
pub const DPFLTR_PERFLIB: u32 = 107;
pub const DPFLTR_KTM: u32 = 108;
pub const DPFLTR_IOSTRESS: u32 = 109;
pub const DPFLTR_HEAP: u32 = 110;
pub const DPFLTR_WHEA: u32 = 111;
pub const DPFLTR_USERGDI: u32 = 112;
pub const DPFLTR_MMCSS: u32 = 113;
pub const DPFLTR_TPM: u32 = 114;
pub const DPFLTR_THREADORDER: u32 = 115;
pub const DPFLTR_ENVIRON: u32 = 116;
pub const DPFLTR_EMS: u32 = 117;
pub const DPFLTR_WDT: u32 = 118;
pub const DPFLTR_FVEVOL: u32 = 119;
pub const DPFLTR_NDIS: u32 = 120;
pub const DPFLTR_NVCTRACE: u32 = 121;
pub const DPFLTR_LUAFV: u32 = 122;
pub const DPFLTR_APPCOMPAT: u32 = 123;
pub const DPFLTR_USBSTOR: u32 = 124;
pub const DPFLTR_SBP2PORT: u32 = 125;
pub const DPFLTR_COVERAGE: u32 = 126;
pub const DPFLTR_CACHEMGR: u32 = 127;
pub const DPFLTR_MOUNTMGR: u32 = 128;
pub const DPFLTR_CFR: u32 = 129;
pub const DPFLTR_TXF: u32 = 130;
pub const DPFLTR_KSECDD: u32 = 131;
pub const DPFLTR_FLTREGRESS: u32 = 132;
pub const DPFLTR_MPIO: u32 = 133;
pub const DPFLTR_MSDSM: u32 = 134;
pub const DPFLTR_UDFS: u32 = 135;
pub const DPFLTR_PSHED: u32 = 136;
pub const DPFLTR_STORVSP: u32 = 137;
pub const DPFLTR_LSASS: u32 = 138;
pub const DPFLTR_SSPICLI: u32 = 139;
pub const DPFLTR_CNG: u32 = 140;
pub const DPFLTR_EXFAT: u32 = 141;
pub const DPFLTR_FILETRACE: u32 = 142;
pub const DPFLTR_XSAVE: u32 = 143;
pub const DPFLTR_SE: u32 = 144;
pub const DPFLTR_DRIVEEXTENDER: u32 = 145;
pub const DPFLTR_POWER: u32 = 146;

pub const KDPFLTR_MASK: u32 = 0x8000_0000;

fn component_to_subsystem_bits(component: u32) -> u32 {
    match component {
        DPFLTR_SYSTEM | DPFLTR_SMSS | DPFLTR_NTOSPNP => crate::rtl::logging::subsystem::KERNEL,
        DPFLTR_NTFS => crate::rtl::logging::subsystem::NTFS,
        DPFLTR_NDIS | DPFLTR_TCPIP | DPFLTR_TCPIP6 => crate::rtl::logging::subsystem::NET,
        DPFLTR_STORPORT | DPFLTR_DISK | DPFLTR_CLASSPNP => crate::rtl::logging::subsystem::STORAGE,
        DPFLTR_ALPC => crate::rtl::logging::subsystem::IO,
        _ => crate::rtl::logging::subsystem::DBG,
    }
}

fn component_to_subsystem_name(component: u32) -> &'static str {
    match component {
        DPFLTR_SYSTEM | DPFLTR_SMSS | DPFLTR_NTOSPNP => "KERNEL",
        DPFLTR_NTFS => "NTFS",
        DPFLTR_NDIS | DPFLTR_TCPIP | DPFLTR_TCPIP6 => "NET",
        DPFLTR_STORPORT | DPFLTR_DISK | DPFLTR_CLASSPNP => "STORAGE",
        DPFLTR_ALPC => "IO",
        _ => "DBG",
    }
}

fn read_c_string(ptr: *const u8) -> Option<&'static str> {
    if ptr.is_null() {
        return None;
    }
    let mut len = 0;
    // The string comes from user mode and has no known length, so walk
    // it in probed windows: touching a byte before validating the range
    // would turn a hostile pointer into a kernel page fault.
    const WINDOW: usize = 64;
    while len < 4096 {
        let window = WINDOW.min(4096 - len);
        if crate::mm::user_copy::probe_user_read(ptr as u64 + len as u64, window).is_err() {
            return None;
        }
        for i in 0..window {
            if unsafe { *ptr.add(len + i) } == 0 {
                let slice = unsafe { core::slice::from_raw_parts(ptr, len + i) };
                return core::str::from_utf8(slice).ok();
            }
        }
        len += window;
    }
    None
}

/// Format a debug message into a buffer.
fn format_message(msg: &str) -> [u8; 512] {
    let mut buf = [0u8; 512];
    for (i, &b) in msg.as_bytes().iter().enumerate().take(511) {
        buf[i] = b;
    }
    buf
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `dbg_print` - send a formatted message to the kernel debugger.
/// In the Windows API this is variadic; we accept a NUL-terminated
/// format string pointer. Since we can't parse C format strings,
/// we emit the raw string as-is.
/// Returns 1 on success, 0 on failure.
pub unsafe extern "C" fn dbg_print(format: PVOID) -> u32 {
    if super::debug_filter::nt_query_debug_filter_state(u32::MAX, 0) == 0 {
        return 1;
    }
    let msg = match read_c_string(format as *const u8) {
        Some(s) => s,
        None => {
            crate::kprintln_info!("DBG", "DbgPrint: null format");
            return 0;
        }
    };

    // Route through the kernel logging system
    // dbg_print uses the DEFAULT component
    if crate::rtl::logging::should_log(
        crate::rtl::logging::LogLevel::Info,
        crate::rtl::logging::subsystem::DBG,
    ) {
        let buf = format_message(msg);
        let _ = &buf;
        let cpu = crate::rtl::logging::current_cpu();
        crate::rtl::logging::log_write_impl(crate::rtl::logging::LogLevel::Info, "DBG", cpu, msg);
    }
    1
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `dbg_print_ex` - same as dbg_print but with component/level filtering.
/// Messages are emitted only when the component mask includes the level bits.
/// Returns 1 on success, 0 on failure.
pub unsafe extern "C" fn dbg_print_ex(component: u32, level: u32, format: PVOID) -> u32 {
    if super::debug_filter::nt_query_debug_filter_state(component, level) == 0 {
        return 1;
    }

    let msg = match read_c_string(format as *const u8) {
        Some(s) => s,
        None => {
            crate::kprintln_info!("DBG", "DbgPrintEx: null format");
            return 0;
        }
    };

    let subsys_bits = component_to_subsystem_bits(component);
    let subsys_name = component_to_subsystem_name(component);

    if crate::rtl::logging::should_log(crate::rtl::logging::LogLevel::Info, subsys_bits) {
        let cpu = crate::rtl::logging::current_cpu();
        crate::rtl::logging::log_write_impl(
            crate::rtl::logging::LogLevel::Info,
            subsys_name,
            cpu,
            msg,
        );
    }
    1
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `v_dbg_print_ex` - takes a va_list for format arguments.
/// Since we cannot parse C format strings, we emit the raw format
/// string as a placeholder. Returns 0.
pub unsafe extern "C" fn v_dbg_print_ex(
    component: u32,
    level: u32,
    format: PVOID,
    args: PVOID,
) -> u32 {
    // Suppress the warning about unused args
    let _ = args;
    if super::debug_filter::nt_query_debug_filter_state(component, level) == 0 {
        return 1;
    }

    let msg = match read_c_string(format as *const u8) {
        Some(s) => s,
        None => return 0,
    };

    if crate::rtl::logging::should_log(
        crate::rtl::logging::LogLevel::Info,
        crate::rtl::logging::subsystem::DBG,
    ) {
        crate::kprintln_info!("DBG", "[vDbgPrintEx] {}", msg);
    }
    1
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `v_dbg_print_ex_with_prefix` - same as v_dbg_print_ex but with a prefix string.
pub unsafe extern "C" fn v_dbg_print_ex_with_prefix(
    _prefix: PVOID,
    _component: u32,
    _level: u32,
    _format: PVOID,
    _args: PVOID,
) -> u32 {
    0
}

/// `dbg_break_point` - trigger a breakpoint exception.
/// In kernel mode, this causes a INT3 which would normally be caught
/// by a kernel debugger. In our environment, we just log and continue.
pub fn dbg_break_point() {
    crate::kprintln_info!("DBG", "[DBG] BreakPoint triggered");
}

/// `dbg_print_hex` - helper to print a hex value. Used during development.
pub fn dbg_print_hex(label: &str, value: u64) {
    crate::kprintln_info!("DBG", "[DBG] {} = 0x{:016x}", label, value);
}

/// # Safety
/// The UNICODE_STRING and its contents designate readable caller memory.
pub unsafe extern "C" fn nt_display_string(string: *const super::types::UnicodeString) -> super::types::NTSTATUS {
    use super::status::*;
    if !super::privilege::enabled(7) { return STATUS_PRIVILEGE_NOT_HELD; }
    let units = match crate::mm::user_copy::read_user_unicode_string(string as u64, 65534) {
        Ok(units) => units,
        Err(status) => return status,
    };
    for character in core::char::decode_utf16(units) {
        let character = character.unwrap_or(core::char::REPLACEMENT_CHARACTER);
        let mut bytes = [0; 4];
        let encoded = character.encode_utf8(&mut bytes);
        crate::hal::serial::write_string(encoded);
        // Boot video has an eight-bit font, so unrepresentable glyphs use '?'.
        crate::drivers::bootvid::inbv_display_string(&[if character as u32 <= 255 { character as u8 } else { b'?' }]);
    }
    STATUS_SUCCESS
}
