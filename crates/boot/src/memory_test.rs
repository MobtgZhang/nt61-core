//! Destructive, full-address-space memory sweeps. Only caller-owned RAM is used.
//! The test families follow the Windows 7 diagnostic menu; this is an independent
//! implementation, not Microsoft's proprietary memtest binary.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TestMix {
    Basic,
    Standard,
    Extended,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cache {
    Default,
    On,
    Off,
}
impl Cache {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Default => "Default",
            Self::On => "On",
            Self::Off => "Off",
        }
    }
    pub fn next(self) -> Self {
        match self {
            Self::Default => Self::On,
            Self::On => Self::Off,
            Self::Off => Self::Default,
        }
    }
    pub fn previous(self) -> Self {
        self.next().next()
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TestKind {
    Mats,
    Invc,
    Schckr,
    Lrand,
    Stride6,
    Chckr3,
    Wmats,
    Winvc,
    MatsUncached,
    Stride38,
    Wschckr,
    Wstride6,
    Chckr4,
    Wchckr3,
    Erand,
    Stride6Uncached,
    Chckr8,
}
const BASIC: &[TestKind] = &[TestKind::Mats, TestKind::Invc, TestKind::Schckr];
const STANDARD: &[TestKind] = &[
    TestKind::Mats,
    TestKind::Invc,
    TestKind::Schckr,
    TestKind::Lrand,
    TestKind::Stride6,
    TestKind::Chckr3,
    TestKind::Wmats,
    TestKind::Winvc,
];
const EXTENDED: &[TestKind] = &[
    TestKind::Mats,
    TestKind::Invc,
    TestKind::Schckr,
    TestKind::Lrand,
    TestKind::Stride6,
    TestKind::Chckr3,
    TestKind::Wmats,
    TestKind::Winvc,
    TestKind::MatsUncached,
    TestKind::Stride38,
    TestKind::Wschckr,
    TestKind::Wstride6,
    TestKind::Chckr4,
    TestKind::Wchckr3,
    TestKind::Erand,
    TestKind::Stride6Uncached,
    TestKind::Chckr8,
];
impl TestMix {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Basic => "Basic",
            Self::Standard => "Standard",
            Self::Extended => "Extended",
        }
    }
    pub fn next(self) -> Self {
        match self {
            Self::Basic => Self::Standard,
            Self::Standard => Self::Extended,
            Self::Extended => Self::Basic,
        }
    }
    pub fn previous(self) -> Self {
        self.next().next()
    }
    pub const fn tests(self) -> &'static [TestKind] {
        match self {
            Self::Basic => BASIC,
            Self::Standard => STANDARD,
            Self::Extended => EXTENDED,
        }
    }
}
impl TestKind {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Mats | Self::MatsUncached => "MATS+",
            Self::Invc => "INVC",
            Self::Schckr => "SCHCKR",
            Self::Lrand => "LRAND",
            Self::Chckr3 => "CHCKR3",
            Self::Stride6 | Self::Stride6Uncached => "Stride6",
            Self::Wmats => "WMATS+",
            Self::Winvc => "WINVC",
            Self::Stride38 => "Stride38",
            Self::Wschckr => "WSCHCKR",
            Self::Wstride6 => "WStride6",
            Self::Chckr4 => "CHCKR4",
            Self::Wchckr3 => "WCHCKR3",
            Self::Erand => "ERAND",
            Self::Chckr8 => "CHCKR8",
        }
    }
    pub const fn cache_enabled(self, cache: Cache) -> bool {
        match cache {
            Cache::On => true,
            Cache::Off => false,
            Cache::Default => !matches!(
                self,
                Self::MatsUncached
                    | Self::Stride38
                    | Self::Wschckr
                    | Self::Wstride6
                    | Self::Chckr4
                    | Self::Wchckr3
                    | Self::Erand
                    | Self::Stride6Uncached
                    | Self::Chckr8
            ),
        }
    }
    fn rounds(self) -> usize {
        match self {
            Self::Wmats | Self::Winvc => 64,
            Self::Wschckr | Self::Wstride6 | Self::Wchckr3 => 8,
            Self::Invc => 4,
            Self::Erand => 8,
            _ => 1,
        }
    }
    fn stride(self) -> usize {
        match self {
            Self::Stride6 | Self::Stride6Uncached | Self::Wstride6 => 6,
            Self::Stride38 => 38,
            _ => 1,
        }
    }
    fn value(self, address: u64, round: usize) -> u64 {
        let word = address / 8;
        match self {
            Self::Mats | Self::MatsUncached => 0,
            Self::Invc => [0, 0x5555_5555_5555_5555, address, !address][round],
            Self::Wmats | Self::Winvc => 1u64 << round,
            Self::Lrand | Self::Erand => {
                let mut x =
                    address.wrapping_add((round as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15));
                x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
                x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
                x ^ (x >> 31)
            }
            Self::Stride6 | Self::Stride6Uncached | Self::Stride38 => address,
            _ => {
                let group = match self {
                    Self::Chckr4 => 4,
                    Self::Chckr3 | Self::Wchckr3 => 3,
                    Self::Chckr8 => 8,
                    _ => 1,
                };
                let bits = if matches!(self, Self::Wschckr | Self::Wstride6 | Self::Wchckr3) {
                    0x0101_0101_0101_0101u64 << round
                } else {
                    0x5555_5555_5555_5555
                };
                if (word / group) & 1 == 0 {
                    bits
                } else {
                    !bits
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Failure {
    pub address: u64,
    pub expected: u64,
    pub actual: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TestError {
    Cancelled,
    Fault(Failure),
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub errors: u64,
    pub first: Option<Failure>,
}
#[derive(Clone, Copy, Debug)]
pub struct Progress {
    pub done: u64,
    pub total: u64,
    pub report: Report,
}

pub trait Memory {
    fn words(&self) -> usize;
    fn address(&self, index: usize) -> u64;
    fn write(&mut self, index: usize, value: u64);
    fn read(&mut self, index: usize) -> u64;
    /// Restore firmware cache state before returning to UI/firmware code.
    fn set_cache(&mut self, _enabled: bool) {}
    fn restore_cache(&mut self) {}
}

/// Each phase traverses ALL reserved regions before the next starts. This is
/// essential: per-block write/read pairs cannot detect aliases between blocks.
/// Failures accumulate instead of stopping the diagnostic at the first bad word.
/// `proceed` is called at most 4096 accesses apart, with firmware cache state restored.
pub fn run<M: Memory>(
    memory: &mut M,
    kind: TestKind,
    cache: Cache,
    mut proceed: impl FnMut(Progress) -> bool,
) -> Result<Report, TestError> {
    let words = memory.words();
    let total = words as u64 * kind.rounds() as u64 * 4;
    let mut done = 0;
    let mut report = Report::default();
    for round in 0..kind.rounds() {
        // write P; ascending read P/write !P; descending read !P/write P; read P.
        for phase in 0..4 {
            for lane in 0..kind.stride() {
                let mut position = lane;
                while position < words {
                    if done % 4096 == 0 {
                        memory.restore_cache();
                        if !proceed(Progress {
                            done,
                            total,
                            report,
                        }) {
                            return Err(TestError::Cancelled);
                        }
                        memory.set_cache(kind.cache_enabled(cache));
                    }
                    let index = if phase == 2 {
                        words - 1 - position
                    } else {
                        position
                    };
                    let value = kind.value(memory.address(index), round);
                    if phase != 0 {
                        let expected = if phase == 2 { !value } else { value };
                        let actual = memory.read(index);
                        if actual != expected {
                            report.errors = report.errors.saturating_add(1);
                            report.first.get_or_insert(Failure {
                                address: memory.address(index),
                                expected,
                                actual,
                            });
                        }
                    }
                    if phase != 3 {
                        memory.write(index, if phase == 1 { !value } else { value });
                    }
                    done += 1;
                    position += kind.stride();
                }
            }
        }
    }
    memory.restore_cache();
    if !proceed(Progress {
        done,
        total,
        report,
    }) {
        return Err(TestError::Cancelled);
    }
    Ok(report)
}

/// Compatibility API. Returns unique bytes verified, never accumulated writes.
#[allow(dead_code)]
pub fn test<M: Memory>(
    memory: &mut M,
    mix: TestMix,
    mut proceed: impl FnMut() -> bool,
) -> Result<u64, TestError> {
    for &kind in mix.tests() {
        let report = run(memory, kind, Cache::Default, |_| proceed())?;
        if let Some(failure) = report.first {
            return Err(TestError::Fault(failure));
        }
    }
    Ok(memory.words() as u64 * 8)
}

pub struct OwnedRegion {
    pointer: *mut u64,
    words: usize,
}
impl OwnedRegion {
    /// # Safety
    /// `pointer` must be aligned, mapped and exclusively reserved for writes
    /// throughout the test, with `words` valid u64s and no allocator metadata.
    pub unsafe fn new(pointer: *mut u64, words: usize) -> Self {
        Self { pointer, words }
    }
}
impl Memory for OwnedRegion {
    fn words(&self) -> usize {
        self.words
    }
    fn address(&self, index: usize) -> u64 {
        self.pointer as u64 + index as u64 * 8
    }
    fn write(&mut self, index: usize, value: u64) {
        assert!(index < self.words);
        // SAFETY: new's contract reserves the entire range; index is checked.
        unsafe {
            self.pointer.add(index).write_volatile(value);
        }
    }
    fn read(&mut self, index: usize) -> u64 {
        assert!(index < self.words);
        // SAFETY: new's contract reserves the entire range; index is checked.
        unsafe { self.pointer.add(index).read_volatile() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Ram {
        words: std::vec::Vec<u64>,
        alias: bool,
        stuck: bool,
        cache: bool,
        cache_changes: usize,
    }
    impl Ram {
        fn new() -> Self {
            Self {
                words: std::vec![0; 16385],
                alias: false,
                stuck: false,
                cache: true,
                cache_changes: 0,
            }
        }
    }
    impl Memory for Ram {
        fn words(&self) -> usize {
            self.words.len()
        }
        fn address(&self, i: usize) -> u64 {
            0x100000 + i as u64 * 8
        }
        fn write(&mut self, i: usize, v: u64) {
            let index = if self.alias && i >= 8192 { i - 8192 } else { i };
            self.words[index] = if self.stuck { v & !0x100 } else { v };
        }
        fn read(&mut self, i: usize) -> u64 {
            self.words[if self.alias && i >= 8192 { i - 8192 } else { i }]
        }
        fn set_cache(&mut self, enabled: bool) {
            self.cache = enabled;
            self.cache_changes += 1;
        }
        fn restore_cache(&mut self) {
            self.cache = true;
        }
    }
    #[test]
    fn all_test_families_verify_healthy_memory_and_report_monotonic_progress() {
        for &kind in TestMix::Extended.tests() {
            let mut ram = Ram::new();
            let mut previous = 0;
            let result = run(&mut ram, kind, Cache::Default, |p| {
                assert!(p.done >= previous && p.done <= p.total);
                previous = p.done;
                true
            })
            .unwrap();
            assert_eq!(result.errors, 0, "{}", kind.name());
            assert!(previous > 0);
            assert!(ram.cache);
        }
    }
    #[test]
    fn failures_accumulate_and_testing_continues() {
        let mut ram = Ram::new();
        ram.stuck = true;
        let mut reached_end = false;
        let result = run(&mut ram, TestKind::Mats, Cache::Off, |p| {
            reached_end = p.done == p.total;
            true
        })
        .unwrap();
        assert!(result.errors > 1 && result.first.is_some());
        assert!(reached_end && ram.cache);
    }
    #[test]
    fn address_aliases_between_regions_are_detected() {
        let mut ram = Ram::new();
        ram.alias = true;
        assert!(matches!(
            test(&mut ram, TestMix::Basic, || true),
            Err(TestError::Fault(_))
        ));
    }
    #[test]
    fn cancellation_restores_cache_and_leaves_unreached_memory_untouched() {
        let mut ram = Ram::new();
        ram.words.fill(0x1234);
        let mut calls = 0;
        assert_eq!(
            run(&mut ram, TestKind::Mats, Cache::Off, |_| {
                calls += 1;
                calls < 2
            }),
            Err(TestError::Cancelled)
        );
        assert!(ram.cache && ram.cache_changes > 0);
        assert!(ram.words[4096..].iter().all(|&v| v == 0x1234));
    }
    #[test]
    fn win7_mixes_and_default_cache_choices() {
        assert_eq!(TestMix::Basic.tests().len(), 3);
        assert_eq!(TestMix::Standard.tests().len(), 8);
        assert_eq!(TestMix::Extended.tests().len(), 17);
        assert_eq!(TestMix::Standard.tests().iter().map(|t| t.name()).collect::<std::vec::Vec<_>>(),
            ["MATS+", "INVC", "SCHCKR", "LRAND", "Stride6", "CHCKR3", "WMATS+", "WINVC"]);
        assert!(TestKind::Mats.cache_enabled(Cache::Default));
        assert!(!TestKind::MatsUncached.cache_enabled(Cache::Default));
        assert!(TestKind::MatsUncached.cache_enabled(Cache::On));
        assert!(!TestKind::Mats.cache_enabled(Cache::Off));
    }
}
