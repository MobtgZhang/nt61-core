//! CCD queries over the current single-framebuffer topology. Adapter identity
//! is session-local. Persistent topology, EDID and mode switching are not exposed.
use super::input_services::screen_rect;
use crate::arch::common::syscall_frame::SyscallFrame;
use crate::mm::user_copy::{copy_to_user, probe_user_write, read_user_object, write_user_object};

const ADAPTER: u64 = 1;
fn valid_flags(flags: u32) -> bool {
    matches!(flags, 1 | 2 | 4)
}
fn put(data: &mut [u8], offset: usize, value: u32) {
    data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
fn text(data: &mut [u8], offset: usize, value: &str) {
    for (index, unit) in value.encode_utf16().chain(core::iter::once(0)).enumerate() {
        data[offset + index * 2..offset + index * 2 + 2].copy_from_slice(&unit.to_le_bytes());
    }
}
pub extern "C" fn nt_user_get_display_config_buffer_sizes(frame: &SyscallFrame) -> u64 {
    if !valid_flags(frame.args[0] as u32) {
        return 87;
    }
    if probe_user_write(frame.args[1], 4).is_err() || probe_user_write(frame.args[2], 4).is_err() {
        return 87;
    }
    // Both active and database requests need these buffer sizes, although this
    // backend cannot retrieve a persistent database snapshot.
    if write_user_object(frame.args[1], &1u32).is_err()
        || write_user_object(frame.args[2], &2u32).is_err()
    {
        return 87;
    }
    0
}
pub extern "C" fn nt_user_query_display_config(frame: &SyscallFrame) -> u64 {
    let flags = frame.args[0] as u32;
    if !valid_flags(flags) || (flags == 4) != (frame.args[5] != 0) {
        return 87;
    }
    if flags == 4 {
        return 50;
    }
    let Ok(paths) = read_user_object::<u32>(frame.args[1]) else {
        return 87;
    };
    let Ok(modes) = read_user_object::<u32>(frame.args[3]) else {
        return 87;
    };
    if probe_user_write(frame.args[1], 4).is_err() || probe_user_write(frame.args[3], 4).is_err() {
        return 87;
    }
    if paths < 1 || modes < 2 {
        let _ = write_user_object(frame.args[1], &1u32);
        let _ = write_user_object(frame.args[3], &2u32);
        return 122;
    }
    if probe_user_write(frame.args[2], 72).is_err() || probe_user_write(frame.args[4], 128).is_err()
    {
        return 87;
    }
    let screen = screen_rect();
    let width = screen.right as u32;
    let height = screen.bottom as u32;
    let mut path = [0u8; 72];
    path[..8].copy_from_slice(&ADAPTER.to_le_bytes());
    put(&mut path, 12, 0); // source mode index
    put(&mut path, 16, 1); // source in use
    path[20..28].copy_from_slice(&ADAPTER.to_le_bytes());
    put(&mut path, 32, 1); // target mode index
    put(&mut path, 36, 0xffff_fffe); // internal output
    put(&mut path, 40, 1); // identity rotation
    put(&mut path, 44, 1); // identity scaling
                           // Refresh rate is unknown without mode timing information.
    put(&mut path, 52, 1);
    put(&mut path, 60, 1); // target available
    put(&mut path, 64, 1); // target in use
    put(&mut path, 68, 1); // path active
    let mut modes = [0u8; 128];
    put(&mut modes, 0, 1); // source
    modes[8..16].copy_from_slice(&ADAPTER.to_le_bytes());
    put(&mut modes, 16, width);
    put(&mut modes, 20, height);
    put(&mut modes, 24, 4); // 32 bpp
    put(&mut modes, 64, 2); // target
    modes[72..80].copy_from_slice(&ADAPTER.to_le_bytes());
    // DISPLAYCONFIG_VIDEO_SIGNAL_INFO: active/total dimensions, unknown clocks.
    put(&mut modes, 92, 1);
    put(&mut modes, 100, 1);
    put(&mut modes, 104, width);
    put(&mut modes, 108, height);
    put(&mut modes, 112, width);
    put(&mut modes, 116, height);
    if copy_to_user(frame.args[2], &path).is_err()
        || copy_to_user(frame.args[4], &modes).is_err()
        || write_user_object(frame.args[1], &1u32).is_err()
        || write_user_object(frame.args[3], &2u32).is_err()
    {
        return 87;
    }
    0
}
pub extern "C" fn nt_user_display_config_get_device_info(frame: &SyscallFrame) -> u64 {
    let Ok(header) = read_user_object::<[u8; 20]>(frame.args[0]) else {
        return 87;
    };
    let kind = u32::from_le_bytes(header[..4].try_into().unwrap_or([0; 4]));
    let size = u32::from_le_bytes(header[4..8].try_into().unwrap_or([0; 4]));
    let adapter = u64::from_le_bytes(header[8..16].try_into().unwrap_or([0; 8]));
    if adapter != ADAPTER || header[16..20] != [0; 4] {
        return 87;
    }
    let needed = match kind {
        1 => 84,
        2 => 420,
        4 => 276,
        _ => return 50,
    };
    if size < needed || probe_user_write(frame.args[0], needed as usize).is_err() {
        return 87;
    }
    let mut data = [0u8; 420];
    data[..20].copy_from_slice(&header);
    match kind {
        1 => text(&mut data, 20, "\\\\.\\DISPLAY1"),
        2 => {
            put(&mut data, 24, 0xffff_fffe);
            text(&mut data, 36, "NT61 Framebuffer");
            text(&mut data, 164, "\\??\\DISPLAY#NT61_FRAMEBUFFER#0");
        }
        4 => text(&mut data, 20, "\\??\\Root#NT61_FRAMEBUFFER#0"),
        _ => return 50,
    }
    if copy_to_user(frame.args[0], &data[..needed as usize]).is_err() {
        return 87;
    }
    0
}
