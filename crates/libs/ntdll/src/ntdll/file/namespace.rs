//! Namespace transactions use the same lock as handle creation and close.
//! Parent identities prevent children of an unlinked directory from appearing
//! under a subsequently created directory with the same textual name.

use super::*;
use super::super::super::status::*;
use super::super::super::object::equal_sync_name;

fn copied(units: &[u16]) -> Result<Vec<u16>, NTSTATUS> {
    let mut result = Vec::new();
    result.try_reserve_exact(units.len()).map_err(|_| STATUS_NO_MEMORY)?;
    result.extend_from_slice(units);
    Ok(result)
}

impl HandleTable {
    pub(super) fn ensure_namespace(&mut self) -> Result<(), NTSTATUS> {
        if self.namespace_initialized {
            return Ok(());
        }
        for path in ["\\", "\\BaseNamedObjects", "\\Sessions", "\\Sessions\\0",
            "\\Sessions\\0\\BaseNamedObjects", "\\??", "\\Device", "\\ObjectTypes",
            "\\Windows", "\\RPC Control"]
        {
            let name: Vec<u16> = path.encode_utf16().collect();
            if self.names.iter().any(|(_, existing)| *existing == name) { continue; }
            self.names.try_reserve(1).map_err(|_| STATUS_NO_MEMORY)?;
            let parent = if name.len() == 1 { None } else {
                let split = name.iter().rposition(|&unit| unit == b'\\' as u16)
                    .ok_or(STATUS_OBJECT_PATH_INVALID)?;
                let parent = &name[..split.max(1)];
                let index = self.names.iter().find(|(_, existing)| existing == parent)
                    .map(|(index, _)| *index).ok_or(STATUS_OBJECT_PATH_NOT_FOUND)?;
                Some(self.objects[index].ok_or(STATUS_INVALID_HANDLE)?.entry.identity)
            };
            let handle = self.allocate(HandleKind::Directory, 0, DispatcherState::None, 0x000f000f, 0)?;
            let slot = decode_handle(handle);
            let index = self.slots[slot].ok_or(STATUS_INVALID_HANDLE)?.object;
            let object = self.objects[index].as_mut().ok_or(STATUS_INVALID_HANDLE)?;
            object.parent = parent;
            object.permanent = true;
            object.handles = 0;
            self.slots[slot] = None;
            self.names.push((index, name));
        }
        self.namespace_initialized = true;
        Ok(())
    }

    fn named_child(&self, name: &[u16], parent: Option<u64>, insensitive: bool) -> Option<usize> {
        self.names.iter().find_map(|(index, candidate)| {
            let object = self.objects[*index]?;
            let leaf = |units: &[u16]| units.iter().rposition(|&unit| unit == b'\\' as u16)
                .map_or(0, |index| index + 1);
            let matches = if parent.is_some() {
                equal_sync_name(&candidate[leaf(candidate)..], &name[leaf(name)..], insensitive)
            } else { equal_sync_name(candidate, name, insensitive) };
            (object.parent == parent && matches).then_some(*index)
        })
    }

    /// Resolve every parent and intermediate link, returning the canonical
    /// directory name plus an optional existing final component.
    pub(super) fn resolve_for_create(&self, input: &[u16], insensitive: bool,
        follow_final: bool) -> Result<(Vec<u16>, u64, Option<usize>), NTSTATUS>
    {
        let mut path = copied(input)?;
        for _ in 0..32 {
            let root = self.named_child(&[b'\\' as u16], None, insensitive)
                .ok_or(STATUS_OBJECT_PATH_NOT_FOUND)?;
            let root_identity = self.objects[root].ok_or(STATUS_INVALID_HANDLE)?.entry.identity;
            if path.len() == 1 { return Ok((path, root_identity, Some(root))); }
            let mut parent = root_identity;
            let mut parent_index = root;
            let mut redirected = false;
            let mut start = 1;
            while start < path.len() {
                let end = path[start..].iter().position(|&unit| unit == b'\\' as u16)
                    .map_or(path.len(), |offset| start + offset);
                let final_component = end == path.len();
                let found = self.named_child(&path[..end], Some(parent), insensitive);
                if let Some(index) = found {
                    let object = self.objects[index].ok_or(STATUS_INVALID_HANDLE)?;
                    if object.entry.kind == HandleKind::SymbolicLink && (!final_component || follow_final) {
                        let target = &self.links.iter().find(|(id, _)| *id == object.entry.identity)
                            .ok_or(STATUS_OBJECT_NAME_NOT_FOUND)?.1;
                        if target.first() != Some(&(b'\\' as u16)) {
                            return Err(STATUS_OBJECT_PATH_SYNTAX_BAD);
                        }
                        let mut next = copied(target)?;
                        next.try_reserve(path.len() - end).map_err(|_| STATUS_NO_MEMORY)?;
                        next.extend_from_slice(&path[end..]);
                        if next.len() > 32766 { return Err(STATUS_NAME_TOO_LONG); }
                        path = next;
                        redirected = true;
                        break;
                    }
                    if final_component { return Ok((path, parent, Some(index))); }
                    if object.entry.kind != HandleKind::Directory {
                        return Err(STATUS_OBJECT_TYPE_MISMATCH);
                    }
                    parent = object.entry.identity;
                    parent_index = index;
                } else if final_component {
                    let parent_name = &self.names.iter().find(|(index, _)| *index == parent_index)
                        .ok_or(STATUS_OBJECT_PATH_NOT_FOUND)?.1;
                    let mut canonical = copied(parent_name)?;
                    let separator = usize::from(canonical.len() != 1);
                    canonical.try_reserve(separator + end - start).map_err(|_| STATUS_NO_MEMORY)?;
                    if separator != 0 { canonical.push(b'\\' as u16); }
                    canonical.extend_from_slice(&path[start..end]);
                    return Ok((canonical, parent, None));
                } else {
                    return Err(STATUS_OBJECT_PATH_NOT_FOUND);
                }
                start = end + 1;
            }
            if !redirected { return Err(STATUS_OBJECT_NAME_INVALID); }
        }
        // Reparse loops and excessive indirection must terminate deterministically.
        Err(STATUS_OBJECT_PATH_NOT_FOUND)
    }
}

