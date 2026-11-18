//! Shadow SSDT Syscall Handlers
//
//! Implements syscall handlers for win32k.sys GDI and USER functions.
//! These handlers are registered with the Shadow SSDT and called via syscall.
//
//! Handlers receive `SyscallFrame`, an architecture-neutral argument frame.
//! `arch/*/` is responsible for filling register and stack arguments from the
//! native trap frame; this module contains no per-ISA conditions.
//
//! ## Syscall Numbers
//
//! Syscall numbers are defined in `src/ke/shadow_ssdt.rs` based on
//! j00ru/windows-syscalls for Windows 7 SP1 x64.

extern crate alloc;

pub use super::bitmap::*;
pub use super::clipboard::services::*;
pub use super::font_services::{
    nt_gdi_add_font_mem_resource_ex, nt_gdi_remove_font_mem_resource_ex,
    nt_gdi_hfont_create,
    nt_gdi_get_char_abc_widths_w, nt_gdi_get_char_width_info, nt_gdi_get_char_width_w,
    nt_gdi_get_font_data, nt_gdi_get_font_file_data, nt_gdi_get_font_file_info,
    nt_gdi_get_font_unicode_ranges, nt_gdi_get_glyph_indices_w,
    nt_gdi_get_glyph_indices_w_internal, nt_gdi_get_glyph_outline, nt_gdi_get_kerning_pairs,
    nt_gdi_get_rasterizer_caps, nt_gdi_get_realization_info, nt_gdi_get_text_extent_ex_w,
};
pub use super::user_state::menu_services::*;
pub use super::window::attributes::nt_user_window_from_physical_point as nt_user_window_from_point;
pub use super::window::attributes::*;
pub use super::window::class_services::*;
pub use super::window::coordinates::*;
pub use super::window::input_context::*;
pub use super::window::native_state::*;
pub use super::window::placement::*;

pub use super::clipping::*;
pub use super::dc::bounds::*;
pub use super::dc::caps::*;
pub use super::dc::services::*;
pub use super::object_enumeration::nt_gdi_enum_objects;
pub use super::objects::nt_gdi_create_hatch_brush_internal;
pub use super::objects::nt_gdi_get_stock_object;
pub use super::palette::*;
pub use super::palette::halftone::nt_gdi_ht_get_8bpp_format_palette;
pub use super::palette::halftone::nt_gdi_ht_get_8bpp_mask_palette;
pub use super::path::services::*;
pub use super::path::shapes::*;
pub use super::polygon::services::*;
pub use super::region::services::*;
pub use super::retired_services::*;
pub use super::server_metafile::*;
pub use super::selection::*;
pub use super::transforms::*;
pub use super::transforms::metafile::nt_gdi_convert_metafile_rect;
use crate::arch::common::syscall_frame::SyscallFrame;

// Helper functions to call win32k::mod functions (since mod is a keyword)
mod win32k_helpers {
    use crate::libs::win32k;

    pub fn create_timer(hwnd: u64, n_id_event: u32, u_elapse: u32, timer_proc: u64) -> Option<u32> {
        win32k::create_timer(hwnd, n_id_event, u_elapse, timer_proc)
    }

    pub fn kill_timer(hwnd: u64, n_id_event: u32) -> bool {
        win32k::kill_timer(hwnd, n_id_event)
    }

    pub fn install_hook(hook_type: i32, hook_proc: u64, thread_id: u32) -> Option<u64> {
        win32k::install_hook(hook_type, hook_proc, thread_id)
    }

    pub fn remove_hook(handle: u64) -> bool {
        win32k::remove_hook(handle)
    }

    pub fn allocate_menu_handle() -> u64 {
        win32k::allocate_menu_handle()
    }

    pub const fn get_mb_ok() -> u32 {
        win32k::MB_OK
    }
    pub const fn get_idok() -> u32 {
        win32k::IDOK
    }
}

