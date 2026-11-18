//! AMD64 static PE function lookup, observed against original SP1.
//! This locates metadata; it does not execute unwind codes or dispatch exceptions.
use super::*;

fn within(rva: u64, size: u64, image_size: usize) -> Result<(), ApiError> {
    if rva.checked_add(size).is_some_and(|end| end <= image_size as u64) {
        Ok(())
    } else {
        Err(ApiError::MemoryFault)
    }
}

fn dword(image: &mut GuestImage, address: u64) -> Result<u64, ApiError> {
    word(&mut image.memory, address, 4).map_err(|_| ApiError::MemoryFault)
}

pub(super) fn lookup(image: &mut GuestImage, a: &[u64]) -> Result<u64, ApiError> {
    // The history table is a caller-visible cache, not an inert reserved arg.
    // Do not silently pretend to implement its updates or x86 unwind metadata.
    if image.thread.context.mode != DecodeMode::X64 || a[2] != 0 {
        return Err(ApiError::Unsupported);
    }
    let module = image.module_metadata.iter().copied()
        .find(|m| a[0] >= m.base && a[0] - m.base < m.size as u64);
    let base = module.map_or(0, |m| m.base);
    put(&mut image.memory, a[1], base, 8).map_err(|_| ApiError::MemoryFault)?;
    let Some(module) = module else { return Ok(0); };
    let (table, size) = module.directories[3];
    if table == 0 || size == 0 { return Ok(0); }
    within(u64::from(table), u64::from(size), module.size)?;
    if size % 12 != 0 || size as usize > MAX_BUFFER {
        return Err(ApiError::Unsupported);
    }
    // Valid AMD64 .pdata is sorted by BeginAddress. EndAddress is exclusive;
    // an address inside the image but outside every entry still publishes base.
    let relative = a[0] - base;
    let (mut low, mut high) = (0, u64::from(size / 12));
    while low < high {
        let mid = low + (high - low) / 2;
        let entry = base + u64::from(table) + mid * 12;
        let start = dword(image, entry)?;
        let end = dword(image, entry + 4)?;
        if relative < start {
            high = mid;
        } else if relative >= end {
            low = mid + 1;
        } else {
            let mut rva = u64::from(table) + mid * 12;
            // SP1 resolves indirect RUNTIME_FUNCTION records (UnwindData bit0)
            // to the actual entry, unlike UNW_FLAG_CHAININFO in UNWIND_INFO.
            // Bound the traversal so a corrupt guest cycle cannot hang a host.
            let mut visited = Vec::new();
            for _ in 0..64 {
                within(rva, 12, module.size)?;
                if visited.contains(&rva) { return Err(ApiError::Unsupported); }
                visited.push(rva);
                let unwind = dword(image, base + rva + 8)?;
                if unwind & 1 == 0 { return Ok(base + rva); }
                rva = unwind & !1;
            }
            return Err(ApiError::Unsupported);
        }
    }
    Ok(0)
}
