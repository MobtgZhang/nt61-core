//! Captured OBJECT_TYPE_LIST and the ordered access masks of its tree.
//!
//! NT 6.1 permits levels 0..=4. Each GUID selects its first occurrence;
//! an ACE applies to that node and its descendants. Parent remaining/denied
//! masks are unions of their children; parent allowed masks are intersections.

use super::*;
use alloc::vec::Vec;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ObjectTypeList {
    pub level: u16,
    pub sbz: u16,
    pub object_type: *const [u8; 16],
}

#[derive(Clone, Copy, Debug)]
pub(super) struct ObjectType {
    pub level: u16,
    pub guid: [u8; 16],
    parent: Option<usize>,
}

pub(super) fn capture(address: u64, count: u32) -> Result<Vec<ObjectType>, NTSTATUS> {
    if count == 0 {
        return Ok(Vec::new());
    }
    if address == 0 {
        return Err(STATUS_INVALID_PARAMETER);
    }
    if address & 3 != 0 {
        return Err(STATUS_DATATYPE_MISALIGNMENT);
    }
    let length = (count as usize)
        .checked_mul(16)
        .ok_or(STATUS_INVALID_PARAMETER)?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|_| STATUS_NO_MEMORY)?;
    bytes.resize(length, 0);
    copy_from_user(&mut bytes, address)?;
    let mut output = Vec::new();
    output
        .try_reserve_exact(count as usize)
        .map_err(|_| STATUS_NO_MEMORY)?;
    let mut ancestors = [0; 5];
    for (index, record) in bytes.chunks_exact(16).enumerate() {
        let level = u16::from_le_bytes(record[..2].try_into().unwrap());
        let parent = validate_level(&output, index, level, &ancestors)?;
        let guid_address = u64::from_le_bytes(record[8..16].try_into().unwrap());
        if guid_address & 3 != 0 {
            return Err(STATUS_DATATYPE_MISALIGNMENT);
        }
        let guid = read_user_object(guid_address)?;
        output.push(ObjectType {
            level,
            guid,
            parent,
        });
        ancestors[level as usize] = index;
    }
    Ok(output)
}

fn validate_level(
    previous: &[ObjectType],
    index: usize,
    level: u16,
    ancestors: &[usize; 5],
) -> Result<Option<usize>, NTSTATUS> {
    if level > 4
        || (index == 0 && level != 0)
        || (index != 0 && (level == 0 || level > previous[index - 1].level + 1))
    {
        return Err(STATUS_INVALID_PARAMETER);
    }
    Ok((level != 0).then(|| ancestors[level as usize - 1]))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Masks {
    pub remaining: u32,
    pub allowed: u32,
    pub denied: u32,
}

pub(super) struct Tree<'a> {
    types: &'a [ObjectType],
    pub masks: Vec<Masks>,
}

#[derive(Clone, Copy)]
pub(super) enum Operation {
    Grant,
    AllowMaximum,
    DenyMaximum,
}

impl<'a> Tree<'a> {
    pub fn new(types: &'a [ObjectType], remaining: u32) -> Result<Self, NTSTATUS> {
        let mut masks = Vec::new();
        masks
            .try_reserve_exact(types.len().max(1))
            .map_err(|_| STATUS_NO_MEMORY)?;
        masks.resize(
            types.len().max(1),
            Masks {
                remaining,
                allowed: 0,
                denied: 0,
            },
        );
        Ok(Self { types, masks })
    }

    pub fn first(&self, guid: &[u8; 16]) -> Option<usize> {
        self.types.iter().position(|object| object.guid == *guid)
    }

