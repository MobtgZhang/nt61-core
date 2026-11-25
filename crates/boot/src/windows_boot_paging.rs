extern crate alloc;

use alloc::vec::Vec;

pub const ADDRESS_MASK: u64 = 0x000f_ffff_ffff_f000;
pub const PAGE_SIZE: usize = 4096;
pub const TABLE_ENTRIES: usize = PAGE_SIZE / size_of::<u64>();
pub const SELF_MAP_INDEX: usize = 0x1ed;
const PRESENT: u64 = 1;
const LARGE_PAGE: u64 = 1 << 7;

#[derive(Clone, Copy)]
struct Table {
    physical: u64,
    level: u8,
}

pub struct PageTableLayout(Vec<Table>);

impl PageTableLayout {
    pub fn discover(root: u64, read: impl Fn(u64) -> [u64; TABLE_ENTRIES]) -> Option<Self> {
        let mut layout = Self(Vec::new());
        layout.push(Table {
            physical: root,
            level: 4,
        })?;
        let mut index = 0;
        while let Some(table) = layout.0.get(index).copied() {
            for physical in read(table.physical)
                .iter()
                .filter_map(|entry| child(*entry, table.level))
            {
                if layout.index_of(physical).is_none() {
                    layout.push(Table {
                        physical,
                        level: table.level - 1,
                    })?;
                }
            }
            index += 1;
        }
        Some(layout)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn copy_to(
        &self,
        base: u64,
        target: &mut [[u64; TABLE_ENTRIES]],
        read: impl Fn(u64) -> [u64; TABLE_ENTRIES],
    ) -> Option<()> {
        if target.len() != self.len() {
            return None;
        }
        for (table, output) in self.0.iter().zip(target.iter_mut()) {
            for (entry, copy) in read(table.physical).iter().copied().zip(output) {
                *copy = match child(entry, table.level) {
                    Some(physical) => {
                        let index = self.index_of(physical)?;
                        (entry & !ADDRESS_MASK) | (base + (index * PAGE_SIZE) as u64)
                    }
                    None => entry,
                };
            }
        }
        target[0][SELF_MAP_INDEX] = base | 3;
        Some(())
    }

    fn index_of(&self, physical: u64) -> Option<usize> {
        self.0.iter().position(|table| table.physical == physical)
    }

    fn push(&mut self, table: Table) -> Option<()> {
        self.0.try_reserve(1).ok()?;
        self.0.push(table);
        Some(())
    }
}

fn child(entry: u64, level: u8) -> Option<u64> {
    (level > 1 && entry & PRESENT != 0 && (level == 4 || entry & LARGE_PAGE == 0))
        .then_some(entry & ADDRESS_MASK)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_non_leaf_tables_without_sharing_the_firmware_directory() {
        let mut source = [[0; TABLE_ENTRIES]; 4];
        source[0][0] = 0x2003;
        source[0][1] = 0x2001;
        source[0][511] = 0x1003;
        source[1][0] = 0x3003;
        source[1][1] = 0x4000_0083;
        source[2][3] = 0x60_0083;
        source[2][4] = 0x4003;
        source[3][0] = 0x50_0083;
        let read = |physical| source[physical as usize / PAGE_SIZE - 1];
        let layout = PageTableLayout::discover(0x1000, read).unwrap();
        assert_eq!(layout.len(), 4);
        let mut clone = [[0; TABLE_ENTRIES]; 4];
        layout.copy_to(0x8000, &mut clone, read).unwrap();
        assert_eq!(clone[0][0], 0x9003);
        assert_eq!(clone[0][1], 0x9001);
        assert_eq!(clone[0][511], 0x8003);
        assert_eq!(clone[0][SELF_MAP_INDEX], 0x8003);
        assert_eq!(&clone[1][..2], &[0xa003, 0x4000_0083]);
        assert_eq!(&clone[2][3..5], &[0x60_0083, 0xb003]);
        assert_eq!(clone[3][0], 0x50_0083);
        clone[2][3] = 0;
        assert_ne!(clone[2][3], source[2][3]);
    }

    #[test]
    fn preserves_an_unmapped_firmware_null_page() {
        let mut source = [[0; TABLE_ENTRIES]; 4];
        source[0][0] = 0x2003;
        source[1][0] = 0x3003;
        source[2][0] = 0x4003;
        source[3][1] = 0x1003;
        let read = |physical| source[physical as usize / PAGE_SIZE - 1];
        let layout = PageTableLayout::discover(0x1000, read).unwrap();
        let mut clone = [[0; TABLE_ENTRIES]; 4];
        layout.copy_to(0x8000, &mut clone, read).unwrap();
        assert_eq!(&clone[3][..2], &[0, 0x1003]);
    }
}
