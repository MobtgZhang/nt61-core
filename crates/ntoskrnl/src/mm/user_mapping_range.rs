//! Checked byte ranges used before installing user page-table leaves.
#[cfg(any(target_arch = "aarch64", test))]
pub const AARCH64_BOOTSTRAP_MMIO: [(u64, u64); 3] = [
    (0x0800_0000, 0x1_0000),
    (0x0801_0000, 0x1_0000),
    (0x0900_0000, 0x1000),
];

pub fn page_extent(base: u64, size: u64) -> Option<(u64, u64)> {
    if size == 0 {
        return Some((base, base));
    }
    let end = base.checked_add(size)?.checked_add(0xfff)? & !0xfff;
    Some((base & !0xfff, end))
}

/// Visit each touched page only after validating the complete byte extent.
/// Empty ranges require no page-table access; malformed ranges fail closed.
pub fn all_pages(base: u64, size: u64, mut mapped: impl FnMut(u64) -> bool) -> bool {
    let Some((mut page, end)) = page_extent(base, size) else {
        return false;
    };
    while page < end {
        if !mapped(page) {
            return false;
        }
        page += 0x1000;
    }
    true
}

/// Missing reservations have size zero. Malformed nonempty reservations fail
/// closed, including arithmetic overflow. Adjacent half-open ranges are safe.
pub fn available(base: u64, size: u64, reserved: &[(u64, u64)]) -> bool {
    if size == 0 {
        return true;
    }
    let Some((start, end)) = page_extent(base, size) else {
        return false;
    };
    reserved.iter().all(|&(other_base, other_size)| {
        if other_size == 0 {
            return true;
        }
        let Some((other_start, other_end)) = page_extent(other_base, other_size) else {
            return false;
        };
        end <= other_start || other_end <= start
    })
}

/// Complete aligned physical span touched by a VA-sized mapping.
pub fn physical_extent(va: u64, size: u64, physical: u64) -> Option<(u64, u64)> {
    let (start, end) = page_extent(va, size)?;
    let physical = physical & !0xfff;
    Some((physical, physical.checked_add(end.checked_sub(start)?)?))
}

/// Reject VA aliases before an architecture masks high bits into table indices.
pub fn user_extent(base: u64, size: u64, limit: u64) -> bool {
    if size == 0 {
        return true;
    }
    match page_extent(base, size) {
        Some((start, end)) => start >= 0x1000 && start < limit && end <= limit,
        None => false,
    }
}
