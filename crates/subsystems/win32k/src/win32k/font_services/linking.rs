//! Linking capability queries for the installed local font engine. The engine
//! substitutes whole faces and has no per-glyph linked-font chains.
use crate::arch::common::syscall_frame::SyscallFrame;

pub extern "C" fn nt_gdi_any_linked_fonts(_: &SyscallFrame) -> u64 {
    0
}
pub extern "C" fn nt_gdi_font_is_linked(frame: &SyscallFrame) -> u64 {
    let Some(_dc) = super::dc::get_dc(frame.args[0]) else {
        return 0;
    };
    0
}
pub extern "C" fn nt_gdi_query_font_assoc_info(frame: &SyscallFrame) -> u64 {
    // Neither DBCS ANSI/OEM association nor font-linking flags are enabled.
    // The argument is HDC; no output buffer is part of this Win7 service.
    let Some(_dc) = super::dc::get_dc(frame.args[0]) else {
        return 0;
    };
    0
}
