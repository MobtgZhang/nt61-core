//! Lifetime of dynamically mapped guest DLLs.
use super::{error, ApiError, GuestImage};
use crate::arch::common::btl::{ir::X86Reg, pe::tls::{Routine, Startup}, runtime::StopReason};
use alloc::vec::Vec;

enum Completion {
    Attach { base: u64, edge_count: usize },
    Rejected { edge_count: usize },
    Unload { exit_code: Option<u32> },
}

pub(in super::super) struct Pending {
    startup: Startup,
    modules: Vec<u64>,
    completion: Completion,
}

impl Pending {
    pub(in super::super) fn attach(image: &GuestImage, routines: Vec<Routine>, modules: Vec<u64>, base: u64) -> Self {
        Self {
            startup: Startup::dynamic(routines, 1, &image.thread),
            modules,
            completion: Completion::Attach { base, edge_count: image.win32.module_dependencies.len() },
        }
    }
}

pub(in super::super) fn start_next(image: &mut GuestImage) -> Result<Option<u32>, StopReason> {
    let Some(pending) = image.pending_library.as_mut() else { return Ok(None); };
    if pending.startup.active() {
        return Ok(None);
    }
    pending.startup.begin(&mut image.thread, &mut image.memory)?;
    if pending.startup.active() {
        return Ok(None);
    }
    let pending = image.pending_library.take().unwrap();
    let value = match pending.completion {
        Completion::Attach { base, .. } => {
            image.initialization.register_routines(pending.startup);
            base
        }
        Completion::Rejected { edge_count } => {
            image.win32.module_dependencies.truncate(edge_count);
            unmap(image, &pending.modules);
            error(image, 1114);
            0
        }
        Completion::Unload { exit_code } => {
            unmap(image, &pending.modules);
            if exit_code.is_some() {
                return Ok(exit_code);
            }
            1
        }
    };
    image.thread.context.set_reg(X86Reg::Rax, value);
    Ok(None)
}

pub(in super::super) fn complete(image: &mut GuestImage, reason: StopReason) -> Result<bool, StopReason> {
    let pending = image.pending_library.as_mut().unwrap();
    match pending.startup.complete(&mut image.thread, reason) {
        Err(StopReason::InitializationFailed { .. }) => {
            let Completion::Attach { edge_count, .. } = pending.completion else { unreachable!() };
            pending.startup.rollback(&mut image.thread);
            pending.completion = Completion::Rejected { edge_count };
            Ok(true)
        }
        result => result,
    }
}

pub(super) fn free(image: &mut GuestImage, base: u64, exit_code: Option<u32>) -> Result<u64, ApiError> {
    if image.pending_library.is_some() {
        return Err(120u32.into());
    }
    if !image.win32.modules.iter().any(|module| module.1 == base) {
        // Windows 7 reports ERROR_MOD_NOT_FOUND for a stale/null module
        // handle passed to FreeLibrary, rather than ERROR_INVALID_HANDLE.
        return Err(126u32.into());
    }
    if image.win32.pinned_modules.contains(&base) {
        return Ok(1);
    }
    let Some(reference) = image.win32.library_loads.iter().position(|m| m.0 == base && m.1 != 0) else {
        if exit_code.is_some() && base == image.image_base {
            return Ok(1);
        }
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
            removed.push(module.base);
        }
    }
    if image.active_notification_in(&removed) {
        return Err(120u32.into());
    }
    let routines = image.initialization.unload_routines(&removed).map_err(|_| 8u32)?;
    image.win32.library_loads[reference].1 -= 1;
    if routines.is_empty() {
        unmap(image, &removed);
    } else {
        image.pending_library = Some(Pending {
            startup: Startup::dynamic(routines, 0, &image.thread),
            modules: removed,
            completion: Completion::Unload { exit_code },
        });
    }
    Ok(1)
}

fn unmap(image: &mut GuestImage, removed: &[u64]) {
    image.initialization.remove_routines(removed);
    if removed.is_empty() {
        image.win32.library_loads.retain(|m| m.1 != 0);
        return;
    }
    let width = if image.thread.context.mode == super::DecodeMode::X86 { 4 } else { 8 };
    image.initialization.remove_modules(&mut image.memory, removed,
        image.threads.tls_threads(image.teb).map(|(_, vector)| vector), width);
    image.discard_module_notifications(removed);
    image.win32.library_loads.retain(|m| m.1 != 0 && !removed.contains(&m.0));
    image.win32.pinned_modules.retain(|base| !removed.contains(base));
    image.win32.modules.retain(|m| !removed.contains(&m.1));
    image.win32.module_paths.retain(|(base, _)| !removed.contains(base));
    image.module_metadata.retain(|m| !removed.contains(&m.base));
    image.win32.resources.retain(|resource| !removed.contains(&resource.owner));
    image.win32.module_dependencies.retain(|&(owner, target)|
        !removed.contains(&owner) && !removed.contains(&target));
    image.memory.regions.retain(|region| !removed.contains(&region.base)
        && !image.resource_satellites.iter().any(|(owner, _, satellite)|
            removed.contains(owner) && satellite.base == region.base));
    image.resource_satellites.retain(|(owner, _, _)| !removed.contains(owner));
    image.thread.invalidate_code();
    image.threads.invalidate_code();
}