/// Change object permanence atomically with close and namespace lookup.
pub(crate) fn set_object_permanent(handle: HANDLE, permanent: bool) -> Result<(), NTSTATUS> {
    let mut table = HANDLE_TABLE.lock();
    let slot = table.slots.get(decode_handle(handle)).and_then(|slot| *slot)
        .ok_or(STATUS_INVALID_HANDLE)?;
    if !permanent && slot.access & 0x0001_0000 == 0 {
        return Err(STATUS_ACCESS_DENIED);
    }
    table.objects[slot.object].as_mut().ok_or(STATUS_INVALID_HANDLE)?.permanent = permanent;
    Ok(())
}

pub(crate) fn directory_root(handle: HANDLE) -> Result<(Vec<u16>, u64), NTSTATUS> {
    let table = HANDLE_TABLE.lock();
    let slot = table.slots.get(decode_handle(handle)).and_then(|slot| *slot)
        .ok_or(STATUS_INVALID_HANDLE)?;
    let object = table.objects[slot.object].ok_or(STATUS_INVALID_HANDLE)?;
    if object.entry.kind != HandleKind::Directory { return Err(STATUS_OBJECT_TYPE_MISMATCH); }
    let name = table.names.iter().find(|(index, _)| *index == slot.object)
        .ok_or(STATUS_OBJECT_PATH_NOT_FOUND)?;
    Ok((copied(&name.1)?, object.entry.identity))
}

pub(crate) fn symbolic_link_target(handle: HANDLE) -> Result<Vec<u16>, NTSTATUS> {
    let table = HANDLE_TABLE.lock();
    let slot = table.slots.get(decode_handle(handle)).and_then(|slot| *slot)
        .ok_or(STATUS_INVALID_HANDLE)?;
    let object = table.objects[slot.object].ok_or(STATUS_INVALID_HANDLE)?;
    if object.entry.kind != HandleKind::SymbolicLink { return Err(STATUS_OBJECT_TYPE_MISMATCH); }
    if slot.access & 1 == 0 { return Err(STATUS_ACCESS_DENIED); }
    let target = table.links.iter().find(|(id, _)| *id == object.entry.identity)
        .ok_or(STATUS_OBJECT_NAME_NOT_FOUND)?;
    copied(&target.1)
}

pub(crate) fn directory_entries(handle: HANDLE) -> Result<Vec<(Vec<u16>, HandleKind)>, NTSTATUS> {
    let table = HANDLE_TABLE.lock();
    let slot = table.slots.get(decode_handle(handle)).and_then(|slot| *slot)
        .ok_or(STATUS_INVALID_HANDLE)?;
    let object = table.objects[slot.object].ok_or(STATUS_INVALID_HANDLE)?;
    if object.entry.kind != HandleKind::Directory { return Err(STATUS_OBJECT_TYPE_MISMATCH); }
    if slot.access & 1 == 0 { return Err(STATUS_ACCESS_DENIED); }
    let mut entries = Vec::new();
    for (index, path) in &table.names {
        let child = table.objects[*index].ok_or(STATUS_INVALID_HANDLE)?;
        if child.parent != Some(object.entry.identity) { continue; }
        let start = path.iter().rposition(|&unit| unit == b'\\' as u16)
            .ok_or(STATUS_OBJECT_PATH_INVALID)? + 1;
        entries.try_reserve(1).map_err(|_| STATUS_NO_MEMORY)?;
        entries.push((copied(&path[start..])?, child.entry.kind));
    }
    entries.sort_unstable_by(|left, right| left.0.cmp(&right.0));
    Ok(entries)
}
