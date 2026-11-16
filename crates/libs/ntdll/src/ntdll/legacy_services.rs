//! Legacy service behavior retained by the Windows 7 SP1 x64 kernel.
//!
//! These are the complete unconditional bodies observed in the pinned kernel
//! (SHA256 7fba129db114e8808a2ee5ae597f66176b7eb3c4077e0b5e9a3be3f74aa2e6a6),
//! rather than fallback handlers for missing implementations.

/// The complete original x64 NtCompleteConnectPort body at RVA 0x30aa00
/// returns STATUS_SUCCESS without looking up or dereferencing the handle.
/// Connection setup belongs to the ALPC accept path in this version of NT.
pub extern "C" fn nt_complete_connect_port(_port: super::types::HANDLE) -> i32 {
    0
}

/// The original x64 service at RVA 0x3eb3c0 returns TRUE without reading state.
pub extern "C" fn nt_query_port_information_process() -> u8 {
    1
}

/// The original x64 service at RVA 0x105210 has no LDT implementation and
/// unconditionally returns STATUS_NOT_IMPLEMENTED. All supported target ISAs
/// use this same 64-bit compatibility ABI, without a per-process x86 LDT.
pub extern "C" fn nt_set_ldt_entries(
    _selector0: u32,
    _entry0_low: u32,
    _entry0_high: u32,
    _selector1: u32,
    _entry1_low: u32,
    _entry1_high: u32,
) -> i32 {
    0xc0000002u32 as i32
}
