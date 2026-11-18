//! Select a bounded PFN bank without discarding usable firmware islands.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Region {
    pub base: u64,
    pub size: u64,
    pub usable: u64,
}

/// `descriptor` yields (base, byte length, UEFI type). The caller must still
/// reserve every non-usable and unlisted hole before allocating from the bank.
pub fn select(count: usize, descriptor: impl Fn(usize) -> (u64, u64, u32), capacity: u64) -> Option<Region> {
    let capacity = capacity & !4095;
    if capacity == 0 { return None; }
    let usable_range = |index| {
        let (base, size, kind) = descriptor(index);
        if !matches!(kind, 3 | 4 | 7 | 9) || base & 4095 != 0 {
            return None;
        }
        let end = base.checked_add(size)? & !4095;
        let base = base.max(0x100000);
        (end > base).then_some((base, end))
    };
    let mut best = None;
    for index in 0..count {
        let Some((start, end)) = usable_range(index) else { continue; };
        // The maximum overlap changes slope at descriptor boundaries or a
        // boundary shifted by the capacity. No page-by-page search is needed.
        for base in [start, end.saturating_sub(capacity).max(0x100000)] {
            let limit = base.saturating_add(capacity);
            let mut usable = 0u64;
            let mut last = base;
            for index in 0..count {
                let Some((start, end)) = usable_range(index) else { continue; };
                let start = start.max(base);
                let end = end.min(limit);
                if end > start {
                    usable += end - start;
                    last = last.max(end);
                }
            }
            let region = Region { base, size: last - base, usable };
            if usable != 0 && best.map_or(true, |r: Region| usable > r.usable ||
                (usable == r.usable && region.size < r.size)) {
                best = Some(region);
            }
        }
    }
    best
}
