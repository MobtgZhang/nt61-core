//! Destructive tests for memory exclusively owned by the caller.
//! Full-region write/read sweeps also detect aliased address lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TestMix {
    Basic,
    Standard,
    Extended,
}
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
    fn pattern_count(self) -> usize {
        match self {
            Self::Basic => 2,
            Self::Standard => 6,
            Self::Extended => 70,
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

pub trait Memory {
    fn words(&self) -> usize;
    fn address(&self, index: usize) -> u64;
    fn write(&mut self, index: usize, value: u64);
    fn read(&mut self, index: usize) -> u64;
}
fn pattern(step: usize, address: u64) -> u64 {
    match step {
        0 => 0,
        1 => u64::MAX,
        2 => 0x5555_5555_5555_5555,
        3 => 0xaaaa_aaaa_aaaa_aaaa,
        4 => address,
        5 => !address,
        n => 1u64 << (n - 6),
    }
}
/// Returns unique bytes verified, never the sum of repeated pattern writes.
/// `proceed` runs at bounded intervals in both the write and read sweeps.
pub fn test<M: Memory>(
    memory: &mut M,
    mix: TestMix,
    mut proceed: impl FnMut() -> bool,
) -> Result<u64, TestError> {
    for step in 0..mix.pattern_count() {
        for index in 0..memory.words() {
            if index % 4096 == 0 && !proceed() {
                return Err(TestError::Cancelled);
            }
            memory.write(index, pattern(step, memory.address(index)));
        }
        for index in 0..memory.words() {
            if index % 4096 == 0 && !proceed() {
                return Err(TestError::Cancelled);
            }
            let address = memory.address(index);
            let expected = pattern(step, address);
            let actual = memory.read(index);
            if expected != actual {
                return Err(TestError::Fault(Failure {
                    address,
                    expected,
                    actual,
                }));
            }
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
    /// throughout the test. It must not contain code or allocator metadata.
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
        unsafe {
            self.pointer.add(index).write_volatile(value);
        }
    }
    fn read(&mut self, index: usize) -> u64 {
        unsafe { self.pointer.add(index).read_volatile() }
    }
}
