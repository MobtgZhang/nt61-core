//! Public server metafiles: immutable bytes plus METAFILEPICT/EMF metadata.
//!
//! Win7 SP1 x64 NtGdiCreateServerMetaFile (RVA 0x216c14) and
//! NtGdiGetServerMetaFileBits (0x216d3c) operate on type-21 public HMG
//! objects. This backend uses the existing synthetic Metafile handle kind.
use super::objects::{self, GdiObjectType};
use crate::{
    arch::common::syscall_frame::SyscallFrame,
    mm::{pool, user_copy},
};

const MFP: u32 = 0x5f50_464d;
const MFEX: u32 = 0x5845_464d;
const ORIGINAL_OVERHEAD: usize = 0x30;
const MAX_ALLOCATION: usize = 0x271_0000;

#[repr(C)]
#[derive(Clone, Copy)]
struct ServerMetafile {
    kind: u32,
    map_mode: u32,
    x_extent: u32,
    y_extent: u32,
    count: u32,
}

/// The input is opaque metafile bytes: the original entry does not parse its
/// record stream. A nonnull pointer is required even for an empty metafile.
pub extern "C" fn nt_gdi_create_server_meta_file(frame: &SyscallFrame) -> u64 {
    let kind = frame.args[0] as u32;
    let count = frame.args[1] as u32 as usize;
    let input = frame.args[2];
    if !matches!(kind, MFP | MFEX) || input == 0 || count > MAX_ALLOCATION - ORIGINAL_OVERHEAD {
        return 0;
    }
    let allocation = pool::allocate(pool::PoolType::NonPaged, count + ORIGINAL_OVERHEAD);
    if allocation.is_null() {
        return 0;
    }
    let header = ServerMetafile {
        kind,
        map_mode: frame.args[3] as u32,
        x_extent: frame.args[4] as u32,
        y_extent: frame.args[5] as u32,
        count: count as u32,
    };
    // SAFETY: this fresh, unpublished pool allocation contains the header and
    // count payload bytes; ORIGINAL_OVERHEAD exceeds the header size.
    unsafe {
        allocation.cast::<ServerMetafile>().write(header);
    }
    if count != 0 {
        // Win7's nonempty capture requires DWORD alignment, before reading.
        // SAFETY: allocation owns header plus count bytes; user_copy validates
        // the source mappings before reading them.
        let captured = input & 3 == 0
            && unsafe {
                let payload = core::slice::from_raw_parts_mut(
                    allocation.add(core::mem::size_of::<ServerMetafile>()),
                    count,
                );
                user_copy::copy_from_user(payload, input).is_ok()
            };
        if !captured {
            // SAFETY: unpublished allocation is still exclusively owned here.
            unsafe {
                let _ = pool::free(allocation);
            }
            return 0;
        }
    }
    // Original HmgAlloc flags9 selects public owner0, allowing cross-process
    // consumers. Generic DeleteObject frees this contiguous pool allocation.
    let handle = objects::with_handle_table(|table| {
        table.allocate(GdiObjectType::Metafile, allocation as u64, 0)
    });
    if handle == 0 {
        // SAFETY: failed publication leaves ownership with this function.
        unsafe {
            let _ = pool::free(allocation);
        }
    }
    handle
}

/// Zero length queries only the byte count. A nonzero length must equal the
/// stored count exactly; partial copies and oversized buffers are rejected.
pub extern "C" fn nt_gdi_get_server_meta_file_bits(frame: &SyscallFrame) -> u64 {
    objects::with_handle_table(|table| {
        let Some(entry) = table.get(frame.args[0]) else {
            return 0;
        };
        if entry.get_type() != Some(GdiObjectType::Metafile) || entry.obj_ptr == 0 {
            return 0;
        }
        // SAFETY: this kind owns a ServerMetafile followed by immutable bytes.
        // The table guard also excludes generic deletion for the entire copy.
        let header = unsafe { &*(entry.obj_ptr as *const ServerMetafile) };
        if !matches!(header.kind, MFP | MFEX) {
            return 0;
        }
        let count = frame.args[1] as u32;
        if count == 0 {
            return u64::from(header.count);
        }
        if count != header.count {
            return 0;
        }
        // These individual stores intentionally precede the data probe. Win7
        // leaves earlier metadata stores visible when a later output faults.
        for (address, value) in [
            (frame.args[5], header.x_extent),
            (frame.args[6], header.y_extent),
            (frame.args[3], header.kind),
            (frame.args[4], header.map_mode),
        ] {
            if user_copy::copy_to_user(address, &value.to_le_bytes()).is_err() {
                return 0;
            }
        }
        if frame.args[2] & 3 != 0 {
            return 0;
        }
        // SAFETY: successful type validation and the table guard keep the
        // allocation, header and its count payload bytes alive until copy ends.
        let payload = unsafe {
            core::slice::from_raw_parts(
                (entry.obj_ptr as *const u8).add(core::mem::size_of::<ServerMetafile>()),
                count as usize,
            )
        };
        if user_copy::copy_to_user(frame.args[2], payload).is_err() {
            return 0;
        }
        u64::from(count)
    })
}
