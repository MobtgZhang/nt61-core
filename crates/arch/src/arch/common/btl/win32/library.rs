//! Lifetime of callback-free dynamically mapped guest DLLs.
use super::{ApiError, GuestImage};
use alloc::vec::Vec;

pub(super) fn free(image: &mut GuestImage, base: u64) -> Result<u64, ApiError> {
    if !image.win32.modules.iter().any(|module| module.1 == base) {
        // Windows 7 reports ERROR_MOD_NOT_FOUND for a stale/null module
        // handle passed to FreeLibrary, rather than ERROR_INVALID_HANDLE.
        return Err(126u32.into());
    }
    if image.win32.pinned_modules.contains(&base) {
        return Ok(1);
    }
    let Some(reference) = image.win32.library_loads.iter().position(|m| m.0 == base && m.1 != 0) else {
        // Borrowed handles and implicit import references do not identify an
        // owned dynamic reference. Their release is outside this limited path.
        return Err(ApiError::Unsupported);
    };
    let count = image.module_metadata.len();
    let mut live = Vec::new();
    live.try_reserve_exact(count).map_err(|_| 8u32)?;
    live.resize(count, false);
    for (index, module) in image.module_metadata.iter().enumerate() {
        live[index] = module.base == image.image_base
            || image.win32.process_dependencies.contains(&module.base)
            || image.win32.pinned_modules.contains(&module.base)
            || image.win32.library_loads.iter().enumerate().any(|(i, &(address, refs))|
                address == module.base && refs > u32::from(i == reference));
    }
    // Reachability, rather than recursive decrements, handles shared imports
    // and orphan cycles. Only explicit references, process imports and pins
    // are roots; dependencies cannot retain an otherwise unreachable cycle.
    loop {
        let mut changed = false;
        for index in 0..count {
            if !live[index] { continue; }
            for &(owner, dependency) in &image.win32.module_dependencies {
                if owner != image.module_metadata[index].base { continue; }
                if let Some(target) = image.module_metadata.iter().position(|m| m.base == dependency) {
                    if !live[target] { live[target] = true; changed = true; }
                }
            }
        }
        if !changed { break; }
    }
    let mut removed = Vec::new();
    removed.try_reserve_exact(count).map_err(|_| 8u32)?;
    for (index, module) in image.module_metadata.iter().enumerate() {
        if !live[index] {
            // Dynamic loading already rejects these. Keep the invariant
            // explicit so future callback support cannot silently skip detach.
            if module.entry != 0 || module.directories[9] != (0, 0) {
                return Err(ApiError::Unsupported);
            }
            removed.push(module.base);
        }
    }
    // No fallible work follows the reference decrement.
    image.win32.library_loads[reference].1 -= 1;
    image.win32.library_loads.retain(|m| m.1 != 0 && !removed.contains(&m.0));
    image.win32.modules.retain(|m| !removed.contains(&m.1));
    image.module_metadata.retain(|m| !removed.contains(&m.base));
    image.win32.module_dependencies.retain(|&(owner, target)|
        !removed.contains(&owner) && !removed.contains(&target));
    image.memory.regions.retain(|region| !removed.contains(&region.base));
    if !removed.is_empty() {
        image.thread.invalidate_code();
        image.threads.invalidate_code();
    }
    Ok(1)
}
