//! Logical palette services with fallible, variable-sized entry storage.
//! The display DC is true-color; indexed DIB color tables remain pending.
extern crate alloc;
use super::{
    dc,
    objects::{self, GdiObjectType, GdiPalette, PaletteEntry},
};
use crate::{arch::common::syscall_frame::SyscallFrame, mm::user_copy};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

#[path = "palette/halftone.rs"]
pub mod halftone;

// The palette policy is device-wide, rather than a saved DC attribute.
static SYSTEM_PALETTE_USE: AtomicU32 = AtomicU32::new(1); // SYSPAL_STATIC

/// The query returns the session's system palette policy independently of HDC.
pub extern "C" fn nt_gdi_get_system_palette_use(_frame: &SyscallFrame) -> u64 {
    u64::from(SYSTEM_PALETTE_USE.load(Ordering::Relaxed))
}

/// A true-color framebuffer has no modifiable hardware palette. Check the
/// same RASTERCAPS reported by GetDeviceCaps before accepting a policy change.
pub extern "C" fn nt_gdi_set_system_palette_use(frame: &SyscallFrame) -> u64 {
    let mut query = SyscallFrame::default();
    query.args[0] = frame.args[0];
    query.args[1] = 38; // RASTERCAPS
    if dc::caps::nt_gdi_get_device_caps(&query) & 0x100 == 0 {
        return 0; // SYSPAL_ERROR
    }
    let policy = frame.args[1] as u32;
    if !(1..=3).contains(&policy) {
        return 0;
    }
    u64::from(SYSTEM_PALETTE_USE.swap(policy, Ordering::Relaxed))
}

fn with_palette<R>(handle: u64, operation: impl FnOnce(&mut GdiPalette, bool) -> R) -> Option<R> {
    objects::with_handle_table(|table| {
        let entry = table.get(handle)?;
        if entry.get_type()? != GdiObjectType::Palette || entry.obj_ptr == 0 {
            return None;
        }
        // SAFETY: the GDI table lock excludes deletion and other palette access.
        let palette = unsafe { &mut *(entry.obj_ptr as *mut GdiPalette) };
        Some(operation(
            palette,
            entry.flags & objects::HANDLE_FLAG_STOCK != 0,
        ))
    })
}
fn decode(bytes: &[u8]) -> PaletteEntry {
    PaletteEntry {
        red: bytes[0],
        green: bytes[1],
        blue: bytes[2],
        flags: bytes[3] & 7,
    }
}
fn zero_buffer(size: usize) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(size).ok()?;
    bytes.resize(size, 0);
    Some(bytes)
}

/// The display's standard 256-entry halftone palette. The first and last ten
/// entries are reserved system colors; the interior is the 8x8x4 RGB cube.
pub extern "C" fn nt_gdi_create_halftone_palette(_frame: &SyscallFrame) -> u64 {
    let handle = objects::gdi_create_palette();
    let success = with_palette(handle, |palette, _| {
        let mut system = [PaletteEntry { red: 0, green: 0, blue: 0, flags: 0 }; 20];
        system.copy_from_slice(&palette.entries);
        if palette.entries.try_reserve_exact(256 - 20).is_err() {
            return false;
        }
        palette.entries.resize(256, system[0]);
        for (index, entry) in palette.entries.iter_mut().enumerate() {
            *entry = if index < 10 {
                system[index]
            } else if index >= 246 {
                system[index - 236]
            } else {
                PaletteEntry {
                    red: ((index & 7) * 32) as u8,
                    green: (((index >> 3) & 7) * 32) as u8,
                    blue: (((index >> 6) & 3) * 64) as u8,
                    flags: 0,
                }
            };
        }
        palette.num_entries = 256;
        true
    }).unwrap_or(false);
    if !success {
        objects::gdi_delete_object(handle);
        return 0;
    }
    handle
}
pub extern "C" fn nt_gdi_create_palette_internal(frame: &SyscallFrame) -> u64 {
    let count = frame.args[1] as u32 as usize;
    if count == 0 || count > u16::MAX as usize {
        return 0;
    }
    let Some(mut captured) = zero_buffer(4 + count * 4) else {
        return 0;
    };
    if user_copy::copy_from_user(&mut captured[..4 + count * 4], frame.args[0]).is_err() {
        return 0;
    }
    // The native entry count is authoritative, rather than palNumEntries.
    let handle = objects::gdi_create_palette();
    let success = with_palette(handle, |palette, _| {
        if count > palette.entries.len() {
            if palette
                .entries
                .try_reserve_exact(count - palette.entries.len())
                .is_err()
            {
                return false;
            }
        }
        palette.entries.resize(
            count,
            PaletteEntry {
                red: 0,
                green: 0,
                blue: 0,
                flags: 0,
            },
        );
        palette.num_entries = count as u32;
        for (entry, bytes) in palette
            .entries
            .iter_mut()
            .zip(captured[4..4 + count * 4].chunks_exact(4))
        {
            *entry = decode(bytes);
        }
        true
    })
    .unwrap_or(false);
    if !success {
        objects::gdi_delete_object(handle);
        return 0;
    }
    handle
}

