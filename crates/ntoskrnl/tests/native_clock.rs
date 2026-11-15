//! Run the production native clock queries and clock-source conversion with
//! bounded output buffers; no kernel or hardware instructions run on the host.
#![allow(non_camel_case_types)]
use std::cell::RefCell;
thread_local! {
    static MEMORY: RefCell<Memory> = RefCell::new(Memory::default());
}
#[derive(Default)]
struct Memory {
    slots: [i64; 2],
    probes: Vec<u64>,
    writes: Vec<u64>,
    deny_probe: Option<u64>,
    deny_write: Option<u64>,
    interrupt_time: u64,
    samples: usize,
}
#[path = "native_clock/ntdll.rs"]
mod ntdll;
mod ke {
    pub mod time {
        pub fn get_system_time() -> u64 {
            133_801_234_567_890_123
        }
        pub fn get_interrupt_time() -> u64 {
            crate::MEMORY.with(|memory| {
                let mut state = memory.borrow_mut();
                state.samples += 1;
                state.interrupt_time
            })
        }
    }
}
mod mm {
    pub mod user_copy {
        const ACCESS_VIOLATION: i32 = 0xC0000005u32 as i32;
        pub fn probe_user_write(address: u64, size: usize) -> Result<(), i32> {
            crate::MEMORY.with(|memory| {
                let mut state = memory.borrow_mut();
                state.probes.push(address);
                if size != 8
                    || ![0x1000, 0x1008].contains(&address)
                    || state.deny_probe == Some(address)
                {
                    Err(ACCESS_VIOLATION)
                } else {
                    Ok(())
                }
            })
        }
        pub fn write_user_object<T: Copy>(address: u64, value: &T) -> Result<(), i32> {
            probe_user_write(address, core::mem::size_of::<T>())?;
            crate::MEMORY.with(|memory| {
                let mut state = memory.borrow_mut();
                if state.deny_write == Some(address) {
                    return Err(ACCESS_VIOLATION);
                }
                state.writes.push(address);
                assert_eq!(core::mem::size_of::<T>(), 8);
                state.slots[((address - 0x1000) / 8) as usize] =
                    unsafe { (value as *const T).cast::<i64>().read_unaligned() };
                Ok(())
            })
        }
    }
}
use ntdll::clock::*;
const COUNTER: *mut i64 = 0x1000 as *mut i64;
const FREQUENCY: *mut i64 = 0x1008 as *mut i64;
fn fixture() {
    MEMORY.with(|memory| {
        *memory.borrow_mut() = Memory {
            slots: [-1, -2],
            interrupt_time: 123_450_000,
            ..Memory::default()
        }
    });
}
#[test]
fn system_time_uses_kernel_nt_epoch_units_and_rejects_null() {
    fixture();
    assert_eq!(unsafe { nt_query_system_time(COUNTER) }, 0);
    MEMORY.with(|memory| assert_eq!(memory.borrow().slots, [133_801_234_567_890_123, -2]));
    assert_eq!(
        unsafe { nt_query_system_time(core::ptr::null_mut()) },
        0xC0000005u32 as i32
    );
}
#[test]
fn counter_frequency_matches_millisecond_counts_and_is_monotonic() {
    fixture();
    assert_eq!(
        unsafe { nt_query_performance_counter(COUNTER, FREQUENCY) },
        0
    );
    MEMORY.with(|memory| {
        let mut state = memory.borrow_mut();
        assert_eq!(state.slots, [12345, 1000]);
        assert_eq!(state.writes, [0x1008, 0x1000]);
        assert_eq!(&state.probes[..2], &[0x1000, 0x1008]);
        state.interrupt_time += 10_000_000;
    });
    assert_eq!(
        unsafe { nt_query_performance_counter(COUNTER, FREQUENCY) },
        0
    );
    MEMORY.with(|memory| assert_eq!(memory.borrow().slots, [13345, 1000]));
}
#[test]
fn frequency_is_optional_and_counter_is_required() {
    fixture();
    assert_eq!(
        unsafe { nt_query_performance_counter(COUNTER, core::ptr::null_mut()) },
        0
    );
    MEMORY.with(|memory| assert_eq!(memory.borrow().slots, [12345, -2]));
    assert_eq!(
        unsafe { nt_query_performance_counter(core::ptr::null_mut(), FREQUENCY) },
        0xC0000005u32 as i32
    );
}
#[test]
fn invalid_optional_frequency_preserves_counter_without_sampling() {
    fixture();
    MEMORY.with(|memory| memory.borrow_mut().deny_probe = Some(0x1008));
    assert_eq!(
        unsafe { nt_query_performance_counter(COUNTER, FREQUENCY) },
        0xC0000005u32 as i32
    );
    MEMORY.with(|memory| {
        let state = memory.borrow();
        assert_eq!(state.slots, [-1, -2]);
        assert_eq!(state.samples, 0);
        assert!(state.writes.is_empty());
    });
}
#[test]
fn frequency_copy_failure_preserves_counter() {
    fixture();
    MEMORY.with(|memory| memory.borrow_mut().deny_write = Some(0x1008));
    assert_eq!(
        unsafe { nt_query_performance_counter(COUNTER, FREQUENCY) },
        0xC0000005u32 as i32
    );
    MEMORY.with(|memory| assert_eq!(memory.borrow().slots, [-1, -2]));
}
#[test]
fn counter_copy_failure_is_reported() {
    fixture();
    MEMORY.with(|memory| memory.borrow_mut().deny_write = Some(0x1000));
    assert_eq!(
        unsafe { nt_query_performance_counter(COUNTER, FREQUENCY) },
        0xC0000005u32 as i32
    );
    MEMORY.with(|memory| assert_eq!(memory.borrow().slots, [-1, 1000]));
}