    pub fn apply(&mut self, index: usize, mask: u32, operation: Operation) {
        let update = |state: &mut Masks| match operation {
            Operation::Grant => state.remaining &= !mask,
            Operation::AllowMaximum => state.allowed |= mask & !state.denied,
            Operation::DenyMaximum => state.denied |= mask & !state.allowed,
        };
        update(&mut self.masks[index]);
        if let Some(object) = self.types.get(index) {
            // Ancestors summarize immediate children, preserving the other
            // fields until the corresponding operation updates those fields.
            let mut parent = object.parent;
            while let Some(index) = parent {
                let mut remaining = 0;
                let mut allowed = u32::MAX;
                let mut denied = 0;
                for (child, state) in self.types.iter().zip(&self.masks) {
                    if child.parent == Some(index) {
                        remaining |= state.remaining;
                        allowed &= state.allowed;
                        denied |= state.denied;
                    }
                }
                match operation {
                    Operation::Grant => self.masks[index].remaining = remaining,
                    Operation::AllowMaximum => self.masks[index].allowed = allowed,
                    Operation::DenyMaximum => self.masks[index].denied = denied,
                }
                parent = self.types[index].parent;
            }
            let level = object.level;
            for child in index + 1..self.types.len() {
                if self.types[child].level <= level {
                    break;
                }
                update(&mut self.masks[child]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn types(levels: &[u16]) -> Result<Vec<ObjectType>, NTSTATUS> {
        let mut output = Vec::new();
        let mut ancestors = [0; 5];
        for (index, &level) in levels.iter().enumerate() {
            let parent = validate_level(&output, index, level, &ancestors)?;
            output.push(ObjectType {
                level,
                guid: [index as u8; 16],
                parent,
            });
            ancestors[level as usize] = index;
        }
        Ok(output)
    }

    #[test]
    fn object_tree_level_contract_and_first_guid() {
        for bad in [
            &[1][..],
            &[0, 0],
            &[0, 2],
            &[0, 1, 2, 3, 4, 5],
            &[0, 1, 2, 1, 3],
        ] {
            assert_eq!(types(bad).unwrap_err(), STATUS_INVALID_PARAMETER);
        }
        let mut types = types(&[0, 1, 2, 3, 4, 1, 2]).unwrap();
        types[5].guid = types[1].guid;
        let tree = Tree::new(&types, 7).unwrap();
        assert_eq!(tree.first(&types[1].guid), Some(1));
    }

    #[test]
    fn object_tree_siblings_intersect_parent_and_subtrees_inherit() {
        let types = types(&[0, 1, 2, 1, 2]).unwrap();
        let mut tree = Tree::new(&types, 7).unwrap();
        tree.apply(1, 3, Operation::AllowMaximum);
        assert_eq!(
            tree.masks.iter().map(|s| s.allowed).collect::<Vec<_>>(),
            [0, 3, 3, 0, 0]
        );
        tree.apply(4, 5, Operation::AllowMaximum);
        assert_eq!(
            tree.masks.iter().map(|s| s.allowed).collect::<Vec<_>>(),
            [1, 3, 3, 5, 5]
        );
        tree.apply(0, 7, Operation::DenyMaximum);
        assert_eq!(
            tree.masks.iter().map(|s| s.denied).collect::<Vec<_>>(),
            [6, 4, 4, 2, 2]
        );
        tree.apply(0, 7, Operation::AllowMaximum);
        assert_eq!(
            tree.masks.iter().map(|s| s.allowed).collect::<Vec<_>>(),
            [1, 3, 3, 5, 5]
        );
    }

    #[test]
    fn object_tree_explicit_grant_requires_every_child() {
        let types = types(&[0, 1, 2, 1]).unwrap();
        let mut tree = Tree::new(&types, 3).unwrap();
        tree.apply(2, 3, Operation::Grant);
        assert_eq!(
            tree.masks.iter().map(|s| s.remaining).collect::<Vec<_>>(),
            [3, 0, 0, 3]
        );
        tree.apply(3, 1, Operation::Grant);
        assert_eq!(tree.masks[0].remaining, 2);
        tree.apply(0, 2, Operation::Grant);
        assert!(tree.masks.iter().all(|s| s.remaining == 0));
    }
}
