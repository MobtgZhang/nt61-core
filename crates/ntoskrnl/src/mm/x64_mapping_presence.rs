//! Read-only x64 presence walk. Large leaves terminate the walk: their
//! physical payload is never interpreted as another page-table page.

pub fn page_is_mapped(root: u64, va: u64, mut read: impl FnMut(u64, usize) -> u64) -> bool {
    const PRESENT: u64 = 1;
    const LARGE: u64 = 1 << 7;
    const ADDRESS: u64 = 0x000f_ffff_ffff_f000;
    let mut table = root;
    for shift in [39, 30, 21, 12] {
        let entry = read(table, ((va >> shift) & 0x1ff) as usize);
        if entry & PRESENT == 0 {
            return false;
        }
        if shift == 12 {
            return true;
        }
        if entry & LARGE != 0 {
            // PML4 entries cannot be large leaves. For PDPTE/PDE leaves,
            // bit 12 is PAT; bits 13 up to the leaf size must be zero.
            return shift != 39 && entry & (((1u64 << shift) - 1) & !0x1fff) == 0;
        }
        table = entry & ADDRESS;
    }
    false
}
