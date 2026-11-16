//! Realized font identities retain the selected SFNT resource independently
//! of its DC, logical-font handle, and memory-font installation handle.
use super::{with_font, FontSource, RealizedFont};
use crate::arch::common::syscall_frame::SyscallFrame;
use crate::ke::sync::Spinlock;
use crate::mm::user_copy::{copy_to_user, probe_user_write, read_user_object};
use alloc::vec::Vec;

#[derive(Clone, Copy, PartialEq, Eq)]
struct RealizationKey {
    file: usize,
    em_height: i64,
    horizontal_numerator: i64,
    horizontal_denominator: i64,
    escapement: i32,
    orientation: i32,
    weight: i32,
    decorations: [u8; 4],
}

impl RealizationKey {
    fn from_font(font: &RealizedFont) -> Self {
        Self {
            file: font.font.data().as_ptr() as usize,
            em_height: font.em_height,
            horizontal_numerator: font.horizontal_numerator,
            horizontal_denominator: font.horizontal_denominator,
            escapement: font.requested.escapement,
            orientation: font.requested.orientation,
            weight: font.requested.weight,
            decorations: [font.requested.italic, font.requested.underline,
                font.requested.strike_out, font.requested.quality],
        }
    }
}

struct CachedFont {
    id: u32,
    key: RealizationKey,
    source: FontSource,
}

struct Cache { entries: Vec<CachedFont>, next_id: u32 }
static REALIZATIONS: Spinlock<Cache> = Spinlock::new(Cache { entries: Vec::new(), next_id: 1 });

pub(super) fn release_process(process: u64) {
    REALIZATIONS.lock().entries.retain(|entry| !entry.source.belongs_to(process));
}

fn instance(font: &RealizedFont) -> Option<u32> {
    let key = RealizationKey::from_font(font);
    let mut cache = REALIZATIONS.lock();
    if let Some(entry) = cache.entries.iter().find(|entry| entry.key == key) {
        return Some(entry.id);
    }
    // Bound kernel retention. IDs are never reused for another realization.
    if cache.entries.len() >= 4096 {
        return None;
    }
    let id = cache.next_id;
    cache.next_id = id.checked_add(1)?;
    cache.entries.try_reserve(1).ok()?;
    cache.entries.push(CachedFont { id, key, source: font.source.clone() });
    Some(id)
}

pub extern "C" fn nt_gdi_get_realization_info(frame: &SyscallFrame) -> u64 {
    let result = (|| {
        let size: u32 = read_user_object(frame.args[1]).ok()?;
        if size != 16 && size != 24 {
            return None;
        }
        probe_user_write(frame.args[1], size as usize).ok()?;
        with_font(frame.args[0], |font| {
            let id = instance(font)?;
            let mut info = [0u8; 24];
            for (slot, value) in info.chunks_exact_mut(4).zip([size, 3, id, id, 1, 0]) {
                slot.copy_from_slice(&value.to_le_bytes());
            }
            // A single regular scalable face, without synthetic bold/oblique.
            copy_to_user(frame.args[1], &info[..size as usize]).ok()
        })
    })();
    u64::from(result.is_some())
}

pub extern "C" fn nt_gdi_get_font_file_data(frame: &SyscallFrame) -> u64 {
    let result = (|| {
        let id = frame.args[0] as u32;
        if id == 0 || frame.args[1] as u32 != 0 {
            return None;
        }
        // Release the cache lock before any access to caller memory.
        let source = REALIZATIONS.lock().entries.iter().find(|entry| entry.id == id)?.source.clone();
        if !source.accessible() { return None; }
        let data = source.data();
        let offset: u64 = read_user_object(frame.args[2]).ok()?;
        let start = usize::try_from(offset).ok()?;
        let size = usize::try_from(frame.args[4]).ok()?;
        let end = start.checked_add(size)?;
        let bytes = data.get(start..end)?;
        if !bytes.is_empty() {
            copy_to_user(frame.args[3], bytes).ok()?;
        }
        Some(())
    })();
    u64::from(result.is_some())
}

/// Embedded fonts have retained bytes but no backing filesystem path or file
/// timestamp. FONT_FILE_INFO therefore describes a memory font with an empty
/// terminated path; its 64-bit C layout occupies 24 bytes including tail padding.
pub extern "C" fn nt_gdi_get_font_file_info(frame: &SyscallFrame) -> u64 {
    let result = (|| {
        let id = frame.args[0] as u32;
        if id == 0 || frame.args[1] as u32 != 0 { return None; }
        let source = REALIZATIONS.lock().entries.iter().find(|entry| entry.id == id)?.source.clone();
        if !source.accessible() { return None; }
        let data = source.data();
        const SIZE: usize = 24;
        if frame.args[4] != 0 {
            probe_user_write(frame.args[4], 8).ok()?;
        }
        if frame.args[3] < SIZE as u64 {
            if frame.args[4] != 0 {
                copy_to_user(frame.args[4], &(SIZE as u64).to_le_bytes()).ok()?;
            }
            return None;
        }
        probe_user_write(frame.args[2], SIZE).ok()?;
        let mut info = [0u8; SIZE];
        info[8..16].copy_from_slice(&(data.len() as u64).to_le_bytes());
        copy_to_user(frame.args[2], &info).ok()?;
        if frame.args[4] != 0 {
            copy_to_user(frame.args[4], &(SIZE as u64).to_le_bytes()).ok()?;
        }
        Some(())
    })();
    u64::from(result.is_some())
}