// =============================================================================
// Argument Extraction Helpers
// =============================================================================

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// Read a validated argument slot from the architecture-neutral frame.
#[inline(always)]
pub unsafe fn get_arg0(frame: &SyscallFrame) -> u64 {
    frame.args[0]
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
#[inline(always)]
pub unsafe fn get_arg1(frame: &SyscallFrame) -> u64 {
    frame.args[1]
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
#[inline(always)]
pub unsafe fn get_arg2(frame: &SyscallFrame) -> u64 {
    frame.args[2]
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
#[inline(always)]
pub unsafe fn get_arg3(frame: &SyscallFrame) -> u64 {
    frame.args[3]
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
#[inline(always)]
pub unsafe fn get_arg4(frame: &SyscallFrame) -> u64 {
    frame.args[4]
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
#[inline(always)]
pub unsafe fn get_arg5(frame: &SyscallFrame) -> u64 {
    frame.args[5]
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
#[inline(always)]
pub unsafe fn get_arg6(frame: &SyscallFrame) -> u64 {
    frame.args[6]
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
#[inline(always)]
pub unsafe fn get_arg7(frame: &SyscallFrame) -> u64 {
    frame.args[7]
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// Read a stack argument by its Windows syscall index (arg4 is index 0).
#[inline(always)]
pub unsafe fn get_stack_arg(frame: &SyscallFrame, index: usize) -> u64 {
    if 4 + index < crate::arch::common::syscall_frame::SYSCALL_ARGUMENT_COUNT {
        frame.args[4 + index]
    } else {
        0
    }
}

/// Copy a NUL-terminated UTF-16 string out of user memory.
//
/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// The scan is bounded by `max_chars` and every character is read from a
/// probed range, so an unterminated user string can no longer walk into
/// kernel memory. Returns the number of characters copied.
#[inline]
pub unsafe fn capture_user_unicode_string(
    address: u64,
    max_chars: usize,
    dst: &mut [u16],
) -> Result<usize, i32> {
    let len = crate::arch::common::user_memory::probe_for_read_unicode_string(address, max_chars)?;
    if len > dst.len() {
        return Err(crate::libs::ntdll::status::STATUS_INVALID_PARAMETER);
    }
    for i in 0..len {
        crate::mm::user_copy::read_user(address + i as u64 * 2, &mut dst[i])?;
    }
    Ok(len)
}

// =============================================================================
// GDI Syscall Handlers - Graphics Device Interface
// =============================================================================

/// NT_GDI_BIT_BLT - Bit block transfer
pub extern "C" fn nt_gdi_bit_blt(frame: &SyscallFrame) -> u64 {
    let hdc_dest = unsafe { get_arg0(frame) };
    let _ = &hdc_dest;
    let x = unsafe { get_arg1(frame) } as i32;
    let _ = &x;
    let y = unsafe { get_arg2(frame) } as i32;
    let _ = &y;
    let width = unsafe { get_arg3(frame) } as i32;
    let _ = &width;
    let height = unsafe { get_arg4(frame) } as i32;
    let _ = &height;
    let hdc_src = unsafe { get_arg5(frame) };
    let _ = &hdc_src;
    let src_x = unsafe { get_arg6(frame) } as i32;
    let _ = &src_x;
    let src_y = unsafe { get_arg7(frame) } as i32;
    let _ = &src_y;
    // Native NtGdiBitBlt has eleven parameters. ROP remains slot eight;
    // the final two are the background color and native operation flags.
    let rop = frame.args[8] as u32;
    let _background_color = frame.args[9] as u32;
    if frame.args[10] as u32 != 0 {
        // CAPTUREBLT/device synchronization flags are not implemented.
        return 0;
    }

    // kprintln!("[win32k] NT_GDI_BIT_BLT: dest=0x{:x} ({}x{}) src=0x{:x} ({}x{}) rop=0x{:08x}",
    //         hdc_dest, x, y, hdc_src, src_x, src_y, rop);

    let mut table = super::dc::get_dc_table();
    let source = if hdc_src != 0 {
        let Some(source) = table.get(hdc_src) else {
            return 0;
        };
        let binding = source.drawing_surface();
        if binding.is_null() {
            // A detached DC has no readable pixels. Passing no source lets
            // raster::bit_blt preserve its zero-size and source-free ROP
            // semantics without turning surface == 0 into the framebuffer.
            None
        } else {
            let mut snapshot = super::dc::DcObject::new();
            snapshot.surface = binding as u64;
            snapshot.world_transform = source.world_transform;
            snapshot.viewport_ext = source.viewport_ext;
            snapshot.window_ext = source.window_ext;
            snapshot.viewport_org = source.viewport_org;
            snapshot.window_org = source.window_org;
            snapshot.attributes = source.attributes;
            Some(snapshot)
        }
    } else {
        None
    };
    if let Some(dst_dc) = table.get(hdc_dest) {
        // Call the actual GDI bit_blt function
        let success = crate::libs::win32k::gdi_ops::gre_bit_blt(
            dst_dc,
            x,
            y,
            width,
            height,
            source.as_ref(),
            src_x,
            src_y,
            rop,
        );

        if success {
            1
        } else {
            0
        }
    } else {
        // kprintln!("[win32k] NT_GDI_BIT_BLT: invalid destination DC");
        0
    }
}

/// NT_GDI_GET_CHAR_SET - Get character set
pub use super::font_services::nt_gdi_get_char_set;

pub use super::dc::info::nt_gdi_make_info_dc;

/// NT_GDI_SELECT_BITMAP - Select bitmap into DC
pub extern "C" fn nt_gdi_select_object(frame: &SyscallFrame) -> u64 {
    let hdc = unsafe { get_arg0(frame) };
    let _ = &hdc;
    let hgdiobj = unsafe { get_arg1(frame) };
    let _ = &hgdiobj;

    // kprintln!("[win32k] NT_GDI_SELECT_BITMAP: hdc=0x{:x}, hgdiobj=0x{:016x}", hdc, hgdiobj);

    // Call the DC select object function
    let result = crate::libs::win32k::dc::select_typed_object(
        hdc,
        hgdiobj,
        Some(super::objects::GdiObjectType::Bitmap),
    );
    let _ = &result;

    // kprintln!("[win32k] NT_GDI_SELECT_BITMAP: returning 0x{:016x}", result);
    result
}

/// NT_GDI_DELETE_OBJECT_APP - Delete GDI object
pub extern "C" fn nt_gdi_delete_object(frame: &SyscallFrame) -> u64 {
    let hgdiobj = unsafe { get_arg0(frame) };
    let _ = &hgdiobj;

    // kprintln!("[win32k] NT_GDI_DELETE_OBJECT_APP: hgdiobj=0x{:016x}", hgdiobj);

    if hgdiobj == 0 {
        return 0;
    }

    let success = crate::libs::win32k::objects::gdi_delete_object(hgdiobj);
    let _ = &success;

    if success {
        1
    } else {
        0
    }
}

/// NtGdiStretchBlt has twelve parameters. The source DC is slot five,
/// source extents are slots eight/nine, ROP is slot ten, and background
/// color is slot eleven. A six-argument PatBlt interpretation is invalid.
pub extern "C" fn nt_gdi_stretch_blt(frame: &SyscallFrame) -> u64 {
    super::raster::stretch::nt_gdi_stretch_blt(frame)
}

pub use super::font_services::{
    nt_gdi_ext_text_out, nt_gdi_poly_text_out, nt_gdi_set_text_justification,
};

/// NT_GDI_RESTORE_DC - Restore device context
pub extern "C" fn nt_gdi_restore_dc(frame: &SyscallFrame) -> u64 {
    let hdc = unsafe { get_arg0(frame) };
    let _ = &hdc;
    let n_saved = unsafe { get_arg1(frame) } as i32;
    let _ = &n_saved;

    // kprintln!("[win32k] NT_GDI_RESTORE_DC: hdc=0x{:x}, n_saved={}", hdc, n_saved);

    let success = crate::libs::win32k::dc::gre_restore_dc(hdc, n_saved);
    let _ = &success;

    if success {
        1
    } else {
        0
    }
}

/// NT_GDI_SAVE_DC - Save device context
pub extern "C" fn nt_gdi_save_dc(frame: &SyscallFrame) -> u64 {
    let hdc = unsafe { get_arg0(frame) };
    let _ = &hdc;

    // kprintln!("[win32k] NT_GDI_SAVE_DC: hdc=0x{:x}", hdc);

    let result = crate::libs::win32k::dc::gre_save_dc(hdc);
    let _ = &result;

    // kprintln!("[win32k] NT_GDI_SAVE_DC: returning {}", result);
    result as u64
}

/// NT_GDI_LINE_TO - Draw line to position
pub extern "C" fn nt_gdi_line_to(frame: &SyscallFrame) -> u64 {
    let Some(mut dc) = super::dc::get_dc(frame.args[0]) else {
        return 0;
    };
    super::gdi_ops::gre_line_to(&mut dc, frame.args[1] as i32, frame.args[2] as i32) as u64
}

/// NT_GDI_CREATE_COMPATIBLE_BITMAP - create compatible bitmap
pub extern "C" fn nt_gdi_create_compatible_bitmap(frame: &SyscallFrame) -> u64 {
    let hdc = unsafe { get_arg0(frame) };
    let _ = &hdc;
    let width = unsafe { get_arg1(frame) } as i32;
    let _ = &width;
    let height = unsafe { get_arg2(frame) } as i32;
    let _ = &height;

    // kprintln!("[win32k] NT_GDI_CREATE_COMPATIBLE_BITMAP: hdc=0x{:x}, {}x{}", hdc, width, height);

    // create a 32-bit compatible bitmap
    let handle = crate::libs::win32k::objects::gdi_create_bitmap(width, height, 32);
    let _ = &handle;

    // kprintln!("[win32k] NT_GDI_CREATE_COMPATIBLE_BITMAP: returning 0x{:016x}", handle);
    handle
}

/// NT_GDI_GET_TEXT_CHARSET_INFO - Get text charset info
pub use super::font_services::nt_gdi_get_text_charset_info;

/// NT_GDI_GET_TEXT_METRICS_W - Get text metrics
pub use super::font_services::nt_gdi_get_text_metrics_w;

/// NT_GDI_GET_TEXT_EXTENT - Get text extent
pub use super::font_services::nt_gdi_get_text_extent;

/// NT_GDI_GET_TEXT_FACE_W - Get font face name
pub use super::font_services::nt_gdi_get_text_face_w;

/// NT_GDI_EXT_GET_OBJECT_W - Get object information
pub extern "C" fn nt_gdi_ext_get_object_w(frame: &SyscallFrame) -> u64 {
    super::object_queries::nt_gdi_ext_get_object_w(frame)
}

/// NT_GDI_CREATE_COMPATIBLE_DC - create memory DC
pub extern "C" fn nt_gdi_create_compatible_dc(frame: &SyscallFrame) -> u64 {
    let hdc_src = unsafe { get_arg0(frame) };
    let _ = &hdc_src;

    // kprintln!("[win32k] NT_GDI_CREATE_COMPATIBLE_DC: hdc_src=0x{:x}", hdc_src);

    let dc = crate::libs::win32k::dc::gre_create_compatible_dc(hdc_src);
    let _ = &dc;

    // kprintln!("[win32k] NT_GDI_CREATE_COMPATIBLE_DC: returning DC=0x{:08x}", dc);
    dc
}

/// NT_GDI_CREATE_PEN - create pen object
pub use super::pen_services::nt_gdi_create_pen;

/// NT_GDI_CREATE_BITMAP - create bitmap object
pub extern "C" fn nt_gdi_create_bitmap(frame: &SyscallFrame) -> u64 {
    super::bitmap::storage::nt_gdi_create_bitmap(frame)
}

/// NT_GDI_PAT_BLT - Pattern blt operation
pub extern "C" fn nt_gdi_pat_blt(frame: &SyscallFrame) -> u64 {
    let hdc = unsafe { get_arg0(frame) };
    let _ = &hdc;
    let x = unsafe { get_arg1(frame) } as i32;
    let _ = &x;
    let y = unsafe { get_arg2(frame) } as i32;
    let _ = &y;
    let width = unsafe { get_arg3(frame) } as i32;
    let _ = &width;
    let height = unsafe { get_arg4(frame) } as i32;
    let _ = &height;
    let rop = unsafe { get_arg5(frame) } as u32;
    let _ = &rop;

    // kprintln!("[win32k] NT_GDI_PAT_BLT: hdc=0x{:x}, ({}x{}) size={}x{}, rop=0x{:08x}",
    //         hdc, x, y, width, height, rop);

    if let Some(mut dc) = crate::libs::win32k::dc::get_dc(hdc) {
        let success = crate::libs::win32k::gdi_ops::gre_pat_blt(&mut dc, x, y, width, height, rop);
        if success {
            1
        } else {
            0
        }
    } else {
        0
    }
}

/// NT_GDI_DRAW_STREAM - Draw stream
pub extern "C" fn nt_gdi_draw_stream(frame: &SyscallFrame) -> u64 {
    let hdc = unsafe { get_arg0(frame) };
    let _ = &hdc;
    let pso = unsafe { get_arg1(frame) };
    let _ = &pso;
    let pdso = unsafe { get_arg2(frame) };
    let _ = &pdso;

    // kprintln!("[win32k] NT_GDI_DRAW_STREAM");

    // DrawStream: return 1 for success (stream drawing is a no-op for now).
    1
}

/// NT_GDI_MASK_BLT - Masked block transfer
pub extern "C" fn nt_gdi_mask_blt(frame: &SyscallFrame) -> u64 {
    super::raster::mask::nt_gdi_mask_blt(frame)
}

/// NT_GDI_GET_WIDTH_TABLE - Get width table
pub extern "C" fn nt_gdi_get_width_table(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NT_GDI_GET_WIDTH_TABLE");

    // GetWidthTable: return 8 (monospace default).
    8
}

/// NT_GDI_POLY_PAT_BLT - Poly pattern blt
pub extern "C" fn nt_gdi_poly_pat_blt(frame: &SyscallFrame) -> u64 {
    super::raster::pattern::nt_gdi_poly_pat_blt(frame)
}

/// NT_GDI_GET_NEAREST_COLOR - Get nearest color
pub extern "C" fn nt_gdi_get_nearest_color(frame: &SyscallFrame) -> u64 {
    let hdc = unsafe { get_arg0(frame) };
    let _ = &hdc;
    let cry = unsafe { get_arg1(frame) };
    let _ = &cry;
    let pcc = unsafe { get_arg2(frame) };
    let _ = &pcc;

    // kprintln!("[win32k] NT_GDI_GET_NEAREST_COLOR");

    // GetNearestColor: with a 32bpp framebuffer, all colors are exact.
    cry
}

/// NT_GDI_CREATE_DIBBRUSH - create DIB brush
pub extern "C" fn nt_gdi_create_dib_brush(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NT_GDI_CREATE_DIBBRUSH");

    // CreateDIBBrush: return a pseudo-handle for the DIB brush.
    0x0004_0001
}

/// NT_GDI_ALPHA_BLEND - Alpha blend
pub extern "C" fn nt_gdi_alpha_blend(frame: &SyscallFrame) -> u64 {
    super::raster::alpha::nt_gdi_alpha_blend(frame)
}

/// NT_GDI_DD_BLT - DirectDraw blt
pub extern "C" fn nt_gdi_dd_blt(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NT_GDI_DD_BLT");

    // DdBlt: return 1 (DDERR_OK equivalent).
    1
}

/// NT_GDI_OPEN_DCW - Open device context (wide)
pub extern "C" fn nt_gdi_open_dc_w(frame: &SyscallFrame) -> u64 {
    // Only a direct DC for the default display is supported. Device modes,
    // output addresses, information DCs, printer/spooler state and the
    // user-mode device output require implementations of their own.
    if frame.args[1] != 0
        || frame.args[2] != 0
        || frame.args[3] as u32 != 0
        || frame.args[4] as u32 == 0
        || frame.args[5] != 0
        || frame.args[6] != 0
        || frame.args[7] != 0
    {
        return 0;
    }

    if frame.args[0] != 0 {
        #[repr(C)]
        #[derive(Clone, Copy)]
        struct DeviceName {
            length: u16,
            maximum_length: u16,
            reserved: u32,
            buffer: u64,
        }
        // Native OpenDCW takes a UNICODE_STRING, not a NUL-terminated
        // pointer or USER's LARGE_STRING. Accept only the DISPLAY alias;
        // named adapters are not resolved by the current display manager.
        let name: DeviceName = match crate::mm::user_copy::read_user_object(frame.args[0]) {
            Ok(name) => name,
            Err(_) => return 0,
        };
        let display = [0x44u16, 0x49, 0x53, 0x50, 0x4c, 0x41, 0x59];
        if name.length != 14 || name.length > name.maximum_length || name.buffer == 0 {
            return 0;
        }
        for (index, expected) in display.iter().enumerate() {
            let source = match name.buffer.checked_add(index as u64 * 2) {
                Some(source) => source,
                None => return 0,
            };
            let mut unit = 0u16;
            if crate::mm::user_copy::read_user(source, &mut unit).is_err() {
                return 0;
            }
            if (0x61..=0x7a).contains(&unit) {
                unit -= 0x20;
            }
            if unit != *expected {
                return 0;
            }
        }
    }

    crate::libs::win32k::dc::gre_create_display_dc()
}

/// NT_GDI_GET_BITMAP_BITS - Get bitmap bits
pub extern "C" fn nt_gdi_get_bitmap_bits(frame: &SyscallFrame) -> u64 {
    super::bitmap::storage::nt_gdi_get_bitmap_bits(frame)
}

/// NT_GDI_STRETCH_DIBITS_INTERNAL - Stretch DIB bits (internal)
pub extern "C" fn nt_gdi_stretch_dibits_internal(frame: &SyscallFrame) -> u64 {
    super::raster::dib::nt_gdi_stretch_dibits_internal(frame)
}

pub extern "C" fn nt_gdi_set_dibits_to_device_internal(frame: &SyscallFrame) -> u64 {
    super::raster::dib::nt_gdi_set_dibits_to_device_internal(frame)
}

/// NT_GDI_GET_DIBITS_INTERNAL - Get DIB bits (internal)
pub extern "C" fn nt_gdi_get_dibits_internal(frame: &SyscallFrame) -> u64 {
    super::bitmap::dib::nt_gdi_get_dibits_internal(frame)
}

/// NT_GDI_DELETE_CLIENT_OBJ - Delete client object
pub extern "C" fn nt_gdi_delete_client_obj(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NT_GDI_DELETE_CLIENT_OBJ");

    0
}

/// NT_GDI_COMPUTE_XFORM_COEFFICIENTS - Compute transform coefficients
pub extern "C" fn nt_gdi_compute_xform_coefficients(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NT_GDI_COMPUTE_XFORM_COEFFICIENTS");

    // ComputeXformCoefficients: return 0 for identity transform.
    0
}

/// NT_GDI_UNREALIZE_OBJECT - Unrealize GDI object
pub extern "C" fn nt_gdi_unrealize_object(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NT_GDI_UNREALIZE_OBJECT");

    1
}

/// NT_GDI_RECTANGLE - Draw rectangle
pub extern "C" fn nt_gdi_rectangle(frame: &SyscallFrame) -> u64 {
    let hdc = unsafe { get_arg0(frame) };
    let _ = &hdc;
    let left = unsafe { get_arg1(frame) as i32 };
    let _ = &left;
    let top = unsafe { get_arg2(frame) as i32 };
    let _ = &top;
    let right = unsafe { get_arg3(frame) as i32 };
    let _ = &right;
    let bottom = unsafe { get_arg4(frame) as i32 };
    let _ = &bottom;

    // kprintln!("[win32k] NT_GDI_RECTANGLE: hdc=0x{:x} ({},{})-({},{})", hdc, left, top, right, bottom);

    // Get DC and draw rectangle
    if let Some(mut dc) = crate::libs::win32k::dc::get_dc(hdc) {
        let success =
            crate::libs::win32k::gdi_ops::gre_rectangle(&mut dc, left, top, right, bottom);
        let _ = &success;
        if success {
            1
        } else {
            0
        }
    } else {
        0
    }
}

/// NT_GDI_SET_LAYOUT - Set DC layout
pub extern "C" fn nt_gdi_set_layout(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NT_GDI_SET_LAYOUT");

    // SetLayout: return 0 (previous layout, LAYOUT_RTL = 1).
    0
}

/// NT_GDI_CREATE_DIBSECTION - create DIB section
pub extern "C" fn nt_gdi_create_dib_section(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NT_GDI_CREATE_DIBSECTION");

    // CreateDIBSection: return a pseudo-handle for the DIB section.
    0x0005_0001
}

/// NT_GDI_GET_DCFOR_BITMAP - Get DC for bitmap
pub extern "C" fn nt_gdi_get_dc_for_bitmap(frame: &SyscallFrame) -> u64 {
    let hbitmap = unsafe { get_arg0(frame) };
    let _ = &hbitmap;

    // kprintln!("[win32k] NT_GDI_GET_DCFOR_BITMAP: hbitmap=0x{:x}", hbitmap);

    // Get DC for bitmap - create a compatible DC with the bitmap selected
    crate::libs::win32k::dc::gre_get_dcfor_bitmap(hbitmap)
}

/// NT_GDI_GET_DC - Get device context
pub extern "C" fn nt_gdi_get_dc(frame: &SyscallFrame) -> u64 {
    let hwnd = unsafe { get_arg0(frame) };
    let _ = &hwnd;

    // kprintln!("[win32k] NT_GDI_GET_DC: hwnd=0x{:x}", hwnd);

    // Get DC for window - 0 means screen DC
    crate::libs::win32k::dc::gre_create_display_dc()
}

/// NT_GDI_CREATE_DIBITMAP_INTERNAL - create DIB bitmap (internal)
pub extern "C" fn nt_gdi_create_dibitmap_internal(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NT_GDI_CREATE_DIBITMAP_INTERNAL");

    // CreateDIBitmapInternal: return a pseudo-handle for the bitmap.
    0x0006_0001
}

/// NT_GDI_DD_DELETE_SURFACE_OBJECT - Delete DirectDraw surface object
pub extern "C" fn nt_gdi_dd_delete_surface_object(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NT_GDI_DD_DELETE_SURFACE_OBJECT");

    // DdDeleteSurfaceObject: return 1 (DDERR_OK).
    1
}

/// NT_GDI_DD_CAN_CREATE_SURFACE - Check if surface can be created
pub extern "C" fn nt_gdi_dd_can_create_surface(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NT_GDI_DD_CAN_CREATE_SURFACE");

    // DdCanCreateSurface: return 1 (DDERR_OK).
    1
}

/// NT_GDI_DD_CREATE_SURFACE - create DirectDraw surface
pub extern "C" fn nt_gdi_dd_create_surface(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NT_GDI_DD_CREATE_SURFACE");

    // DdCreateSurface: return 1 (DDERR_OK).
    1
}

/// NT_GDI_DD_DESTROY_SURFACE - Destroy DirectDraw surface
pub extern "C" fn nt_gdi_dd_destroy_surface(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NT_GDI_DD_DESTROY_SURFACE");

    // DdDestroySurface: return 1 (DDERR_OK).
    1
}

/// NT_GDI_DD_RESET_VISRGN - Reset visible region
pub extern "C" fn nt_gdi_dd_reset_visrgn(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NT_GDI_DD_RESET_VISRGN");

    // DdResetVisrgn: return 1 (DDERR_OK).
    1
}

/// NT_GDI_EXT_CREATE_PEN - Extended pen creation
pub use super::pen_services::nt_gdi_ext_create_pen;

/// NT_GDI_CREATE_PATTERN_BRUSH_INTERNAL - create pattern brush (internal)
pub extern "C" fn nt_gdi_create_pattern_brush_internal(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NT_GDI_CREATE_PATTERN_BRUSH_INTERNAL");

    // CreatePatternBrushInternal: return a pseudo-handle for the pattern brush.
    0x0009_0001
}

/// NT_GDI_GET_OUTLINE_TEXT_METRICS_INTERNAL_W - Get outline text metrics (internal)
pub extern "C" fn nt_gdi_get_outline_text_metrics_internal_w(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NT_GDI_GET_OUTLINE_TEXT_METRICS_INTERNAL_W");

    // GetOutlineTextMetricsInternalW: return 0 (no outline metrics without a font table).
    0
}

/// NT_GDI_SET_BITMAP_BITS - Set bitmap bits
pub extern "C" fn nt_gdi_set_bitmap_bits(frame: &SyscallFrame) -> u64 {
    super::bitmap::storage::nt_gdi_set_bitmap_bits(frame)
}

/// NT_GDI_CREATE_SOLID_BRUSH - create solid brush
pub extern "C" fn nt_gdi_create_solid_brush(frame: &SyscallFrame) -> u64 {
    // The second native argument is a preallocated user handle. Its ownership
    // protocol is not implemented; reject it rather than ignore the request.
    if frame.args[1] != 0 {
        return 0;
    }
    super::objects::gdi_create_solid_brush(frame.args[0] as u32)
}

/// NT_GDI_CREATE_CLIENT_OBJ - create client object
pub extern "C" fn nt_gdi_create_client_obj(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NT_GDI_CREATE_CLIENT_OBJ");

    // CreateClientObj: return a pseudo-handle for the client object.
    0x000B_0001
}

// =============================================================================
// USER Syscall Handlers - Window Management
// =============================================================================

/// NT_USER_GET_DC - Get device context
pub extern "C" fn nt_user_get_dc(frame: &SyscallFrame) -> u64 {
    let hwnd = unsafe { get_arg0(frame) };
    let _ = &hwnd;

    // kprintln!("[win32k] NT_USER_GET_DC: hwnd=0x{:016x}", hwnd);

    // create a display DC
    let dc = crate::libs::win32k::dc::gre_create_display_dc();
    let _ = &dc;

    // kprintln!("[win32k] NT_USER_GET_DC: returning DC=0x{:08x}", dc);
    dc
}

/// NT_USER_GET_DCEX - Get device context (extended)
pub extern "C" fn nt_user_get_dcex(frame: &SyscallFrame) -> u64 {
    let hwnd = unsafe { get_arg0(frame) };
    let _ = &hwnd;
    let hrgnclip = unsafe { get_arg1(frame) };
    let _ = &hrgnclip;
    let flags = unsafe { get_arg2(frame) } as u32;
    let _ = &flags;

    // kprintln!("[win32k] NT_USER_GET_DCEX: hwnd=0x{:x}, hrgnclip=0x{:x}, flags=0x{:08x}",
    //         hwnd, hrgnclip, flags);

    // For now, just create a standard display DC
    // In a full implementation, we would handle clipping regions and DC flags
    let dc = crate::libs::win32k::dc::gre_create_display_dc();
    let _ = &dc;

    // kprintln!("[win32k] NT_USER_GET_DCEX: returning DC=0x{:x}", dc);
    dc
}

/// Capture the counted LARGE_UNICODE_STRING used by native USER services.
/// The high bit of MaximumLength denotes an ANSI buffer, which this path
/// does not yet convert. Reject it rather than interpreting bytes as UTF-16.
fn capture_user_large_unicode_string(
    address: u64,
    maximum_chars: usize,
) -> Result<alloc::vec::Vec<u16>, i32> {
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct LargeString {
        length: u32,
        maximum_length_and_ansi: u32,
        buffer: u64,
    }
    if address == 0 {
        return Ok(alloc::vec![0]);
    }
    let header: LargeString = crate::mm::user_copy::read_user_object(address)?;
    let maximum = header.maximum_length_and_ansi & 0x7fff_ffff;
    let chars = header.length as usize / 2;
    if header.maximum_length_and_ansi & 0x8000_0000 != 0
        || header.length & 1 != 0
        || header.length > maximum
        || chars > maximum_chars
    {
        return Err(crate::libs::ntdll::status::STATUS_INVALID_PARAMETER);
    }
    let mut captured = alloc::vec::Vec::new();
    captured
        .try_reserve_exact(chars + 1)
        .map_err(|_| crate::libs::ntdll::status::STATUS_NO_MEMORY)?;
    captured.resize(chars + 1, 0);
    for (index, unit) in captured[..chars].iter_mut().enumerate() {
        let source = header
            .buffer
            .checked_add(index as u64 * 2)
            .ok_or(crate::libs::ntdll::status::STATUS_ACCESS_VIOLATION)?;
        crate::mm::user_copy::read_user(source, unit)?;
    }
    Ok(captured)
}

/// NtUserCreateWindowEx uses fifteen native parameters, including a class
/// version between the class and title. Its strings are counted descriptors,
/// unlike the twelve-parameter CreateWindowExW Win32 wrapper.
pub extern "C" fn nt_user_create_window_ex(frame: &SyscallFrame) -> u64 {
    let ex_style = frame.args[0] as u32;
    let class_name_ptr = frame.args[1];
    let class_version_ptr = frame.args[2];
    let window_name_ptr = frame.args[3];
    let style = frame.args[4] as u32;
    let x = frame.args[5] as i32;
    let y = frame.args[6] as i32;
    let width = frame.args[7] as i32;
    let height = frame.args[8] as i32;
    let parent = frame.args[9];
    let menu = frame.args[10];

    // Class atoms, instance/class-version resolution, creation callbacks and
    // the activation-context parameter remain unsupported by this manager.
    // Do not turn an integer atom into a user pointer.
    if class_name_ptr <= 0xffff || class_version_ptr > 0xffff {
        return 0;
    }
    let class_name_buf = match capture_user_large_unicode_string(class_name_ptr, 63) {
        Ok(name) => name,
        Err(_) => return 0,
    };
    if class_name_buf[0] == 0 {
        return 0;
    }
    let title_buf = match capture_user_large_unicode_string(window_name_ptr, 32767) {
        Ok(title) => title,
        Err(_) => return 0,
    };

    crate::libs::win32k::window::create_window_internal(
        &class_name_buf,
        &title_buf,
        style,
        ex_style,
        x,
        y,
        width,
        height,
        if parent != 0 { Some(parent) } else { None },
        0,
        menu as u32,
    )
    .unwrap_or_default()
}

/// NT_USER_DESTROY_WINDOW - Destroy window
pub extern "C" fn nt_user_destroy_window(frame: &SyscallFrame) -> u64 {
    let hwnd = unsafe { get_arg0(frame) };
    let _ = &hwnd;

    // kprintln!("[win32k] NT_USER_DESTROY_WINDOW: hwnd=0x{:x}", hwnd);

    if hwnd == 0 {
        return 0;
    }

    let success = crate::libs::win32k::window::destroy_window_internal(hwnd);
    let _ = &success;

    if success {
        1
    } else {
        0
    }
}

/// NT_USER_SHOW_WINDOW - Show/hide window
pub extern "C" fn nt_user_show_window(frame: &SyscallFrame) -> u64 {
    let hwnd = unsafe { get_arg0(frame) };
    let _ = &hwnd;
    let n_cmd_show = unsafe { get_arg1(frame) } as i32;
    let _ = &n_cmd_show;

    // kprintln!("[win32k] NT_USER_SHOW_WINDOW: hwnd=0x{:x}, cmd_show={}", hwnd, n_cmd_show);

    // Convert SW_* commands to boolean show state
    let show = match n_cmd_show {
        0 | 6 | 7 | 8 => false, // SW_HIDE, SW_MINIMIZE, SW_SHOWMINNOACTIVE, SW_FORCEMINIMIZE
        _ => true,              // SW_SHOW, SW_SHOWNORMAL, SW_SHOWMAXIMIZED, etc.
    };

    // Call the window manager to show/hide the window
    let success = crate::libs::win32k::window::show_window_internal(hwnd, show);
    let _ = &success;

    if success {
        1
    } else {
        0
    }
}

/// NT_USER_SET_WINDOW_POS - Set window position
pub extern "C" fn nt_user_set_window_pos(frame: &SyscallFrame) -> u64 {
    let hwnd = unsafe { get_arg0(frame) };
    let _ = &hwnd;
    let hwnd_insert_after = unsafe { get_arg1(frame) };
    let _ = &hwnd_insert_after;
    let x = unsafe { get_arg2(frame) } as i32;
    let _ = &x;
    let y = unsafe { get_arg3(frame) } as i32;
    let _ = &y;
    let cx = unsafe { get_arg4(frame) } as i32;
    let _ = &cx;
    let cy = unsafe { get_arg5(frame) } as i32;
    let _ = &cy;
    let u_flags = unsafe { get_arg6(frame) } as u32;
    let _ = &u_flags;

    // kprintln!("[win32k] NT_USER_SET_WINDOW_POS: hwnd=0x{:x}, insert_after=0x{:x}, pos=({},{}), size={}x{}, flags=0x{:08x}",
    //         hwnd, hwnd_insert_after, x, y, cx, cy, u_flags);

    // Check flags for SWP_NOSIZE
    let use_cx_cy = (u_flags & 0x0001) == 0;
    let _ = &use_cx_cy;
    // Check flags for SWP_NOMOVE
    let use_x_y = (u_flags & 0x0002) == 0;
    let _ = &use_x_y;

    // When SWP_NOMOVE / SWP_NOSIZE is set, keep the window's current
    // position or size instead of collapsing it to (0, 0) / 0x0.
    let (cur_x, cur_y, cur_cx, cur_cy) = {
        let wm = crate::libs::win32k::window::WINDOW_MANAGER.lock();
        match wm.get_window_rect(hwnd) {
            Some(rect) => (
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
            ),
            None => (x, y, cx, cy),
        }
    };
    let final_x = if use_x_y { x } else { cur_x };
    let _ = &final_x;
    let final_y = if use_x_y { y } else { cur_y };
    let _ = &final_y;
    let final_cx = if use_cx_cy { cx } else { cur_cx };
    let _ = &final_cx;
    let final_cy = if use_cx_cy { cy } else { cur_cy };
    let _ = &final_cy;

    // Call the window manager to set window position
    let success = crate::libs::win32k::window::set_window_pos_internal(
        hwnd, final_x, final_y, final_cx, final_cy,
    );

    if success {
        1
    } else {
        0
    }
}

pub use super::message::services::{
    nt_user_get_message, nt_user_peek_message, nt_user_post_message, nt_user_post_thread_message,
};

/// NT_USER_SET_FOCUS - Set focus window
pub extern "C" fn nt_user_set_focus(frame: &SyscallFrame) -> u64 {
    let hwnd = unsafe { get_arg0(frame) };
    let _ = &hwnd;

    // kprintln!("[win32k] NT_USER_SET_FOCUS: hwnd=0x{:x}", hwnd);

    // Call the window manager to set focus
    let result = crate::libs::win32k::window::set_focus_internal(hwnd);
    let _ = &result;

    // Return previous focus handle (0 if none)
    result.unwrap_or_default()
}

/// NT_USER_GET_FOREGROUND_WINDOW - Get foreground window
pub extern "C" fn nt_user_get_foreground_window(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NT_USER_GET_FOREGROUND_WINDOW");

    // Return the current foreground window handle
    crate::libs::win32k::window::get_foreground_window_internal()
}

/// NT_USER_SET_FOREGROUND_WINDOW - Set foreground window
pub extern "C" fn nt_user_set_foreground_window(frame: &SyscallFrame) -> u64 {
    let hwnd = unsafe { get_arg0(frame) };
    let _ = &hwnd;

    // kprintln!("[win32k] NT_USER_SET_FOREGROUND_WINDOW: hwnd=0x{:x}", hwnd);

    // Call the window manager to set foreground window
    let result = crate::libs::win32k::window::set_foreground_window_internal(hwnd);
    let _ = &result;

    result.unwrap_or_default()
}

pub use super::input_services::keyboard::{
    nt_user_get_async_key_state, nt_user_get_key_state, nt_user_get_keyboard_state,
    nt_user_set_keyboard_state,
};

/// NT_USER_SEND_INPUT - Send input events
pub extern "C" fn nt_user_send_input(frame: &SyscallFrame) -> u64 {
    let num_inputs = unsafe { get_arg0(frame) } as i32;
    let _ = &num_inputs;
    let inputs_ptr = unsafe { get_arg1(frame) };
    let _ = &inputs_ptr;
    let size = unsafe { get_arg2(frame) };
    let _ = &size;

    // kprintln!("[win32k] NT_USER_SEND_INPUT: num={}, ptr=0x{:x}, size={}",
    //               num_inputs, inputs_ptr, size);

    // Return number of events successfully injected
    // For now, return 0 (not implemented)
    0
}

/// NT_USER_TRANSLATE_MESSAGE - Translate virtual key message
pub use super::input_services::layout::nt_user_translate_message;

/// NT_USER_DISPATCH_MESSAGE - Dispatch message
pub extern "C" fn nt_user_dispatch_message(frame: &SyscallFrame) -> u64 {
    let msg_ptr = unsafe { get_arg0(frame) };
    let _ = &msg_ptr;

    // kprintln!("[win32k] NT_USER_DISPATCH_MESSAGE: msg_ptr=0x{:x}", msg_ptr);

    if msg_ptr == 0 {
        return 0;
    }

    let mut msg: crate::libs::win32k::message::Msg = unsafe { core::mem::zeroed() };
    if crate::mm::user_copy::read_user(msg_ptr, &mut msg).is_err() {
        return 0;
    }

    // kprintln!("[win32k] NT_USER_DISPATCH_MESSAGE: hwnd=0x{:x}, msg={:#x}",
    //         msg.hwnd, msg.message);

    // Call the default window procedure
    let result = crate::libs::win32k::message::default_wndproc(
        msg.hwnd,
        msg.message,
        msg.wparam,
        msg.lparam,
    );

    // kprintln!("[win32k] NT_USER_DISPATCH_MESSAGE: result=0x{:x}", result as u64);
    result as u64
}

pub use super::system_metrics::nt_user_get_system_metrics;

/// NT_USER_SYSTEM_PARAMETERS_INFO - System parameters
pub extern "C" fn nt_user_system_parameters_info(frame: &SyscallFrame) -> u64 {
    let ui_action = unsafe { get_arg0(frame) } as u32;
    let _ = &ui_action;
    let ui_param = unsafe { get_arg1(frame) };
    let _ = &ui_param;
    let pv_param = unsafe { get_arg2(frame) };
    let _ = &pv_param;
    let f_win_ini = unsafe { get_arg3(frame) };
    let _ = &f_win_ini;

    // kprintln!("[win32k] NT_USER_SYSTEM_PARAMETERS_INFO: action=0x{:x}", ui_action);

    // Most system parameters - return sensible defaults
    if pv_param != 0 {
        // Return 0 for get operations
    }
    1 // TRUE for most operations
}

/// NT_USER_MOVE_WINDOW - Move window
pub extern "C" fn nt_user_move_window(frame: &SyscallFrame) -> u64 {
    let hwnd = unsafe { get_arg0(frame) };
    let _ = &hwnd;
    let x = unsafe { get_arg1(frame) } as i32;
    let _ = &x;
    let y = unsafe { get_arg2(frame) } as i32;
    let _ = &y;
    let cx = unsafe { get_arg3(frame) } as i32;
    let _ = &cx;
    let cy = unsafe { get_arg4(frame) } as i32;
    let _ = &cy;
    let repaint = unsafe { get_arg5(frame) };
    let _ = &repaint;

    // kprintln!("[win32k] NT_USER_MOVE_WINDOW: hwnd=0x{:x}, pos=({},{}), size={}x{}, repaint={}",
    //         hwnd, x, y, cx, cy, repaint);

    // Call the window manager to set window position
    let success = crate::libs::win32k::window::set_window_pos_internal(hwnd, x, y, cx, cy);
    let _ = &success;

    // If repaint is requested, invalidate the window
    if repaint != 0 && success {
        crate::libs::win32k::window::invalidate_rect_internal(hwnd, None);
    }

    if success {
        1
    } else {
        0
    }
}

/// NtUserGetWindowRect - Get window rectangle
pub extern "C" fn nt_user_get_window_rect(frame: &SyscallFrame) -> u64 {
    let hwnd = unsafe { get_arg0(frame) };
    let _ = &hwnd;
    let rect = unsafe { get_arg1(frame) };
    let _ = &rect;

    // kprintln!("[win32k] NtUserGetWindowRect");

    let rect_ptr = unsafe { get_arg1(frame) };
    if rect_ptr == 0 {
        return 0;
    }
    let rect = crate::libs::win32k::window::WINDOW_MANAGER
        .lock()
        .get_window_rect(hwnd)
        .unwrap_or(crate::libs::win32k::objects::Rect {
            left: 0,
            top: 0,
            right: 800,
            bottom: 600,
        });
    let mut rect_bytes = [0u8; 16];
    rect_bytes[0..4].copy_from_slice(&rect.left.to_le_bytes());
    rect_bytes[4..8].copy_from_slice(&rect.top.to_le_bytes());
    rect_bytes[8..12].copy_from_slice(&rect.right.to_le_bytes());
    rect_bytes[12..16].copy_from_slice(&rect.bottom.to_le_bytes());
    if crate::mm::user_copy::copy_to_user(rect_ptr, &rect_bytes).is_ok() {
        1
    } else {
        0
    }
}

/// NtUserGetClientRect - Get client rectangle
pub extern "C" fn nt_user_get_client_rect(frame: &SyscallFrame) -> u64 {
    let hwnd = unsafe { get_arg0(frame) };
    let _ = &hwnd;
    let rect = unsafe { get_arg1(frame) };
    let _ = &rect;

    // kprintln!("[win32k] NtUserGetClientRect");

    let rect_ptr = unsafe { get_arg1(frame) };
    if rect_ptr == 0 {
        return 0;
    }
    let rect = crate::libs::win32k::window::WINDOW_MANAGER
        .lock()
        .get_client_rect(hwnd)
        .unwrap_or(crate::libs::win32k::objects::Rect {
            left: 0,
            top: 0,
            right: 800,
            bottom: 600,
        });
    let mut rect_bytes = [0u8; 16];
    rect_bytes[0..4].copy_from_slice(&rect.left.to_le_bytes());
    rect_bytes[4..8].copy_from_slice(&rect.top.to_le_bytes());
    rect_bytes[8..12].copy_from_slice(&rect.right.to_le_bytes());
    rect_bytes[12..16].copy_from_slice(&rect.bottom.to_le_bytes());
    if crate::mm::user_copy::copy_to_user(rect_ptr, &rect_bytes).is_ok() {
        1
    } else {
        0
    }
}

/// NT_USER_SET_CAPTURE - Set capture window
pub extern "C" fn nt_user_set_capture(frame: &SyscallFrame) -> u64 {
    let hwnd = unsafe { get_arg0(frame) };
    let _ = &hwnd;

    // kprintln!("[win32k] NT_USER_SET_CAPTURE");

    // SetCapture: return the previous capture window (0 = none).
    0
}

/// NtUserGetCapture - Get capture window
pub extern "C" fn nt_user_get_capture(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NtUserGetCapture");

    // GetCapture: return the current capture window (0 = none).
    0
}

/// NT_USER_SET_CURSOR - Set cursor
pub extern "C" fn nt_user_set_cursor(frame: &SyscallFrame) -> u64 {
    let hcur = unsafe { get_arg0(frame) };
    let _ = &hcur;

    // kprintln!("[win32k] NT_USER_SET_CURSOR");

    // SetCursor: return the previous cursor handle (0 = none).
    0
}

/// NtUserGetCursor - Get cursor
pub extern "C" fn nt_user_get_cursor(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NtUserGetCursor");

    // GetCursor: return the current cursor handle (0 = none).
    0
}

/// NT_USER_SET_TIMER - Set timer (P2 Enhancement)
pub extern "C" fn nt_user_set_timer(frame: &SyscallFrame) -> u64 {
    let hwnd = unsafe { get_arg0(frame) };
    let _ = &hwnd;
    let n_id_event = unsafe { get_arg1(frame) } as u32;
    let _ = &n_id_event;
    let u_elapse = unsafe { get_arg2(frame) } as u32;
    let _ = &u_elapse;
    let lp_timer_func = unsafe { get_arg3(frame) };
    let _ = &lp_timer_func;

    // kprintln!(
    //         "[win32k] NT_USER_SET_TIMER: hwnd={:#x}, id={}, interval={}ms, proc={:#x}",
    //         hwnd, n_id_event, u_elapse, lp_timer_func
    //     );

    // create a timer using the timer system
    match win32k_helpers::create_timer(hwnd, n_id_event, u_elapse, lp_timer_func) {
        Some(timer_id) => timer_id as u64,
        None => 0,
    }
}

/// NT_USER_KILL_TIMER - Kill timer (P2 Enhancement)
pub extern "C" fn nt_user_kill_timer(frame: &SyscallFrame) -> u64 {
    let hwnd = unsafe { get_arg0(frame) };
    let _ = &hwnd;
    let n_id_event = unsafe { get_arg1(frame) } as u32;
    let _ = &n_id_event;

    // kprintln!(
    //         "[win32k] NT_USER_KILL_TIMER: hwnd={:#x}, id={}",
    //         hwnd, n_id_event
    //     );

    // Kill the timer using the timer system
    if win32k_helpers::kill_timer(hwnd, n_id_event) {
        1 // TRUE
    } else {
        0 // FALSE
    }
}

/// NT_USER_INVALIDATE_RECT - Invalidate rectangle
pub extern "C" fn nt_user_invalidate_rect(frame: &SyscallFrame) -> u64 {
    let hwnd = unsafe { get_arg0(frame) };
    let _ = &hwnd;
    let rect = unsafe { get_arg1(frame) };
    let _ = &rect;
    let erase = unsafe { get_arg2(frame) };
    let _ = &erase;

    // kprintln!("[win32k] NT_USER_INVALIDATE_RECT");

    let rect = if rect != 0 {
        match crate::mm::user_copy::read_user_object::<crate::libs::win32k::objects::Rect>(rect) {
            Ok(rect) => Some(rect),
            Err(_) => return 0,
        }
    } else {
        None
    };
    crate::libs::win32k::paint::invalidate_rect(hwnd, rect, erase != 0) as u64
}

/// NT_USER_WAIT_MESSAGE - Wait for message
pub extern "C" fn nt_user_wait_message(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NT_USER_WAIT_MESSAGE");

    // WaitMessage: return 1 (a message became available, or we yield).
    1
}

/// NT_USER_CALL_NO_PARAM - Call with no parameters
pub extern "C" fn nt_user_call_no_param(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NT_USER_CALL_NO_PARAM");
    0
}

/// NT_USER_CALL_ONE_PARAM - Call with one parameter
pub extern "C" fn nt_user_call_one_param(frame: &SyscallFrame) -> u64 {
    let param = unsafe { get_arg0(frame) };
    let _ = &param;
    let code = unsafe { get_arg1(frame) };
    let _ = &code;

    // kprintln!("[win32k] NT_USER_CALL_ONE_PARAM");

    // CallOneParam: return the parameter as the result.
    param
}

/// NT_USER_CALL_TWO_PARAM - Call with two parameters
pub extern "C" fn nt_user_call_two_param(frame: &SyscallFrame) -> u64 {
    match frame.args[2] as u32 {
        // Verified against the Win7 SP1 user32 SetCaretPos/SetCursorPos wrappers.
        0x69 => u64::from(super::input_services::get_cursor(frame.args[0])),
        0x73 => {
            let (_, thread) = super::window::input_context::caller_identity();
            u64::from(super::user_state::with_user_state(|state| {
                state.set_caret_position(thread, frame.args[0] as i32, frame.args[1] as i32)
            }))
        }
        0x74 => u64::from(super::input_services::set_cursor(
            frame.args[0] as i32,
            frame.args[1] as i32,
        )),
        _ => 0,
    }
}

/// NT_USER_CALL_MSG_FILTER - Call message filter
pub extern "C" fn nt_user_call_msg_filter(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NT_USER_CALL_MSG_FILTER");
    0
}

/// NT_USER_CALL_NEXT_HOOK_EX - Call next hook
pub extern "C" fn nt_user_call_next_hook_ex(frame: &SyscallFrame) -> u64 {
    let hhk = unsafe { get_arg0(frame) };
    let _ = &hhk;
    let code = unsafe { get_arg1(frame) } as i32;
    let _ = &code;
    let wparam = unsafe { get_arg2(frame) };
    let _ = &wparam;
    let lparam = unsafe { get_arg3(frame) } as i64;
    let _ = &lparam;

    // kprintln!(
    //         "[win32k] NT_USER_CALL_NEXT_HOOK_EX: hhk={:#x}, code={}, wparam={:#x}, lparam={:#x}",
    //         hhk, code, wparam, lparam
    //     );

    // Hook chain: return 0 (no hooks registered).
    0
}

/// NtUserRegisterClass - Register window class
pub extern "C" fn nt_user_register_class(frame: &SyscallFrame) -> u64 {
    let wndclassexw_ptr = unsafe { get_arg0(frame) };
    let _ = &wndclassexw_ptr;
    let name_ptr = unsafe { get_arg1(frame) };
    let _ = &name_ptr;
    let instance = unsafe { get_arg2(frame) };
    let _ = &instance;
    let extra_bytes = unsafe { get_arg3(frame) };
    let _ = &extra_bytes;

    // kprintln!("[win32k] NtUserRegisterClass: wndclass=0x{:x}, name=0x{:x}, instance=0x{:x}",
    //               wndclassexw_ptr, name_ptr, instance);

    // Return ATOM value (non-zero for success)
    // A full implementation would:
    // 1. read WNDCLASSEX structure from user memory
    // 2. create internal WNDCLASS structure
    // 3. Add to global class table
    // 4. Return atom
    if wndclassexw_ptr != 0 {
        0xC000 | (1u16 as u64) // Return a non-zero value to indicate success
    } else {
        0
    }
}

/// NT_USER_UNREGISTER_CLASS - Unregister window class
pub extern "C" fn nt_user_unregister_class(frame: &SyscallFrame) -> u64 {
    let name_ptr = unsafe { get_arg0(frame) };
    let _ = &name_ptr;
    let instance = unsafe { get_arg1(frame) };
    let _ = &instance;
    let extra_bytes = unsafe { get_arg2(frame) };
    let _ = &extra_bytes;

    // kprintln!("[win32k] NT_USER_UNREGISTER_CLASS: name=0x{:x}, instance=0x{:x}",
    //               name_ptr, instance);

    // Return TRUE if successful
    1
}

/// NT_USER_GET_CLASS_NAME - Get window class name
pub extern "C" fn nt_user_get_class_name(frame: &SyscallFrame) -> u64 {
    let hwnd = unsafe { get_arg0(frame) };
    let _ = &hwnd;
    let buffer_ptr = unsafe { get_arg1(frame) };
    let _ = &buffer_ptr;
    let buffer_size = unsafe { get_arg2(frame) } as i32;
    let _ = &buffer_size;

    // kprintln!("[win32k] NT_USER_GET_CLASS_NAME: hwnd=0x{:x}, buffer=0x{:x}, size={}",
    //               hwnd, buffer_ptr, buffer_size);

    // Return 0 (failure) if buffer pointer is null
    // A full implementation would get class name from window
    if buffer_ptr == 0 || buffer_size == 0 {
        return 0;
    }

    // Return the number of characters copied
    0
}

/// NT_USER_REDRAW_WINDOW - Redraw window
pub extern "C" fn nt_user_redraw_window(frame: &SyscallFrame) -> u64 {
    let hwnd = unsafe { get_arg0(frame) };
    let _ = &hwnd;
    let rect = unsafe { get_arg1(frame) };
    let _ = &rect;
    let hrgn_update = unsafe { get_arg2(frame) };
    let _ = &hrgn_update;
    let flags = unsafe { get_arg3(frame) };
    let _ = &flags;

    // kprintln!("[win32k] NT_USER_REDRAW_WINDOW");

    let rect = if rect != 0 {
        match crate::mm::user_copy::read_user_object::<crate::libs::win32k::objects::Rect>(rect) {
            Ok(rect) => Some(rect),
            Err(_) => return 0,
        }
    } else {
        None
    };
    crate::libs::win32k::paint::redraw_window(hwnd, rect, hrgn_update, flags as u32) as u64
}

/// NT_USER_VALIDATE_RECT - Validate rectangle
pub extern "C" fn nt_user_validate_rect(frame: &SyscallFrame) -> u64 {
    let hwnd = unsafe { get_arg0(frame) };
    let _ = &hwnd;
    let rect = unsafe { get_arg1(frame) };
    let _ = &rect;

    // kprintln!("[win32k] NT_USER_VALIDATE_RECT");

    let rect = if rect != 0 {
        match crate::mm::user_copy::read_user_object::<crate::libs::win32k::objects::Rect>(rect) {
            Ok(rect) => Some(rect),
            Err(_) => return 0,
        }
    } else {
        None
    };
    crate::libs::win32k::paint::validate_rect(hwnd, rect) as u64
}

/// NtUserValidateRgn - Validate region
pub extern "C" fn nt_user_validate_rgn(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NtUserValidateRgn");
    1 // TRUE
}

/// NT_USER_GET_UPDATE_RECT - Get update rectangle
pub extern "C" fn nt_user_get_update_rect(frame: &SyscallFrame) -> u64 {
    let hwnd = unsafe { get_arg0(frame) };
    let _ = &hwnd;
    let rect = unsafe { get_arg1(frame) };
    let _ = &rect;
    let erase = unsafe { get_arg2(frame) };
    let _ = &erase;

    // kprintln!("[win32k] NT_USER_GET_UPDATE_RECT");

    // GetUpdateRect: return 0 (no update rect pending).
    0
}

/// NT_USER_GET_UPDATE_RGN - Get update region
pub extern "C" fn nt_user_get_update_rgn(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NT_USER_GET_UPDATE_RGN");
    0
}

/// NT_USER_CREATE_CARET - create caret
pub extern "C" fn nt_user_create_caret(frame: &SyscallFrame) -> u64 {
    let hwnd = unsafe { get_arg0(frame) };
    let _ = &hwnd;
    let bitmap = unsafe { get_arg1(frame) };
    let _ = &bitmap;
    let width = unsafe { get_arg2(frame) };
    let _ = &width;
    let height = unsafe { get_arg3(frame) };
    let _ = &height;

    // kprintln!("[win32k] NT_USER_CREATE_CARET");

    if crate::libs::win32k::user_state::create_caret(crate::libs::win32k::user_state::Caret {
        hwnd,
        bitmap,
        width: width as i32,
        height: height as i32,
        visible: false,
    }) {
        1
    } else {
        0
    }
}

/// NT_USER_SHOW_CARET - Show caret
pub extern "C" fn nt_user_show_caret(frame: &SyscallFrame) -> u64 {
    let hwnd = unsafe { get_arg0(frame) };
    let _ = &hwnd;

    // kprintln!("[win32k] NT_USER_SHOW_CARET");

    crate::libs::win32k::user_state::show_caret(hwnd, true) as u64
}

/// NT_USER_HIDE_CARET - Hide caret
pub extern "C" fn nt_user_hide_caret(frame: &SyscallFrame) -> u64 {
    let hwnd = unsafe { get_arg0(frame) };
    let _ = &hwnd;

    // kprintln!("[win32k] NT_USER_HIDE_CARET");

    crate::libs::win32k::user_state::show_caret(hwnd, false) as u64
}

/// NT_USER_END_DEFER_WINDOW_POS_EX - End defer window position (extended)
pub extern "C" fn nt_user_end_defer_window_pos_ex(frame: &SyscallFrame) -> u64 {
    let h_wnd_pos_info = unsafe { get_arg0(frame) };
    let _ = &h_wnd_pos_info;
    let flags = unsafe { get_arg1(frame) };
    let _ = &flags;

    // kprintln!("[win32k] NT_USER_END_DEFER_WINDOW_POS_EX");

    if crate::libs::win32k::user_state::end_deferred_positions(h_wnd_pos_info, flags as u32) {
        1
    } else {
        0
    }
}

/// NT_USER_DEFER_WINDOW_POS - Defer window position
pub extern "C" fn nt_user_defer_window_pos(frame: &SyscallFrame) -> u64 {
    let h_wnd_pos_info = unsafe { get_arg0(frame) };
    let _ = &h_wnd_pos_info;
    let hwnd = unsafe { get_arg1(frame) };
    let _ = &hwnd;
    let hwnd_insert_after = unsafe { get_arg2(frame) };
    let _ = &hwnd_insert_after;
    let x = unsafe { get_arg3(frame) };
    let _ = &x;
    let y = unsafe { get_arg4(frame) };
    let _ = &y;
    let cx = unsafe { get_arg5(frame) };
    let _ = &cx;
    let cy = unsafe { get_arg6(frame) };
    let _ = &cy;
    let flags = unsafe { get_arg7(frame) };
    let _ = &flags;

    // kprintln!("[win32k] NT_USER_DEFER_WINDOW_POS");

    let position = crate::libs::win32k::user_state::DeferredWindowPosition {
        hwnd,
        insert_after: hwnd_insert_after,
        x: x as i32,
        y: y as i32,
        width: cx as i32,
        height: cy as i32,
        flags: flags as u32,
    };
    crate::libs::win32k::user_state::defer_window_position(h_wnd_pos_info, position).unwrap_or(0)
}

/// NT_USER_REGISTER_WINDOW_MESSAGE - Register window message
pub extern "C" fn nt_user_register_window_message(frame: &SyscallFrame) -> u64 {
    let s = unsafe { get_arg0(frame) };
    let _ = &s;

    // kprintln!("[win32k] NT_USER_REGISTER_WINDOW_MESSAGE");

    // RegisterWindowMessage: return a unique message ID (0xC000+).
    0xC000
}

/// NT_USER_SEND_MESSAGE - Send message to window
pub extern "C" fn nt_user_send_message(frame: &SyscallFrame) -> u64 {
    let hwnd = unsafe { get_arg0(frame) };
    let _ = &hwnd;
    let msg = unsafe { get_arg1(frame) } as u32;
    let _ = &msg;
    let w_param = unsafe { get_arg2(frame) };
    let _ = &w_param;
    let l_param = unsafe { get_arg3(frame) };
    let _ = &l_param;

    // kprintln!("[win32k] NT_USER_SEND_MESSAGE: hwnd=0x{:x}, msg={:#x}, wparam=0x{:x}, lparam=0x{:x}",
    //         hwnd, msg, w_param, l_param);

    // For WmQuit, return 0
    if msg == 0x0012 {
        return 0;
    }

    // Call the default window procedure for the window
    let result = crate::libs::win32k::message::default_wndproc(hwnd, msg, w_param, l_param as i64);
    let _ = &result;

    // kprintln!("[win32k] NT_USER_SEND_MESSAGE: result=0x{:x}", result as u64);
    result as u64
}

/// NT_USER_REPLY_MESSAGE - Reply to GetMessage
pub extern "C" fn nt_user_reply_message(frame: &SyscallFrame) -> u64 {
    let result = unsafe { get_arg0(frame) };
    let _ = &result;

    // kprintln!("[win32k] NT_USER_REPLY_MESSAGE");

    // ReplyMessage: return TRUE.
    1
}

/// NT_USER_FIND_WINDOW_EX - Find window (extended)
pub extern "C" fn nt_user_find_window_ex(frame: &SyscallFrame) -> u64 {
    let hwnd_parent = unsafe { get_arg0(frame) };
    let _ = &hwnd_parent;
    let hwnd_child_after = unsafe { get_arg1(frame) };
    let _ = &hwnd_child_after;
    let class_name = unsafe { get_arg2(frame) };
    let _ = &class_name;
    let window_name = unsafe { get_arg3(frame) };
    let _ = &window_name;
    let flags = unsafe { get_arg4(frame) };
    let _ = &flags;

    // kprintln!("[win32k] NT_USER_FIND_WINDOW_EX");

    // FindWindowEx: return 0 (no matching window).
    0
}

/// NtUserFindWindow - Find window
pub extern "C" fn nt_user_find_window(frame: &SyscallFrame) -> u64 {
    let class_name = unsafe { get_arg0(frame) };
    let _ = &class_name;
    let window_name = unsafe { get_arg1(frame) };
    let _ = &window_name;

    // kprintln!("[win32k] NtUserFindWindow");

    // FindWindow: return 0 (no matching window).
    0
}

/// NT_USER_GET_ACTIVE_WINDOW - Get active window
pub extern "C" fn nt_user_get_active_window(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NT_USER_GET_ACTIVE_WINDOW");

    // Return the current active window handle
    crate::libs::win32k::window::get_active_window_internal()
}

/// NT_USER_SET_ACTIVE_WINDOW - Set active window
pub extern "C" fn nt_user_set_active_window(frame: &SyscallFrame) -> u64 {
    let hwnd = unsafe { get_arg0(frame) };
    let _ = &hwnd;

    // kprintln!("[win32k] NT_USER_SET_ACTIVE_WINDOW");

    // SetActiveWindow: return the previous active window (0 = none).
    0
}

/// NtUserGetFocus - Get focus window
pub extern "C" fn nt_user_get_focus(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NtUserGetFocus");

    // Return the current focus window handle
    crate::libs::win32k::window::get_focus_internal()
}

/// NT_USER_RELEASE_CAPTURE - Release capture
pub extern "C" fn nt_user_release_capture(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NT_USER_RELEASE_CAPTURE");

    crate::libs::win32k::window::release_capture_internal();
    1
}

/// NT_USER_GET_CONTROL_BRUSH - Get control brush
pub extern "C" fn nt_user_get_control_brush(frame: &SyscallFrame) -> u64 {
    let hwnd = unsafe { get_arg0(frame) };
    let _ = &hwnd;
    let color = unsafe { get_arg1(frame) };
    let _ = &color;
    let r#type = unsafe { get_arg2(frame) };
    let _ = &r#type;

    // kprintln!("[win32k] NT_USER_GET_CONTROL_BRUSH");

    // GetControlBrush: return 0 (no control brush).
    0
}

/// NT_USER_DRAW_ICON_EX - Draw icon extended
pub extern "C" fn nt_user_draw_icon_ex(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NT_USER_DRAW_ICON_EX");
    1 // TRUE
}

/// NT_USER_GET_SYSTEM_MENU - Get system menu
pub extern "C" fn nt_user_get_system_menu(frame: &SyscallFrame) -> u64 {
    let hwnd = unsafe { get_arg0(frame) };
    let _ = &hwnd;
    let revert = unsafe { get_arg1(frame) };
    let _ = &revert;

    // kprintln!("[win32k] NT_USER_GET_SYSTEM_MENU");

    // GetSystemMenu: return 0 (no system menu).
    0
}

/// NT_USER_INTERNAL_GET_WINDOW_TEXT - internal get window text
pub use super::window::text::nt_user_internal_get_window_text;

/// NT_USER_GET_WINDOW_DC - Get window DC
pub extern "C" fn nt_user_get_window_dc(frame: &SyscallFrame) -> u64 {
    let hwnd = unsafe { get_arg0(frame) };
    let _ = &hwnd;

    // kprintln!("[win32k] NT_USER_GET_WINDOW_DC");

    // GetWindowDC: return a pseudo-DC handle.
    0x000D_0001
}

/// NT_USER_SCROLL_DC - Scroll DC
pub extern "C" fn nt_user_scroll_dc(frame: &SyscallFrame) -> u64 {
    let hdc = unsafe { get_arg0(frame) };
    let _ = &hdc;
    let dx = unsafe { get_arg1(frame) };
    let _ = &dx;
    let dy = unsafe { get_arg2(frame) };
    let _ = &dy;
    let scroll_rect = unsafe { get_arg3(frame) };
    let _ = &scroll_rect;
    let clip_rect = unsafe { get_arg4(frame) };
    let _ = &clip_rect;
    let hrgn_update = unsafe { get_arg5(frame) };
    let _ = &hrgn_update;
    let prgn_update = unsafe { get_arg6(frame) };
    let _ = &prgn_update;

    // kprintln!("[win32k] NT_USER_SCROLL_DC");

    // ScrollDC: return TRUE.
    1
}

/// NT_USER_UNHOOK_WINDOWS_HOOK_EX - Unhook windows hook (extended) (P2 Enhancement)
pub extern "C" fn nt_user_unhook_windows_hook_ex(frame: &SyscallFrame) -> u64 {
    let hhk = unsafe { get_arg0(frame) };
    let _ = &hhk;

    // kprintln!("[win32k] NT_USER_UNHOOK_WINDOWS_HOOK_EX: hhk={:#x}", hhk);

    if hhk == 0 {
        return 0;
    }

    if win32k_helpers::remove_hook(hhk) {
        1 // TRUE
    } else {
        0 // FALSE
    }
}

/// NT_USER_NOTIFY_PROCESS_CREATE - Notify process create
pub extern "C" fn nt_user_notify_process_create(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NT_USER_NOTIFY_PROCESS_CREATE");
    0
}

/// NT_USER_GET_TITLE_BAR_INFO - Get title bar info
pub extern "C" fn nt_user_get_title_bar_info(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NT_USER_GET_TITLE_BAR_INFO");
    0
}

/// NT_USER_GET_SCROLL_BAR_INFO - Get scroll bar info
pub extern "C" fn nt_user_get_scroll_bar_info(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NT_USER_GET_SCROLL_BAR_INFO");
    0
}

/// NT_USER_SET_WINDOW_FNID - Set window FNID
pub extern "C" fn nt_user_set_window_fnid(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NT_USER_SET_WINDOW_FNID");
    0
}

/// NT_USER_CALC_MENU_BAR - Calculate menu bar
pub extern "C" fn nt_user_calc_menu_bar(frame: &SyscallFrame) -> u64 {
    let hwnd = unsafe { get_arg0(frame) };
    let _ = &hwnd;
    let x = unsafe { get_arg1(frame) };
    let _ = &x;
    let y = unsafe { get_arg2(frame) };
    let _ = &y;
    let flags = unsafe { get_arg3(frame) };
    let _ = &flags;
    let p_rect = unsafe { get_arg4(frame) };
    let _ = &p_rect;

    // kprintln!("[win32k] NT_USER_CALC_MENU_BAR");

    // CalcMenuBar: return 0 (no menu bar height computed).
    0
}

/// NT_USER_THUNKED_MENU_ITEM_INFO - Thunked menu item info
pub extern "C" fn nt_user_thunked_menu_item_info(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NT_USER_THUNKED_MENU_ITEM_INFO");
    0
}

/// NT_USER_GET_ATOM_NAME - Get atom name
pub extern "C" fn nt_user_get_atom_name(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NT_USER_GET_ATOM_NAME");
    0
}

/// NT_USER_SET_CURSOR_ICON_DATA - Set cursor icon data
pub extern "C" fn nt_user_set_cursor_icon_data(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NT_USER_SET_CURSOR_ICON_DATA");
    0
}

/// NT_USER_GET_DOUBLE_CLICK_TIME - Get double click time
pub extern "C" fn nt_user_get_double_click_time(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NT_USER_GET_DOUBLE_CLICK_TIME");

    crate::libs::win32k::user_state::double_click_time() as u64
}

/// NT_USER_ENABLE_SCROLL_BAR - Enable scroll bar
pub extern "C" fn nt_user_enable_scroll_bar(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NT_USER_ENABLE_SCROLL_BAR");
    0
}

/// NT_USER_GET_CLASS_INFO_EX - Get class info (extended)
pub extern "C" fn nt_user_get_class_info_ex(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NT_USER_GET_CLASS_INFO_EX");
    0
}

/// NT_USER_DELETE_MENU - Delete menu item (P2 Enhancement)
pub extern "C" fn nt_user_delete_menu(frame: &SyscallFrame) -> u64 {
    let h_menu = unsafe { get_arg0(frame) };
    let _ = &h_menu;
    let u_position = unsafe { get_arg1(frame) };
    let _ = &u_position;
    let u_flags = unsafe { get_arg2(frame) };
    let _ = &u_flags;

    // kprintln!("[win32k] NT_USER_DELETE_MENU: hmenu={:#x}, pos={}, flags={:#x}",
    //               h_menu, u_position, u_flags);

    crate::libs::win32k::user_state::delete_menu_item(h_menu, u_position as u32, u_flags as u32)
        as u64
}

/// NT_USER_SCROLL_WINDOW_EX - Scroll window (extended)
pub extern "C" fn nt_user_scroll_window_ex(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NT_USER_SCROLL_WINDOW_EX");
    0
}

/// NT_USER_GET_MENU_BAR_INFO - Get menu bar info
pub extern "C" fn nt_user_get_menu_bar_info(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NT_USER_GET_MENU_BAR_INFO");
    0
}

/// NT_USER_GET_CLIPBOARD_SEQUENCE_NUMBER - Get clipboard sequence number
pub extern "C" fn nt_user_get_clipboard_sequence_number(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NT_USER_GET_CLIPBOARD_SEQUENCE_NUMBER");

    // GetClipboardSequenceNumber: report the real mutation counter kept
    // by the clipboard manager instead of a fixed constant.
    crate::libs::win32k::clipboard::get_sequence_number()
}

/// NT_USER_GET_KEYBOARD_LAYOUT_LIST - Get keyboard layout list
pub use super::input_services::layout::nt_user_get_keyboard_layout_list;

/// NT_USER_MAP_VIRTUAL_KEY_EX - Map virtual key (extended)
pub use super::input_services::layout::nt_user_map_virtual_key_ex;

/// NT_USER_TO_UNICODE_EX - Convert to unicode (extended)
pub use super::input_services::layout::nt_user_to_unicode_ex;

/// NT_USER_DEF_SET_TEXT - Default set text
pub use super::window::text::nt_user_def_set_text;

/// NT_USER_GET_ICON_SIZE - Get icon size
pub extern "C" fn nt_user_get_icon_size(frame: &SyscallFrame) -> u64 {
    let hwnd = unsafe { get_arg0(frame) };
    let _ = &hwnd;
    let r#type = unsafe { get_arg1(frame) };
    let _ = &r#type;
    let cx = unsafe { get_arg2(frame) };
    let _ = &cx;
    let cy = unsafe { get_arg3(frame) };
    let _ = &cy;

    // kprintln!("[win32k] NT_USER_GET_ICON_SIZE");

    // GetIconSize: return 32x32 (SM_ICON default).
    (32 << 16) | 32
}

/// NT_USER_FILL_WINDOW - Fill window
pub extern "C" fn nt_user_fill_window(frame: &SyscallFrame) -> u64 {
    let hwnd = unsafe { get_arg0(frame) };
    let _ = &hwnd;
    let h_dc = unsafe { get_arg1(frame) };
    let _ = &h_dc;
    let brush = unsafe { get_arg2(frame) };
    let _ = &brush;
    let result = unsafe { get_arg3(frame) };
    let _ = &result;

    // kprintln!("[win32k] NT_USER_FILL_WINDOW");

    // FillWindow: return TRUE.
    1
}

/// NT_USER_SET_WINDOWS_HOOK_EX - Set windows hook (extended) (P2 Enhancement)
pub extern "C" fn nt_user_set_windows_hook_ex(frame: &SyscallFrame) -> u64 {
    let id_hook = unsafe { get_arg0(frame) } as i32;
    let _ = &id_hook;
    let lpfn = unsafe { get_arg1(frame) };
    let _ = &lpfn;
    let mod_name = unsafe { get_arg2(frame) };
    let _ = &mod_name;
    let thread_id = unsafe { get_arg3(frame) } as u32;
    let _ = &thread_id;

    // kprintln!(
    //         "[win32k] NT_USER_SET_WINDOWS_HOOK_EX: type={}, proc={:#x}, thread_id={}",
    //         id_hook, lpfn, thread_id
    //     );

    // Install the hook using the hook system
    win32k_helpers::install_hook(id_hook, lpfn, thread_id).unwrap_or_default()
}

/// NT_USER_EXCLUDE_UPDATE_RGN - Exclude update region
pub extern "C" fn nt_user_exclude_update_rgn(frame: &SyscallFrame) -> u64 {
    let hdc = unsafe { get_arg0(frame) };
    let _ = &hdc;
    let hwnd = unsafe { get_arg1(frame) };
    let _ = &hwnd;

    // kprintln!("[win32k] NT_USER_EXCLUDE_UPDATE_RGN");

    // ExcludeUpdateRgn: return 1 (SIMPLEREGION).
    1
}

/// NT_USER_CALL_HWND_PARAM - Call with hwnd and param
pub extern "C" fn nt_user_call_hwnd_param(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NT_USER_CALL_HWND_PARAM");
    0
}

/// NT_USER_GET_ICON_INFO - Get icon info
pub extern "C" fn nt_user_get_icon_info(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NT_USER_GET_ICON_INFO");
    0
}

/// NT_USER_SBGET_PARMS - Get scroll bar parameters
pub extern "C" fn nt_user_sb_get_parms(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NT_USER_SBGET_PARMS");
    0
}

/// NT_USER_DESTROY_CURSOR - Destroy cursor
pub extern "C" fn nt_user_destroy_cursor(frame: &SyscallFrame) -> u64 {
    let hcur = unsafe { get_arg0(frame) };
    let _ = &hcur;
    let flags = unsafe { get_arg1(frame) };
    let _ = &flags;

    // kprintln!("[win32k] NT_USER_DESTROY_CURSOR");

    crate::libs::win32k::user_state::destroy_cursor(hcur) as u64
}

/// NT_USER_MESSAGE_CALL - Message call
pub extern "C" fn nt_user_message_call(frame: &SyscallFrame) -> u64 {
    if frame.args[5] as u32 != 0x029e {
        return 0;
    }
    super::message::default_wndproc(
        frame.args[0],
        frame.args[1] as u32,
        frame.args[2],
        frame.args[3] as i64,
    ) as u64
}

/// NT_USER_END_MENU - End menu
pub extern "C" fn nt_user_end_menu(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NT_USER_END_MENU");

    crate::libs::win32k::user_state::end_menu() as u64
}

/// NT_USER_QUERY_WINDOW - Query window info
pub extern "C" fn nt_user_query_window(frame: &SyscallFrame) -> u64 {
    let hwnd = unsafe { get_arg0(frame) };
    let _ = &hwnd;
    let cmd = unsafe { get_arg1(frame) };
    let _ = &cmd;

    // kprintln!("[win32k] NT_USER_QUERY_WINDOW");

    // QueryWindow: return 0 (no window info available).
    0
}

/// NT_USER_TRANSLATE_ACCELERATOR - Translate accelerator
pub extern "C" fn nt_user_translate_accelerator(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NT_USER_TRANSLATE_ACCELERATOR");
    0
}

/// NT_USER_VALIDATE_TIMER_CALLBACK - Validate timer callback
pub extern "C" fn nt_user_validate_timer_callback(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NT_USER_VALIDATE_TIMER_CALLBACK");
    0
}

/// NT_USER_BEGIN_PAINT - Begin paint
pub extern "C" fn nt_user_begin_paint(frame: &SyscallFrame) -> u64 {
    let hwnd = unsafe { get_arg0(frame) };
    let _ = &hwnd;
    let paint = unsafe { get_arg1(frame) };
    let _ = &paint;

    // kprintln!("[win32k] NT_USER_BEGIN_PAINT");

    // BeginPaint: return a pseudo-DC handle.
    0x0011_0001
}

/// NT_USER_END_PAINT - End paint
pub extern "C" fn nt_user_end_paint(frame: &SyscallFrame) -> u64 {
    let hwnd = unsafe { get_arg0(frame) };
    let _ = &hwnd;
    let paint = unsafe { get_arg1(frame) };
    let _ = &paint;

    // kprintln!("[win32k] NT_USER_END_PAINT");

    crate::libs::win32k::paint::end_paint(hwnd, paint) as u64
}

/// NT_USER_BUILD_HWND_LIST - Build window list
pub extern "C" fn nt_user_build_hwnd_list(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NT_USER_BUILD_HWND_LIST");
    0
}

/// NT_USER_BUILD_NAME_LIST - Build window name list
pub extern "C" fn nt_user_build_name_list(frame: &SyscallFrame) -> u64 {
    let hwnd = unsafe { get_arg0(frame) };
    let _ = &hwnd;
    let names_ptr = unsafe { get_arg1(frame) };
    let _ = &names_ptr;
    let max_count = unsafe { get_arg2(frame) };
    let _ = &max_count;

    // kprintln!("[win32k] NT_USER_BUILD_NAME_LIST: hwnd=0x{:x}, names=0x{:x}, max={}", hwnd, names_ptr, max_count);

    // BuildNameList: return 0 (no name list built).
    0
}

/// NT_USER_CALL_HWND_LOCK - Call with hwnd lock
pub extern "C" fn nt_user_call_hwnd_lock(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NT_USER_CALL_HWND_LOCK");
    0
}

/// NT_USER_CALL_HWND_PARAM_LOCK - Call with hwnd and param lock
pub extern "C" fn nt_user_call_hwnd_param_lock(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NT_USER_CALL_HWND_PARAM_LOCK");
    0
}

/// NT_USER_VK_KEY_SCAN_EX - Virtual key scan (extended)
pub use super::input_services::layout::nt_user_vk_key_scan_ex;

/// NT_USER_COPY_ACCELERATOR_TABLE - Copy accelerator table
pub extern "C" fn nt_user_copy_accelerator_table(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NT_USER_COPY_ACCELERATOR_TABLE");
    0
}

/// NT_USER_NOTIFY_WIN_EVENT - Notify win event
pub extern "C" fn nt_user_notify_win_event(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NT_USER_NOTIFY_WIN_EVENT");
    0
}

/// NT_USER_IS_CLIPBOARD_FORMAT_AVAILABLE - Check clipboard format
pub extern "C" fn nt_user_is_clipboard_format_available(frame: &SyscallFrame) -> u64 {
    let format = unsafe { get_arg0(frame) };
    let _ = &format;

    // kprintln!("[win32k] NT_USER_IS_CLIPBOARD_FORMAT_AVAILABLE");

    super::clipboard::formats().contains(&(format as u32)) as u64
}

/// NT_USER_SET_SCROLL_INFO - Set scroll info
pub extern "C" fn nt_user_set_scroll_info(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NT_USER_SET_SCROLL_INFO");
    0
}

/// NT_USER_REGISTER_HOT_KEY - Register hot key
pub extern "C" fn nt_user_register_hot_key(frame: &SyscallFrame) -> u64 {
    let hwnd = unsafe { get_arg0(frame) };
    let _ = &hwnd;
    let id = unsafe { get_arg1(frame) };
    let _ = &id;
    let fs_modifiers = unsafe { get_arg2(frame) };
    let _ = &fs_modifiers;
    let vk = unsafe { get_arg3(frame) };
    let _ = &vk;

    // kprintln!("[win32k] NT_USER_REGISTER_HOT_KEY");

    if crate::libs::win32k::user_state::register_hot_key(crate::libs::win32k::user_state::HotKey {
        hwnd,
        id: id as u32,
        modifiers: fs_modifiers as u32,
        virtual_key: vk as u32,
    }) {
        1
    } else {
        0
    }
}

/// NT_USER_UNREGISTER_HOT_KEY - Unregister hot key
pub extern "C" fn nt_user_unregister_hot_key(frame: &SyscallFrame) -> u64 {
    let hwnd = unsafe { get_arg0(frame) };
    let _ = &hwnd;
    let id = unsafe { get_arg1(frame) };
    let _ = &id;

    // kprintln!("[win32k] NT_USER_UNREGISTER_HOT_KEY");

    crate::libs::win32k::user_state::unregister_hot_key(hwnd, id as u32) as u64
}

/// NT_USER_GET_CURSOR_INFO - Get cursor info
pub extern "C" fn nt_user_get_cursor_info(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NT_USER_GET_CURSOR_INFO");
    0
}

/// NT_USER_SHOW_SCROLL_BAR - Show scroll bar
pub extern "C" fn nt_user_show_scroll_bar(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NT_USER_SHOW_SCROLL_BAR");
    1 // TRUE
}

/// NT_USER_FIND_EXISTING_CURSOR_ICON - Find existing cursor icon
pub extern "C" fn nt_user_find_existing_cursor_icon(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NT_USER_FIND_EXISTING_CURSOR_ICON");
    0
}

/// NtUserGetKeyboardType - Get keyboard type
pub extern "C" fn nt_user_get_keyboard_type(frame: &SyscallFrame) -> u64 {
    let type_flag = unsafe { get_arg0(frame) };
    let _ = &type_flag;

    // kprintln!("[win32k] NtUserGetKeyboardType");

    match type_flag as i32 {
        0 => 4,  // KEYBOARD_TYPE_GENERIC
        1 => 1,  // KEYBOARD_TYPE_I8042
        2 => 12, // Number of function keys
        _ => 0,
    }
}

// =============================================================================
// Additional USER Syscall Handlers
// =============================================================================

/// NT_USER_BIT_BLT_SYS_BMP - bit_blt to system bitmap
pub extern "C" fn nt_user_bit_blt_sys_bmp(frame: &SyscallFrame) -> u64 {
    let hdc = unsafe { get_arg0(frame) };
    let _ = &hdc;
    let x = unsafe { get_arg1(frame) };
    let _ = &x;
    let y = unsafe { get_arg2(frame) };
    let _ = &y;
    let cx = unsafe { get_arg3(frame) };
    let _ = &cx;
    let cy = unsafe { get_arg4(frame) };
    let _ = &cy;
    let rop = unsafe { get_arg5(frame) };
    let _ = &rop;

    // kprintln!("[win32k] NT_USER_BIT_BLT_SYS_BMP");
    1 // TRUE
}

/// NT_USER_INVALIDATE_RGN - Invalidate region
pub extern "C" fn nt_user_invalidate_rgn(frame: &SyscallFrame) -> u64 {
    let hwnd = unsafe { get_arg0(frame) };
    let _ = &hwnd;
    let hrgn = unsafe { get_arg1(frame) };
    let _ = &hrgn;
    let erase = unsafe { get_arg2(frame) };
    let _ = &erase;

    // kprintln!("[win32k] NT_USER_INVALIDATE_RGN");
    1 // TRUE
}

/// NT_USER_SET_WINDOW_RGN - Set window region
pub extern "C" fn nt_user_set_window_rgn(frame: &SyscallFrame) -> u64 {
    super::window::native_state::nt_user_set_window_rgn_ex(frame)
}

/// NT_USER_OPEN_CLIPBOARD - Open clipboard
pub extern "C" fn nt_user_open_clipboard(frame: &SyscallFrame) -> u64 {
    let hwnd = unsafe { get_arg0(frame) };
    let _ = &hwnd;

    // kprintln!("[win32k] NT_USER_OPEN_CLIPBOARD: hwnd=0x{:x}", hwnd);

    if crate::libs::win32k::clipboard::open_clipboard(hwnd) {
        1 // TRUE
    } else {
        0 // FALSE - already open
    }
}

/// NT_USER_CLOSE_CLIPBOARD - close clipboard
pub extern "C" fn nt_user_close_clipboard(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NT_USER_CLOSE_CLIPBOARD");

    if crate::libs::win32k::clipboard::close_clipboard() {
        1 // TRUE
    } else {
        0 // FALSE
    }
}

/// NT_USER_EMPTY_CLIPBOARD - Empty clipboard
pub extern "C" fn nt_user_empty_clipboard(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NT_USER_EMPTY_CLIPBOARD");

    if crate::libs::win32k::clipboard::empty_clipboard() {
        1 // TRUE
    } else {
        0 // FALSE
    }
}

/// NT_USER_SET_CLIPBOARD_DATA - Set clipboard data
pub extern "C" fn nt_user_set_clipboard_data(frame: &SyscallFrame) -> u64 {
    let format = unsafe { get_arg0(frame) };
    let _ = &format;
    let hdata = unsafe { get_arg1(frame) };
    let _ = &hdata;

    // The user-mode clipboard ABI has no defined buffer/size encoding
    // for 'hdata' yet, so there is no safe way to copy caller data
    // into the kernel clipboard. Report failure instead of pretending
    // the object was accepted.
    0 // FALSE
}

/// NT_USER_ENABLE_MENU_ITEM - Enable/disable menu item
pub extern "C" fn nt_user_enable_menu_item(frame: &SyscallFrame) -> u64 {
    let hmenu = unsafe { get_arg0(frame) };
    let _ = &hmenu;
    let id = unsafe { get_arg1(frame) };
    let _ = &id;
    let enable = unsafe { get_arg2(frame) };
    let _ = &enable;

    // kprintln!("[win32k] NT_USER_ENABLE_MENU_ITEM");
    0 // MF_BYCOMMAND
}

/// NT_USER_ALTER_WINDOW_STYLE - Alter window style
pub extern "C" fn nt_user_alter_window_style(frame: &SyscallFrame) -> u64 {
    let hwnd = unsafe { get_arg0(frame) };
    let _ = &hwnd;
    let remove = unsafe { get_arg1(frame) };
    let _ = &remove;
    let add = unsafe { get_arg2(frame) };
    let _ = &add;

    // kprintln!("[win32k] NT_USER_ALTER_WINDOW_STYLE");
    1 // TRUE
}

/// NT_USER_SET_THREAD_STATE - Set thread state
pub extern "C" fn nt_user_set_thread_state(frame: &SyscallFrame) -> u64 {
    let state = unsafe { get_arg0(frame) };
    let _ = &state;

    // kprintln!("[win32k] NT_USER_SET_THREAD_STATE");
    0
}

/// NT_USER_TRACK_MOUSE_EVENT - Track mouse event
pub extern "C" fn nt_user_track_mouse_event(frame: &SyscallFrame) -> u64 {
    let event = unsafe { get_arg0(frame) };
    let _ = &event;

    // kprintln!("[win32k] NT_USER_TRACK_MOUSE_EVENT");
    1 // TRUE
}

/// NT_USER_DESTROY_MENU - Destroy menu
pub extern "C" fn nt_user_destroy_menu(frame: &SyscallFrame) -> u64 {
    let hmenu = unsafe { get_arg0(frame) };
    let _ = &hmenu;

    // kprintln!("[win32k] NT_USER_DESTROY_MENU");
    crate::libs::win32k::user_state::destroy_menu(hmenu) as u64
}

/// NT_USER_CONSOLE_CONTROL - Console control
pub extern "C" fn nt_user_console_control(frame: &SyscallFrame) -> u64 {
    let code = unsafe { get_arg0(frame) };
    let _ = &code;
    let in_data = unsafe { get_arg1(frame) };
    let _ = &in_data;
    let out_data = unsafe { get_arg2(frame) };
    let _ = &out_data;

    // kprintln!("[win32k] NT_USER_CONSOLE_CONTROL");
    0
}

/// NT_USER_SET_INFORMATION_THREAD - Set thread information
pub extern "C" fn nt_user_set_information_thread(frame: &SyscallFrame) -> u64 {
    let hwnd = unsafe { get_arg0(frame) };
    let _ = &hwnd;
    let info_class = unsafe { get_arg1(frame) };
    let _ = &info_class;
    let info = unsafe { get_arg2(frame) };
    let _ = &info;
    let length = unsafe { get_arg3(frame) };
    let _ = &length;

    // kprintln!("[win32k] NT_USER_SET_INFORMATION_THREAD");
    1 // TRUE
}

/// NT_USER_GET_CONTROL_COLOR - Get control color
pub extern "C" fn nt_user_get_control_color(frame: &SyscallFrame) -> u64 {
    let hwnd = unsafe { get_arg0(frame) };
    let _ = &hwnd;
    let hdc = unsafe { get_arg1(frame) };
    let _ = &hdc;
    let class = unsafe { get_arg2(frame) };
    let _ = &class;

    // kprintln!("[win32k] NT_USER_GET_CONTROL_COLOR");
    0 // Default brush
}

/// NT_USER_GET_CLIPBOARD_FORMAT_NAME - Get clipboard format name
pub extern "C" fn nt_user_get_clipboard_format_name(frame: &SyscallFrame) -> u64 {
    let format = unsafe { get_arg0(frame) };
    let _ = &format;
    let buffer = unsafe { get_arg1(frame) };
    let _ = &buffer;
    let buffer_size = unsafe { get_arg2(frame) };
    let _ = &buffer_size;

    // kprintln!("[win32k] NT_USER_GET_CLIPBOARD_FORMAT_NAME");
    0
}

/// NT_USER_REAL_INTERNAL_GET_MESSAGE - Real internal get message
pub extern "C" fn nt_user_real_internal_get_message(frame: &SyscallFrame) -> u64 {
    let msg = unsafe { get_arg0(frame) };
    let _ = &msg;
    let hwnd = unsafe { get_arg1(frame) };
    let _ = &hwnd;
    let msg_min = unsafe { get_arg2(frame) };
    let _ = &msg_min;
    let msg_max = unsafe { get_arg3(frame) };
    let _ = &msg_max;
    let flags = unsafe { get_arg4(frame) };
    let _ = &flags;

    // kprintln!("[win32k] NT_USER_REAL_INTERNAL_GET_MESSAGE");
    0
}

/// NT_USER_CREATE_LOCAL_MEM_HANDLE - create local memory handle
pub extern "C" fn nt_user_create_local_mem_handle(frame: &SyscallFrame) -> u64 {
    let addr = unsafe { get_arg0(frame) };
    let _ = &addr;
    let size = unsafe { get_arg1(frame) };
    let _ = &size;

    // kprintln!("[win32k] NT_USER_CREATE_LOCAL_MEM_HANDLE");
    0
}

/// NT_USER_ATTACH_THREAD_INPUT - Attach thread input
pub extern "C" fn nt_user_attach_thread_input(frame: &SyscallFrame) -> u64 {
    let attach_from = unsafe { get_arg0(frame) };
    let _ = &attach_from;
    let attach_to = unsafe { get_arg1(frame) };
    let _ = &attach_to;
    let attach = unsafe { get_arg2(frame) };
    let _ = &attach;

    // kprintln!("[win32k] NT_USER_ATTACH_THREAD_INPUT");
    1 // TRUE
}

/// NT_USER_PAINT_MENU_BAR - Paint menu bar
pub extern "C" fn nt_user_paint_menu_bar(frame: &SyscallFrame) -> u64 {
    let hwnd = unsafe { get_arg0(frame) };
    let _ = &hwnd;
    let hdc = unsafe { get_arg1(frame) };
    let _ = &hdc;
    let x = unsafe { get_arg2(frame) };
    let _ = &x;
    let y = unsafe { get_arg3(frame) };
    let _ = &y;
    let cx = unsafe { get_arg4(frame) };
    let _ = &cx;
    let cy = unsafe { get_arg5(frame) };
    let _ = &cy;

    // kprintln!("[win32k] NT_USER_PAINT_MENU_BAR");
    0
}

/// NT_USER_CREATE_ACCELERATOR_TABLE - create accelerator table
pub extern "C" fn nt_user_create_accelerator_table(frame: &SyscallFrame) -> u64 {
    let table = unsafe { get_arg0(frame) };
    let _ = &table;

    // kprintln!("[win32k] NT_USER_CREATE_ACCELERATOR_TABLE");
    0 // NULL
}

/// NT_USER_GET_CURSOR_FRAME_INFO - Get cursor frame info
pub extern "C" fn nt_user_get_cursor_frame_info(frame: &SyscallFrame) -> u64 {
    let hcur = unsafe { get_arg0(frame) };
    let _ = &hcur;
    let frame = unsafe { get_arg1(frame) };
    let _ = &frame;

    // kprintln!("[win32k] NT_USER_GET_CURSOR_FRAME_INFO");
    0
}

/// NT_USER_GET_ALT_TAB_INFO - Get alt-tab info
pub extern "C" fn nt_user_get_alt_tab_info(frame: &SyscallFrame) -> u64 {
    let hwnd = unsafe { get_arg0(frame) };
    let _ = &hwnd;
    let info = unsafe { get_arg1(frame) };
    let _ = &info;

    // kprintln!("[win32k] NT_USER_GET_ALT_TAB_INFO");
    0
}

/// NT_USER_GET_CARET_BLINK_TIME - Get caret blink time
pub extern "C" fn nt_user_get_caret_blink_time(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NT_USER_GET_CARET_BLINK_TIME");
    530 // Default blink time
}

/// NT_USER_PROCESS_CONNECT - Process connect
pub extern "C" fn nt_user_process_connect(frame: &SyscallFrame) -> u64 {
    let _ = (frame.args[0], frame.args[1]);
    // USER shared-info mapping is not implemented. This native call returns
    // NTSTATUS; returning TRUE used to conceal an uninitialized connect block.
    0xc00000bb
}

/// NT_USER_ENUM_DISPLAY_DEVICES - Enum display devices
pub extern "C" fn nt_user_enum_display_devices(frame: &SyscallFrame) -> u64 {
    let device = unsafe { get_arg0(frame) };
    let _ = &device;
    let index = unsafe { get_arg1(frame) };
    let _ = &index;
    let info = unsafe { get_arg2(frame) };
    let _ = &info;
    let flags = unsafe { get_arg3(frame) };
    let _ = &flags;

    // kprintln!("[win32k] NT_USER_ENUM_DISPLAY_DEVICES");
    0 // FALSE (no more devices)
}

/// NT_USER_GET_CLIPBOARD_DATA - Get clipboard data
pub extern "C" fn nt_user_get_clipboard_data(frame: &SyscallFrame) -> u64 {
    let format = unsafe { get_arg0(frame) };
    let _ = &format;

    // No user-mode clipboard ABI exists yet: there is no way to hand
    // the caller a global memory handle they can read. NULL is the
    // honest "no object" response until that ABI is designed.
    0 // NULL
}

/// NT_USER_REMOVE_MENU - Remove menu item
pub extern "C" fn nt_user_remove_menu(frame: &SyscallFrame) -> u64 {
    let hmenu = unsafe { get_arg0(frame) };
    let _ = &hmenu;
    let pos = unsafe { get_arg1(frame) };
    let _ = &pos;
    let flags = unsafe { get_arg2(frame) };
    let _ = &flags;

    // kprintln!("[win32k] NT_USER_REMOVE_MENU");
    1 // TRUE
}

/// NT_USER_CONVERT_MEM_HANDLE - Convert memory handle
pub extern "C" fn nt_user_convert_mem_handle(frame: &SyscallFrame) -> u64 {
    let handle = unsafe { get_arg0(frame) };
    let _ = &handle;

    // kprintln!("[win32k] NT_USER_CONVERT_MEM_HANDLE");
    0
}

/// NT_USER_DESTROY_ACCELERATOR_TABLE - Destroy accelerator table
pub extern "C" fn nt_user_destroy_accelerator_table(frame: &SyscallFrame) -> u64 {
    let table = unsafe { get_arg0(frame) };
    let _ = &table;

    // kprintln!("[win32k] NT_USER_DESTROY_ACCELERATOR_TABLE");
    1 // TRUE
}

/// NT_USER_GET_GUITHREAD_INFO - Get GUI thread info
pub extern "C" fn nt_user_get_gui_thread_info(frame: &SyscallFrame) -> u64 {
    let thread_id = unsafe { get_arg0(frame) };
    let _ = &thread_id;
    let info = unsafe { get_arg1(frame) };
    let _ = &info;

    // kprintln!("[win32k] NT_USER_GET_GUITHREAD_INFO");
    0
}

/// NT_USER_SET_WINDOWS_HOOK_AW - Set windows hook ANSI/Wide (P2 Enhancement)
pub extern "C" fn nt_user_set_windows_hook_aw(frame: &SyscallFrame) -> u64 {
    let id_hook = unsafe { get_arg0(frame) } as i32;
    let _ = &id_hook;
    let lpfn = unsafe { get_arg1(frame) };
    let _ = &lpfn;
    let flags = unsafe { get_arg2(frame) };
    let _ = &flags;

    // kprintln!(
    //         "[win32k] NT_USER_SET_WINDOWS_HOOK_AW: type={}, proc={:#x}",
    //         id_hook, lpfn
    //     );

    // Install the hook (similar to SetWindowsHookEx)
    win32k_helpers::install_hook(id_hook, lpfn, 0).unwrap_or_default()
}

/// NT_USER_SET_MENU_DEFAULT_ITEM - Set menu default item
pub extern "C" fn nt_user_set_menu_default_item(frame: &SyscallFrame) -> u64 {
    let hmenu = unsafe { get_arg0(frame) };
    let _ = &hmenu;
    let item = unsafe { get_arg1(frame) };
    let _ = &item;
    let by_pos = unsafe { get_arg2(frame) };
    let _ = &by_pos;

    // kprintln!("[win32k] NT_USER_SET_MENU_DEFAULT_ITEM");
    0 // Previous default
}

/// NT_USER_CHECK_MENU_ITEM - Check menu item
pub extern "C" fn nt_user_check_menu_item(frame: &SyscallFrame) -> u64 {
    let hmenu = unsafe { get_arg0(frame) };
    let _ = &hmenu;
    let id = unsafe { get_arg1(frame) };
    let _ = &id;
    let check = unsafe { get_arg2(frame) };
    let _ = &check;

    // kprintln!("[win32k] NT_USER_CHECK_MENU_ITEM");
    0xFFFFFFFF // MF_BYCOMMAND
}

/// NT_USER_LOCK_WINDOW_UPDATE - Lock window update
pub extern "C" fn nt_user_lock_window_update(frame: &SyscallFrame) -> u64 {
    let hwnd = unsafe { get_arg0(frame) };
    let _ = &hwnd;

    // kprintln!("[win32k] NT_USER_LOCK_WINDOW_UPDATE");
    1 // TRUE
}

/// NT_USER_SET_SYSTEM_MENU - Set system menu
pub extern "C" fn nt_user_set_system_menu(frame: &SyscallFrame) -> u64 {
    let hwnd = unsafe { get_arg0(frame) };
    let _ = &hwnd;
    let hmenu = unsafe { get_arg1(frame) };
    let _ = &hmenu;

    // kprintln!("[win32k] NT_USER_SET_SYSTEM_MENU");
    0 // Previous menu
}

/// NT_USER_THUNKED_MENU_INFO - Thunked menu info
pub extern "C" fn nt_user_thunked_menu_info(frame: &SyscallFrame) -> u64 {
    let hmenu = unsafe { get_arg0(frame) };
    let _ = &hmenu;
    let data = unsafe { get_arg1(frame) };
    let _ = &data;

    // kprintln!("[win32k] NT_USER_THUNKED_MENU_INFO");
    0
}

/// NT_USER_CALL_HWND - Call with hwnd
pub extern "C" fn nt_user_call_hwnd(frame: &SyscallFrame) -> u64 {
    let hwnd = unsafe { get_arg0(frame) };
    let _ = &hwnd;
    let msg = unsafe { get_arg1(frame) };
    let _ = &msg;

    // kprintln!("[win32k] NT_USER_CALL_HWND");
    0
}

/// NT_USER_DDE_INITIALIZE - DDE initialize
pub extern "C" fn nt_user_dde_initialize(frame: &SyscallFrame) -> u64 {
    let pid = unsafe { get_arg0(frame) };
    let _ = &pid;
    let filters = unsafe { get_arg1(frame) };
    let _ = &filters;
    let flags = unsafe { get_arg2(frame) };
    let _ = &flags;

    // kprintln!("[win32k] NT_USER_DDE_INITIALIZE");
    0 // DMLERR_NO_ERROR
}

/// NT_USER_MODIFY_USER_STARTUP_INFO_FLAGS - Modify user startup info flags
pub extern "C" fn nt_user_modify_user_startup_info_flags(frame: &SyscallFrame) -> u64 {
    let flags = unsafe { get_arg0(frame) };
    let _ = &flags;
    let enable = unsafe { get_arg1(frame) };
    let _ = &enable;

    // kprintln!("[win32k] NT_USER_MODIFY_USER_STARTUP_INFO_FLAGS");
    1 // TRUE
}

/// NT_USER_COUNT_CLIPBOARD_FORMATS - count clipboard formats
pub extern "C" fn nt_user_count_clipboard_formats(_frame: &SyscallFrame) -> u64 {
    super::clipboard::formats().len() as u64
}

/// NT_USER_ENUM_DISPLAY_SETTINGS - Enum display settings
pub extern "C" fn nt_user_enum_display_settings(frame: &SyscallFrame) -> u64 {
    let device = unsafe { get_arg0(frame) };
    let _ = &device;
    let index = unsafe { get_arg1(frame) };
    let _ = &index;
    let mode = unsafe { get_arg2(frame) };
    let _ = &mode;
    let flags = unsafe { get_arg3(frame) };
    let _ = &flags;

    // kprintln!("[win32k] NT_USER_ENUM_DISPLAY_SETTINGS");
    0 // FALSE (no more modes)
}

/// NT_USER_PAINT_DESKTOP - Paint desktop
pub extern "C" fn nt_user_paint_desktop(frame: &SyscallFrame) -> u64 {
    let hdc = unsafe { get_arg0(frame) };
    let _ = &hdc;

    // kprintln!("[win32k] NT_USER_PAINT_DESKTOP");
    1 // TRUE
}

/// NT_USER_SHOW_WINDOW_ASYNC - Show window async
pub extern "C" fn nt_user_show_window_async(frame: &SyscallFrame) -> u64 {
    let hwnd = unsafe { get_arg0(frame) };
    let _ = &hwnd;
    let cmd_show = unsafe { get_arg1(frame) };
    let _ = &cmd_show;

    // kprintln!("[win32k] NT_USER_SHOW_WINDOW_ASYNC");
    1 // TRUE
}

/// NT_USER_ACTIVATE_KEYBOARD_LAYOUT - Activate keyboard layout
pub extern "C" fn nt_user_activate_keyboard_layout(frame: &SyscallFrame) -> u64 {
    let hkl = unsafe { get_arg0(frame) };
    let _ = &hkl;
    let flags = unsafe { get_arg1(frame) };
    let _ = &flags;

    // kprintln!("[win32k] NT_USER_ACTIVATE_KEYBOARD_LAYOUT");
    1 // TRUE
}

/// NT_USER_INITIALIZE_CLIENT_PFN_ARRAYS - Initialize client procedure function arrays
pub extern "C" fn nt_user_initialize_client_pfn_arrays(frame: &SyscallFrame) -> u64 {
    let arg1 = unsafe { get_arg0(frame) };
    let _ = &arg1;
    let arg2 = unsafe { get_arg1(frame) };
    let _ = &arg2;
    let arg3 = unsafe { get_arg2(frame) };
    let _ = &arg3;
    let arg4 = unsafe { get_arg3(frame) };
    let _ = &arg4;

    // kprintln!("[win32k] NT_USER_INITIALIZE_CLIENT_PFN_ARRAYS");
    1 // TRUE
}

// =============================================================================
// Dialog System Syscalls (P2 Enhancement)
// =============================================================================

fn find_dialog_item(parent: u64, control_id: u32) -> Option<u64> {
    let manager = crate::libs::win32k::window::WINDOW_MANAGER.lock();
    let children = manager.get_window(parent)?.children.clone();
    children.into_iter().find(|child| {
        manager
            .get_window(*child)
            .map(|window| window.control_id == control_id)
            .unwrap_or(false)
    })
}

/// NtUserMessageBox - Display a message box (P2 Enhancement)
pub extern "C" fn nt_user_message_box(frame: &SyscallFrame) -> u64 {
    let hwnd = unsafe { get_arg0(frame) };
    let _ = &hwnd;
    let lp_text = unsafe { get_arg1(frame) };
    let _ = &lp_text;
    let lp_caption = unsafe { get_arg2(frame) };
    let _ = &lp_caption;
    let u_type = unsafe { get_arg3(frame) };
    let _ = &u_type;

    // kprintln!(
    //         "[win32k] NtUserMessageBox: hwnd={:#x}, type={:#x}",
    //         hwnd, u_type
    //     );

    // Simplified implementation: Return IDOK for MB_OK type
    if u_type & 0x00000007 == (win32k_helpers::get_mb_ok() as u64) {
        return win32k_helpers::get_idok() as u64;
    }

    // For other types, return a default value
    0
}

/// NtUserGetDlgItem - Get dialog item (P2 Enhancement)
pub extern "C" fn nt_user_get_dlg_item(frame: &SyscallFrame) -> u64 {
    let hwnd = unsafe { get_arg0(frame) };
    let _ = &hwnd;
    let n_id = unsafe { get_arg1(frame) };
    let _ = &n_id;

    // kprintln!("[win32k] NtUserGetDlgItem: hwnd={:#x}, id={}", hwnd, n_id);

    find_dialog_item(hwnd, n_id as u32).unwrap_or(0)
}

/// NtUserEndDialog - End dialog (P2 Enhancement)
pub extern "C" fn nt_user_end_dialog(frame: &SyscallFrame) -> u64 {
    let hwnd = unsafe { get_arg0(frame) };
    let _ = &hwnd;
    let n_result = unsafe { get_arg1(frame) };
    let _ = &n_result;

    // kprintln!("[win32k] NtUserEndDialog: hwnd={:#x}", hwnd);

    if hwnd == 0 || !crate::libs::win32k::window::destroy_window_internal(hwnd) {
        return 0;
    }
    n_result
}

/// NtUserSetDlgItemInt - Set dialog item integer (P2 Enhancement)
pub extern "C" fn nt_user_set_dlg_item_int(frame: &SyscallFrame) -> u64 {
    let hwnd = unsafe { get_arg0(frame) };
    let _ = &hwnd;
    let n_id = unsafe { get_arg1(frame) };
    let _ = &n_id;
    let u_value = unsafe { get_arg2(frame) };
    let _ = &u_value;
    let b_signed = unsafe { get_arg3(frame) };
    let _ = &b_signed;

    // kprintln!("[win32k] NtUserSetDlgItemInt");

    let Some(child) = find_dialog_item(hwnd, n_id as u32) else {
        return 0;
    };
    let value = if b_signed != 0 {
        (u_value as i32).unsigned_abs() as u64
    } else {
        u_value
    };
    let mut digits = alloc::vec::Vec::new();
    let mut remainder = value;
    if remainder == 0 {
        digits.push(b'0');
    }
    while remainder > 0 {
        digits.push(b'0' + (remainder % 10) as u8);
        remainder /= 10;
    }
    if b_signed != 0 && (u_value as i32) < 0 {
        digits.push(b'-');
    }
    digits.reverse();
    let mut title = alloc::vec![0u16; digits.len() + 1];
    for (index, &digit) in digits.iter().enumerate() {
        title[index] = digit as u16;
    }

    let mut manager = crate::libs::win32k::window::WINDOW_MANAGER.lock();
    let Some(window) = manager.get_window_mut(child) else {
        return 0;
    };
    window.title = title;
    window.dirty = true;
    1
}

/// NtUserGetDlgItemInt - Get dialog item integer (P2 Enhancement)
pub extern "C" fn nt_user_get_dlg_item_int(frame: &SyscallFrame) -> u64 {
    let hwnd = unsafe { get_arg0(frame) };
    let _ = &hwnd;
    let n_id = unsafe { get_arg1(frame) };
    let _ = &n_id;
    let lp_translated = unsafe { get_arg2(frame) };
    let _ = &lp_translated;
    let b_signed = unsafe { get_arg3(frame) };
    let _ = &b_signed;

    // kprintln!("[win32k] NtUserGetDlgItemInt");

    // GetDlgItemInt: return 0 (no value).
    0
}

// =============================================================================
// Additional Menu Syscalls (P2 Enhancement)
// =============================================================================

/// NtUserCreateMenu - create menu (P2 Enhancement)
pub extern "C" fn nt_user_create_menu(frame: &SyscallFrame) -> u64 {
    let u_flags = unsafe { get_arg0(frame) };
    let _ = &u_flags;

    // kprintln!("[win32k] NtUserCreateMenu");

    // Allocate a menu handle
    let handle = win32k_helpers::allocate_menu_handle();
    if !crate::libs::win32k::user_state::create_menu(handle, false) {
        return 0;
    }
    // kprintln!("[win32k] NtUserCreateMenu: created menu handle={:#x}", handle);
    handle
}

/// NtUserCreatePopupMenu - create popup menu (P2 Enhancement)
pub extern "C" fn nt_user_create_popup_menu(_frame: &SyscallFrame) -> u64 {
    // kprintln!("[win32k] NtUserCreatePopupMenu");

    // Allocate a menu handle with popup style
    let handle = win32k_helpers::allocate_menu_handle();
    if !crate::libs::win32k::user_state::create_menu(handle, true) {
        return 0;
    }
    // kprintln!("[win32k] NtUserCreatePopupMenu: created popup menu handle={:#x}", handle);
    handle
}

/// NtUserAppendMenu - Append menu item (P2 Enhancement)
pub extern "C" fn nt_user_append_menu(frame: &SyscallFrame) -> u64 {
    let h_menu = unsafe { get_arg0(frame) };
    let _ = &h_menu;
    let u_flags = unsafe { get_arg1(frame) };
    let _ = &u_flags;
    let u_id_new_item = unsafe { get_arg2(frame) };
    let _ = &u_id_new_item;
    let lp_new_item = unsafe { get_arg3(frame) };
    let _ = &lp_new_item;

    // kprintln!(
    //         "[win32k] NtUserAppendMenu: hmenu={:#x}, flags={:#x}, id={}",
    //         h_menu, u_flags, u_id_new_item
    //     );

    let mut text = alloc::vec![0u16; 256];
    if lp_new_item != 0 {
        match unsafe { capture_user_unicode_string(lp_new_item, 255, &mut text) } {
            Ok(_) => {}
            Err(_) => return 0,
        }
    } else {
        text.clear();
    }

    if crate::libs::win32k::user_state::append_menu_item(
        h_menu,
        u_flags as u32,
        u_id_new_item as u32,
        &text,
    ) {
        1
    } else {
        0
    }
}

/// NtUserTrackPopupMenu - Track popup menu (P2 Enhancement)
pub extern "C" fn nt_user_track_popup_menu(frame: &SyscallFrame) -> u64 {
    let h_menu = unsafe { get_arg0(frame) };
    let _ = &h_menu;
    let u_flags = unsafe { get_arg1(frame) };
    let _ = &u_flags;
    let x = unsafe { get_arg2(frame) } as i32;
    let _ = &x;
    let y = unsafe { get_arg3(frame) } as i32;
    let _ = &y;

    // kprintln!(
    //         "[win32k] NtUserTrackPopupMenu: hmenu={:#x}, x={}, y={}",
    //         h_menu, x, y
    //     );

    // TrackPopupMenu: return 0 (no item selected).
    0
}

/// NtUserGetMenu - Get window menu (P2 Enhancement)
pub extern "C" fn nt_user_get_menu(frame: &SyscallFrame) -> u64 {
    let hwnd = unsafe { get_arg0(frame) };
    let _ = &hwnd;

    // kprintln!("[win32k] NtUserGetMenu: hwnd={:#x}", hwnd);

    crate::libs::win32k::user_state::window_menu(hwnd)
}

/// NT_USER_SET_MENU - Set window menu (P2 Enhancement)
pub extern "C" fn nt_user_set_menu(frame: &SyscallFrame) -> u64 {
    let hwnd = unsafe { get_arg0(frame) };
    let _ = &hwnd;
    let h_menu = unsafe { get_arg1(frame) };
    let _ = &h_menu;

    // kprintln!("[win32k] NT_USER_SET_MENU: hwnd={:#x}, hmenu={:#x}", hwnd, h_menu);

    crate::libs::win32k::user_state::set_window_menu(hwnd, h_menu).unwrap_or(0)
}

/// NtUserDrawMenuBar - Draw menu bar (P2 Enhancement)
pub extern "C" fn nt_user_draw_menu_bar(frame: &SyscallFrame) -> u64 {
    let hwnd = unsafe { get_arg0(frame) };
    let _ = &hwnd;

    // kprintln!("[win32k] NtUserDrawMenuBar: hwnd={:#x}", hwnd);

    let menu = crate::libs::win32k::user_state::window_menu(hwnd);
    if menu == 0 {
        return 0;
    }
    let valid_window = crate::libs::win32k::window::WINDOW_MANAGER
        .lock()
        .get_window(hwnd)
        .is_some();
    if valid_window {
        crate::libs::win32k::window::invalidate_rect_internal(hwnd, None);
        1
    } else {
        0
    }
}

// =============================================================================
// Additional Hook Syscalls (needed for Shadow SSDT registration)
// =============================================================================

/// NT_USER_SET_WIN_EVENT_HOOK - Set winevent hook
pub extern "C" fn nt_user_set_win_event_hook(frame: &SyscallFrame) -> u64 {
    let event_min = unsafe { get_arg0(frame) } as u32;
    let _ = &event_min;
    let event_max = unsafe { get_arg1(frame) } as u32;
    let _ = &event_max;
    let h_mod = unsafe { get_arg2(frame) };
    let _ = &h_mod;
    let lpfn = unsafe { get_arg3(frame) };
    let _ = &lpfn;
    let id_process = unsafe { get_arg4(frame) } as u32;
    let _ = &id_process;
    let id_thread = unsafe { get_arg5(frame) } as u32;
    let _ = &id_thread;
    let dw_flags = unsafe { get_arg6(frame) } as u32;
    let _ = &dw_flags;

    // kprintln!(
    //         "[win32k] NT_USER_SET_WIN_EVENT_HOOK: event={}-{}, process={}, thread={}",
    //         event_min, event_max, id_process, id_thread
    //     );

    let handle = 0x0013_0001u64;
    if !crate::libs::win32k::user_state::set_event_hook(handle) {
        return 0;
    }
    handle
}

/// NT_USER_UNHOOK_WIN_EVENT - Unhook winevent
pub extern "C" fn nt_user_unhook_win_event(frame: &SyscallFrame) -> u64 {
    let h_win_event_hook = unsafe { get_arg0(frame) };
    let _ = &h_win_event_hook;

    // kprintln!("[win32k] NT_USER_UNHOOK_WIN_EVENT");

    crate::libs::win32k::user_state::remove_event_hook(h_win_event_hook) as u64
}

pub use super::raster::extended::{
    nt_gdi_ext_flood_fill, nt_gdi_gradient_fill, nt_gdi_transparent_blt,
};
pub use super::raster::parallelogram::nt_gdi_plg_blt;
pub use super::raster::{
    nt_gdi_fill_rgn, nt_gdi_frame_rgn, nt_gdi_get_pixel, nt_gdi_invert_rgn, nt_gdi_set_pixel,
};

// Shared USER state services; each export retains the window lifetime guard.
pub use super::window::services::*;

/// Drawing is immediate in this GDI backend; there is no deferred batch to
/// drain. The full fence publishes completed CPU surface writes.
pub extern "C" fn nt_gdi_flush(_frame: &SyscallFrame) -> u64 {
    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
    1
}

pub use super::dc::pixel_format::*;
pub use super::font_services::linking::*;
pub use super::objects::attributes::*;

pub use super::input_services::history::nt_user_get_mouse_move_points_ex;
pub use super::input_services::{
    nt_user_block_input, nt_user_clip_cursor, nt_user_get_caret_pos, nt_user_get_clip_cursor,
    nt_user_get_key_name_text, nt_user_get_keyboard_layout_name, nt_user_get_thread_state,
    nt_user_unload_keyboard_layout,
};

pub use super::raw_input::{
    nt_user_get_raw_input_buffer, nt_user_get_raw_input_data, nt_user_get_raw_input_device_info,
    nt_user_get_raw_input_device_list, nt_user_get_registered_raw_input_devices,
    nt_user_register_raw_input_devices,
};

pub use super::display_config::*;

pub use super::color_space::*;

pub use super::window::session_services::*;

pub use super::resources::nt_user_get_gui_resources;
