//! A single-buffer, true-color GDI framebuffer format. No OpenGL, stereo,
//! hardware acceleration or double buffering is advertised by this backend.
use super::{get_dc, DC_TYPE_MEMORY};
use crate::arch::common::syscall_frame::SyscallFrame;
use crate::mm::user_copy::copy_to_user;

fn descriptor() -> [u8; 40] {
    let mut data = [0u8; 40];
    data[..2].copy_from_slice(&40u16.to_le_bytes());
    data[2..4].copy_from_slice(&1u16.to_le_bytes());
    // PFD_DRAW_TO_WINDOW | PFD_SUPPORT_GDI | PFD_GENERIC_FORMAT.
    data[4..8].copy_from_slice(&0x70u32.to_le_bytes());
    data[9] = 32;
    data[10] = 8;
    data[11] = 16;
    data[12] = 8;
    data[13] = 8;
    data[14] = 8;
    data
}

pub extern "C" fn nt_gdi_describe_pixel_format(frame: &SyscallFrame) -> u64 {
    let Some(dc) = get_dc(frame.args[0]) else {
        return 0;
    };
    if dc.dc_type == DC_TYPE_MEMORY {
        return 0;
    }
    if frame.args[3] == 0 {
        return 1;
    }
    if frame.args[1] != 1 || frame.args[2] < 40 {
        return 0;
    }
    if copy_to_user(frame.args[3], &descriptor()).is_err() {
        return 0;
    }
    1
}

pub extern "C" fn nt_gdi_set_pixel_format(frame: &SyscallFrame) -> u64 {
    let Some(mut dc) = get_dc(frame.args[0]) else {
        return 0;
    };
    if frame.args[1] != 1 || dc.dc_type == DC_TYPE_MEMORY || dc.pixel_format != 0 {
        return 0;
    }
    dc.pixel_format = 1;
    1
}

pub extern "C" fn nt_gdi_swap_buffers(frame: &SyscallFrame) -> u64 {
    let Some(dc) = get_dc(frame.args[0]) else {
        return 0;
    };
    // GDI writes the visible buffer immediately. No exchange is needed for
    // the single-buffer format; unconfigured and memory DCs are rejected.
    u64::from(dc.pixel_format == 1 && dc.dc_type != DC_TYPE_MEMORY)
}

pub extern "C" fn nt_gdi_update_colors(frame: &SyscallFrame) -> u64 {
    let Some(dc) = get_dc(frame.args[0]) else {
        return 0;
    };
    // A true-color DIB stores colors directly and has no realized palette to
    // update. The operation succeeds on this backend without changing pixels.
    u64::from(dc.dc_type != DC_TYPE_MEMORY)
}
