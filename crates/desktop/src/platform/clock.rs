pub trait Clock {
    fn monotonic_millis(&self) -> u64;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FakeClock {
    pub now: u64,
}

impl FakeClock {
    pub fn new(now: u64) -> Self {
        Self { now }
    }

    pub fn advance(&mut self, millis: u64) {
        self.now = self.now.saturating_add(millis);
    }
}

impl Clock for FakeClock {
    fn monotonic_millis(&self) -> u64 {
        self.now
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_clock_is_monotonic() {
        let mut clock = FakeClock::new(10);
        clock.advance(5);
        assert_eq!(clock.monotonic_millis(), 15);
        clock.advance(0);
        assert_eq!(clock.monotonic_millis(), 15);
    }
}
