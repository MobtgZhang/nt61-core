use super::{ApiError, String, Vec};

const FIRST_HANDLE: u64 = 0x6300_0000;
const ITEM_LIMIT: usize = 512;
const PROCESS_MENU_LIMIT: usize = 4096;
const DISABLED: u32 = 3;
const CHECKED: u32 = 8;
const BY_POSITION: u32 = 0x400;
const POPUP: u32 = 0x10;
const SEPARATOR: u32 = 0x800;

#[derive(Clone)]
pub(super) enum Resource {
    Id(u16),
    Name(String),
}

impl Resource {
    pub(super) fn as_name(&self) -> Result<u16, &str> {
        match self {
            Self::Id(id) => Ok(*id),
            Self::Name(name) => Err(name),
        }
    }
}

#[derive(Clone, Copy)]
struct Item {
    id: u32,
    state: u32,
    kind: u32,
    child: Option<usize>,
}

struct Cursor<'a> {
    bytes: &'a [u8],
    position: usize,
    left: usize,
    extended: bool,
}

impl Cursor<'_> {
    fn take<const WIDTH: usize>(&mut self) -> Option<[u8; WIDTH]> {
        let end = self.position.checked_add(WIDTH)?;
        let bytes = self.bytes.get(self.position..end)?.try_into().ok()?;
        self.position = end;
        Some(bytes)
    }

    fn word(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.take()?))
    }

    fn dword(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take()?))
    }

    fn menu(&mut self, menus: &mut Vec<Vec<Item>>, depth: usize) -> Option<usize> {
        if depth > 16 {
            return None;
        }
        let index = menus.len();
        menus.push(Vec::new());
        loop {
            self.left = self.left.checked_sub(1)?;
            let (id, state, kind, popup, end) = if self.extended {
                self.position = self.position.next_multiple_of(4);
                let kind = self.dword()?;
                let state = self.dword()?;
                let id = self.dword()?;
                let flags = self.word()?;
                (id, state, kind, flags & 1 != 0, flags & 0x80 != 0)
            } else {
                let flags = u32::from(self.word()?);
                let popup = flags & POPUP != 0;
                let id = if popup { 0 } else { u32::from(self.word()?) };
                (
                    id,
                    flags & (DISABLED | CHECKED),
                    flags & !(DISABLED | CHECKED | POPUP | 0x80),
                    popup,
                    flags & 0x80 != 0,
                )
            };
            let mut empty = true;
            while self.word()? != 0 {
                empty = false;
            }
            let child = if popup {
                if self.extended {
                    self.position = self.position.next_multiple_of(4);
                    self.dword()?;
                }
                Some(self.menu(menus, depth + 1)?)
            } else {
                None
            };
            let kind = if !self.extended && empty && id == 0 && !popup {
                kind | SEPARATOR
            } else {
                kind
            };
            menus[index].push(Item {
                id,
                state,
                kind,
                child,
            });
            if end {
                return Some(index);
            }
        }
    }
}

fn parse(bytes: &[u8]) -> Option<Vec<Vec<Item>>> {
    let mut cursor = Cursor {
        bytes,
        position: 0,
        left: ITEM_LIMIT,
        extended: false,
    };
    cursor.extended = match cursor.word()? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let offset = usize::from(cursor.word()?);
    if cursor.extended && offset < 4 {
        return None;
    }
    cursor.position = cursor.position.checked_add(offset)?;
    let mut menus = Vec::new();
    cursor.menu(&mut menus, 0)?;
    Some(menus)
}

struct Tree {
    owner: u32,
    first: u64,
    menus: Vec<Vec<Item>>,
}

#[derive(Default)]
pub(super) struct Menus {
    trees: Vec<Tree>,
    allocated: u64,
}

impl Menus {
    pub(super) fn load(&mut self, hwnd: u32, bytes: &[u8]) -> Result<u64, ApiError> {
        let menus = parse(bytes).ok_or(13u32)?;
        if self
            .trees
            .iter()
            .map(|tree| tree.menus.len())
            .sum::<usize>()
            + menus.len()
            > PROCESS_MENU_LIMIT
        {
            return Err(8u32.into());
        }
        let first = FIRST_HANDLE + self.allocated;
        self.allocated = self
            .allocated
            .checked_add(menus.len() as u64)
            .filter(|count| *count < 0x0100_0000)
            .ok_or(8u32)?;
        self.trees.push(Tree {
            owner: hwnd,
            first,
            menus,
        });
        Ok(first)
    }