pub extern "C" fn nt_gdi_do_palette(frame: &SyscallFrame) -> u64 {
    let start = frame.args[1] as u16 as usize;
    let requested = frame.args[2] as u16 as usize;
    let address = frame.args[3];
    let function = frame.args[4] as u32;
    let inbound = frame.args[5] as u32 != 0;
    if function > 2 {
        // The display has no RC_PALETTE; indexed DIB tables are not supported.
        return 0;
    }
    if address != 0 && requested == 0 {
        return 0;
    }
    if (function < 2 && !inbound) || (function == 2 && inbound) {
        return 0;
    }
    let Some(mut bytes) = zero_buffer(if address == 0 { 0 } else { requested * 4 }) else {
        return 0;
    };
    let count = requested;
    if function < 2 {
        if address == 0 || user_copy::copy_from_user(&mut bytes[..count * 4], address).is_err() {
            return 0;
        }
    }
    let result = with_palette(frame.args[0], |palette, stock| {
        if function == 2 && address == 0 {
            return palette.num_entries as usize;
        }
        if start >= palette.num_entries as usize || (stock && function < 2) {
            return 0;
        }
        let count = count.min(palette.num_entries as usize - start);
        let mut changed = 0;
        for (entry, bytes) in palette.entries[start..start + count]
            .iter_mut()
            .zip(bytes.chunks_exact_mut(4))
        {
            if function == 2 {
                bytes.copy_from_slice(&[entry.red, entry.green, entry.blue, entry.flags & 7]);
            } else if function == 1 || entry.flags & 1 != 0 {
                *entry = decode(bytes);
                changed += 1;
            }
        }
        if function == 0 {
            changed
        } else {
            count
        }
    })
    .unwrap_or(0);
    if function == 2
        && address != 0
        && result != 0
        && user_copy::copy_to_user(address, &bytes[..result * 4]).is_err()
    {
        return 0;
    }
    result as u64
}

pub extern "C" fn nt_gdi_get_nearest_palette_index(frame: &SyscallFrame) -> u64 {
    let color = frame.args[1] as u32;
    let target = [color as u8, (color >> 8) as u8, (color >> 16) as u8];
    with_palette(frame.args[0], |palette, _| {
        let mut nearest = 0;
        let mut distance = u32::MAX;
        for (index, entry) in palette.entries[..palette.num_entries as usize]
            .iter()
            .enumerate()
        {
            let channels = [entry.red, entry.green, entry.blue];
            let candidate: u32 = channels
                .iter()
                .zip(target)
                .map(|(&a, b)| {
                    let difference = i32::from(a) - i32::from(b);
                    (difference * difference) as u32
                })
                .sum();
            if candidate < distance {
                nearest = index;
                distance = candidate;
            }
        }
        nearest as u64
    })
    .unwrap_or(0)
}

pub extern "C" fn nt_gdi_resize_palette(frame: &SyscallFrame) -> u64 {
    let count = frame.args[1] as u32 as usize;
    if count == 0 || count > u16::MAX as usize {
        return 0;
    }
    with_palette(frame.args[0], |palette, stock| {
        if stock {
            return 0;
        }
        if count > palette.entries.len() {
            if palette
                .entries
                .try_reserve_exact(count - palette.entries.len())
                .is_err()
            {
                return 0;
            }
        }
        palette.entries.resize(
            count,
            PaletteEntry {
                red: 0,
                green: 0,
                blue: 0,
                flags: 0,
            },
        );
        palette.num_entries = count as u32;
        1
    })
    .unwrap_or(0)
}

pub extern "C" fn nt_user_select_palette(frame: &SyscallFrame) -> u64 {
    // Foreground/background realization has no hardware effect on a true-color
    // DC, but selection and saved-state lifetime remain observable.
    dc::select_typed_object(frame.args[0], frame.args[1], Some(GdiObjectType::Palette))
}
