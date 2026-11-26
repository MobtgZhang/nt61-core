//! Queries over loader/allocator-owned mappings, never guest PE header guesses.
//! SP1 native x64 and WOW64 observations: scripts/fixtures/sp1-memory-query.cs.
use super::*;

const ACCESS_VIOLATION: u32 = 0xc0000005;
const LENGTH_MISMATCH: u32 = 0xc0000004;
const INVALID_CLASS: u32 = 0xc0000003;

struct BasicInformation {
    base: u64,
    allocation: u64,
    allocation_protection: u32,
    size: u64,
    state: u32,
    protection: u32,
    kind: u32,
}

fn describe(image: &GuestImage, address: u64) -> Result<BasicInformation, u32> {
    if address >= image.user_address_end { return Err(0xc000000d); }
    let base = address & !0xfff;
    if let Some(region) = image.memory.regions.iter()
        .find(|r| base >= r.base && base - r.base < r.bytes.len() as u64) {
        let first = ((base - region.base) / PAGE as u64) as usize;
        let allocation = image.win32.virtual_allocations.iter().find(|v| v.base == region.base);
        let state = |page: usize| {
            let committed = allocation.map_or_else(
                || !region.image || region.protections[page] != 0,
                |v| v.committed[page]);
            (if committed { 0x1000 } else { 0x2000 },
             if committed { region.protections[page] } else { 0 })
        };
        let (page_state, protection) = state(first);
        let mut end = first + 1;
        while end < region.protections.len() && state(end) == (page_state, protection) { end += 1; }
        let kind = if region.image { 0x1000000 }
            else if image.win32.mappings.contains_view(region.base) { 0x40000 }
            else { 0x20000 };
        Ok(BasicInformation {
            base, allocation: region.base, allocation_protection: region.allocation_protection,
            size: (region.bytes.len().min(end * PAGE) - first * PAGE) as u64,
            state: page_state, protection, kind,
        })
    } else {
        // The null allocation granule is a distinct free range even when the
        // first actual mapping is much higher. No physical storage is invented.
        let end = if base < 0x10000 { 0x10000 } else {
            image.memory.regions.iter().filter(|r| r.base > base)
                .map(|r| r.base).min().unwrap_or(image.user_address_end)
                .min(image.user_address_end)
        };
        Ok(BasicInformation { base, allocation: 0, allocation_protection: 0,
            size: end - base, state: 0x10000, protection: 1, kind: 0 })
    }
}

fn publish(image: &mut GuestImage, output: u64, info: &BasicInformation, x86: bool) -> bool {
    let width = if x86 { 4 } else { 8 };
    let tail = if x86 { 12 } else { 24 };
    // Padding has no specified value. Never reproduce original kernel stack
    // residue or expose host bytes; preserve the caller's padding instead.
    for (offset, value, bytes) in [
        (0, info.base, width), (width, info.allocation, width),
        (width * 2, u64::from(info.allocation_protection), 4),
        (tail, info.size, width), (tail + width, u64::from(info.state), 4),
        (tail + width + 4, u64::from(info.protection), 4),
        (tail + width + 8, u64::from(info.kind), 4),
    ] {
        if !image.memory.write_virt(output + offset as u64, &value.to_le_bytes()[..bytes]) { return false; }
    }
    true
}

fn query(image: &mut GuestImage, process: u64, address: u64, output: u64,
         length: u64, returned: u64) -> u32 {
    let x86 = image.thread.context.mode == DecodeMode::X86;
    let width = if x86 { 4 } else { 8 };
    let size = if x86 { 28 } else { 48 };
    // The original WOW64 conversion adjusts its local native result length
    // even on failure. Native x64 leaves ReturnLength untouched on failure.
    let mut result_length = if x86 { Some((length as u32).wrapping_sub(20) as u64) } else { None };
    let status = if length < size {
        LENGTH_MISMATCH
    } else if !x86 && output & 7 != 0 {
        0x80000002 // STATUS_DATATYPE_MISALIGNMENT
    } else if !image.memory.probe_write(output, size as usize)
        || (returned != 0 && !image.memory.probe_write(returned, width)) {
        ACCESS_VIOLATION
    } else if process == if x86 { 0xffff_fffe } else { u64::MAX - 1 } {
        0xc0000024 // current-thread pseudo-handle is not a process
    } else if process != if x86 { u32::MAX as u64 } else { u64::MAX } {
        0xc0000008
    } else {
        match describe(image, address) {
            Err(status) => status,
            Ok(info) => {
                if publish(image, output, &info, x86) {
                    result_length = Some(size);
                    0
                } else { ACCESS_VIOLATION }
            }
        }
    };
    if returned != 0 {
        if let Some(n) = result_length {
            if !image.memory.write_virt(returned, &n.to_le_bytes()[..width]) { return ACCESS_VIOLATION; }
        }
    }
    status
}

pub(super) fn invoke(image: &mut GuestImage, name: &str, a: &[u64]) -> Result<u64, ApiError> {
    if name == "NtQueryVirtualMemory" {
        if a[2] as u32 != 0 {
            // The sampled invalid class values precede all pointer/length
            // checks. Other valid information classes need their own backend.
            if a[2] as u32 >= 5 { return Ok(u64::from(INVALID_CLASS)); }
            return Err(ApiError::Unsupported);
        }
        return Ok(u64::from(query(image, a[0], a[1], a[3], a[4], a[5])));
    }
    let pseudo = if image.thread.context.mode == DecodeMode::X86 { u32::MAX as u64 } else { u64::MAX };
    let (process, address, output, length) = if name == "VirtualQueryEx" {
        (a[0], a[1], a[2], a[3])
    } else { (pseudo, a[0], a[1], a[2]) };
    let status = query(image, process, address, output, length, 0);
    if status != 0 {
        return Err(ApiError::Windows(match status {
            LENGTH_MISMATCH => 24,
            ACCESS_VIOLATION | 0x80000002 => NOACCESS,
            0xc0000008 | 0xc0000024 => 6,
            _ => INVALID_PARAMETER,
        }));
    }
    Ok(if image.thread.context.mode == DecodeMode::X86 { 28 } else { 48 })
}
