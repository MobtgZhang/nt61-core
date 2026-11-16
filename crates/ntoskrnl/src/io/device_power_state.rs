//! Cached state of a private device object; values crossing the native ABI
//! use DEVICE_POWER_STATE numbering, independently of the internal enum.
use crate::ke::sync::Spinlock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum DevicePowerState {
    D0 = 0,
    D1 = 1,
    D2 = 2,
    D3 = 3,
}

impl DevicePowerState {
    pub const fn to_native(self) -> u32 {
        match self {
            Self::D0 => 1,
            Self::D1 => 2,
            Self::D2 => 3,
            Self::D3 => 4,
        }
    }
}

pub(super) struct CachedDevicePowerState(Spinlock<DevicePowerState>);

impl CachedDevicePowerState {
    pub const fn new(state: DevicePowerState) -> Self {
        Self(Spinlock::new(state))
    }

    pub fn get(&self) -> DevicePowerState {
        *self.0.lock()
    }

    pub fn set(&self, state: DevicePowerState) {
        *self.0.lock() = state;
    }
}
