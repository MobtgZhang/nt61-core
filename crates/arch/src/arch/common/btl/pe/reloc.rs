pub(super) enum Relocation {
    High,
    Low,
    HighLow,
    HighAdj(i16),
    Dir64,
}

impl Relocation {
    pub(super) fn width(&self) -> usize {
        match self {
            Self::High | Self::Low | Self::HighAdj(_) => 2,
            Self::HighLow => 4,
            Self::Dir64 => 8,
        }
    }

    pub(super) fn value(&self, original: u64, delta: u64) -> u64 {
        match self {
            Self::High => u64::from((original as u16).wrapping_add((delta >> 16) as u16)),
            Self::Low => u64::from((original as u16).wrapping_add(delta as u16)),
            Self::HighLow => u64::from((original as u32).wrapping_add(delta as u32)),
            Self::HighAdj(addend) => {
                let adjusted = ((original as u32) << 16)
                    .wrapping_add(delta as u32)
                    .wrapping_add(i32::from(*addend) as u32)
                    .wrapping_add(0x8000);
                u64::from(adjusted >> 16)
            }
            Self::Dir64 => original.wrapping_add(delta),
        }
    }
}