    pub(super) fn release_window(&mut self, hwnd: u32) {
        self.trees.retain(|tree| tree.owner != hwnd);
    }

    fn locate(&self, handle: u64) -> Option<(usize, usize)> {
        self.trees
            .iter()
            .enumerate()
            .find_map(|(tree_index, tree)| {
                let menu_index = usize::try_from(handle.checked_sub(tree.first)?).ok()?;
                (menu_index < tree.menus.len()).then_some((tree_index, menu_index))
            })
    }

    pub(super) fn contains(&self, handle: u64) -> bool {
        self.locate(handle).is_some()
    }

    pub(super) fn submenu(&self, handle: u64, position: u32) -> u64 {
        let Some((tree, menu)) = self.locate(handle) else {
            return 0;
        };
        let tree = &self.trees[tree];
        tree.menus[menu]
            .get(position as usize)
            .and_then(|item| item.child)
            .map_or(0, |child| tree.first + child as u64)
    }

    pub(super) fn update(
        &mut self,
        api: &str,
        handle: u64,
        item: u32,
        flags: u32,
    ) -> Result<u64, ApiError> {
        let (tree, menu) = self.locate(handle).ok_or(1401u32)?;
        let menus = &mut self.trees[tree].menus;
        let (owner, position) = if flags & BY_POSITION != 0 {
            (menu, item as usize)
        } else {
            find_command(menus, menu, item).ok_or(1456u32)?
        };
        let entry = *menus[owner].get(position).ok_or(1456u32)?;
        let mask = match api {
            "GetMenuState" => {
                let value = if let Some(child) = entry.child {
                    ((menus[child].len().min(255) as u32) << 8)
                        | (entry.state | entry.kind | POPUP) & 0xff
                } else {
                    entry.state | entry.kind
                };
                return Ok(u64::from(value));
            }
            "CheckMenuItem" => CHECKED,
            "EnableMenuItem" => DISABLED,
            _ => return Err(ApiError::Unsupported),
        };
        menus[owner][position].state = (entry.state & !mask) | (flags & mask);
        Ok(u64::from(entry.state & mask))
    }
}

fn find_command(menus: &[Vec<Item>], menu: usize, id: u32) -> Option<(usize, usize)> {
    for (position, item) in menus[menu].iter().enumerate() {
        if let Some(child) = item.child {
            if let Some(found) = find_command(menus, child, id) {
                return Some(found);
            }
        } else if item.kind & SEPARATOR == 0 && item.id == id {
            return Some((menu, position));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_changes_through_parent_are_visible_through_submenu_handle() {
        let words = [0u16, 0, 0x90, 65, 0, 0x80, 42, 66, 0];
        let bytes = words
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        let mut menus = Menus::default();
        let root = menus.load(1, &bytes).ok().expect("valid standard menu");
        let child = menus.submenu(root, 0);
        assert_ne!(child, 0);
        assert_eq!(menus.update("CheckMenuItem", root, 42, CHECKED).ok(), Some(0));
        assert_eq!(
            menus.update("GetMenuState", child, 0, BY_POSITION).ok(),
            Some(u64::from(CHECKED))
        );
        menus.release_window(1);
        assert!(!menus.contains(child));
        assert!(parse(&bytes[..bytes.len() - 2]).is_none());
    }

    #[test]
    fn extended_menu_preserves_dword_command_ids() {
        let bytes = [
            1, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 8, 0, 0, 0, 0x78, 0x56, 0x34, 0x12, 0x80, 0, 65, 0,
            0, 0,
        ];
        let mut menus = Menus::default();
        let root = menus.load(1, &bytes).ok().expect("valid extended menu");
        assert_eq!(
            menus.update("GetMenuState", root, 0x1234_5678, 0).ok(),
            Some(8)
        );
    }
}
