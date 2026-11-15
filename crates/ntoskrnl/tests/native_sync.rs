//! Execute production handle transactions, native waits and timer wrappers on
//! a host. Memory probes and the clock are supplied by the host fixture.
#![allow(dead_code, non_camel_case_types)]
extern crate alloc;
#[path = "native_sync/kernel32.rs"]
mod kernel32;
#[path = "native_sync/ntdll.rs"]
pub mod ntdll;
pub mod libs {
    pub use crate::ntdll;
}
pub mod ke {
    pub mod sync {
        pub struct Spinlock<T>(std::sync::Mutex<T>);
        // The production lock deliberately permits raw-pointer state. The host
        // fixture serializes access exactly as that lock does.
        unsafe impl<T> Sync for Spinlock<T> {}
        impl<T> Spinlock<T> {
            pub const fn new(value: T) -> Self {
                Self(std::sync::Mutex::new(value))
            }
            pub fn lock(&self) -> std::sync::MutexGuard<'_, T> {
                self.0.lock().unwrap()
            }
        }
    }
    pub mod time {
        #[derive(Clone, Copy)]
        pub struct TimeSnapshot {
            pub system_time: u64,
            pub interrupt_time: u64,
        }
        std::thread_local! { pub static CLOCK: core::cell::Cell<(u64,u64)> = const { core::cell::Cell::new((100_000,0)) }; }
        pub fn get_time_snapshot() -> TimeSnapshot {
            CLOCK.with(|clock| {
                let (system_time, interrupt_time) = clock.get();
                TimeSnapshot {
                    system_time,
                    interrupt_time,
                }
            })
        }
        pub fn get_system_time() -> u64 {
            get_time_snapshot().system_time
        }
        pub fn advance(delta: u64) {
            CLOCK.with(|clock| {
                let (wall, uptime) = clock.get();
                clock.set((wall + delta, uptime + delta));
            });
        }
    }
    pub mod scheduler {
        std::thread_local! { pub static YIELD_HOOK: core::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { core::cell::RefCell::new(None) }; }
        std::thread_local! { pub static ADVANCE_ON_YIELD: core::cell::Cell<u64> = const { core::cell::Cell::new(0) }; }
        pub fn yield_() {
            let hook = YIELD_HOOK.with(|slot| slot.borrow_mut().take());
            if let Some(hook) = hook {
                hook();
            }
            ADVANCE_ON_YIELD.with(|delta| super::time::advance(delta.get()));
            std::thread::yield_now();
        }
    }
}
pub mod ps {
    pub mod thread {
        pub struct Thread {
            pub client_id: ClientId,
        }
        pub struct ClientId {
            pub unique_process: u64,
            pub unique_thread: u64,
        }
        std::thread_local! { static CURRENT: core::cell::UnsafeCell<Thread> = const { core::cell::UnsafeCell::new(Thread { client_id: ClientId { unique_process: 100, unique_thread: 701 } }) }; }
        pub fn get_current_ethread() -> *mut Thread {
            CURRENT.with(|current| current.get())
        }
        pub fn set_tid(tid: u64) {
            CURRENT.with(|current| unsafe { (*current.get()).client_id.unique_thread = tid });
        }
    }
}
pub mod arch {
    pub mod common {
        pub mod percpu {
            pub fn get_current_thread() -> *mut crate::ps::thread::Thread {
                crate::ps::thread::get_current_ethread()
            }
        }
    }
}
pub mod mm {
    pub mod user_copy {
        pub fn probe_user_write(address: u64, _: usize) -> Result<(), i32> {
            if address < 0x1000 {
                Err(0xc0000005u32 as i32)
            } else {
                Ok(())
            }
        }
        pub fn read_user_object<T: Copy>(address: u64) -> Result<T, i32> {
            probe_user_write(address, core::mem::size_of::<T>())?;
            Ok(unsafe { (address as *const T).read_unaligned() })
        }
        pub fn write_user_object<T: Copy>(address: u64, value: &T) -> Result<(), i32> {
            probe_user_write(address, core::mem::size_of::<T>())?;
            unsafe {
                (address as *mut T).write_unaligned(*value);
            }
            Ok(())
        }
        pub fn copy_to_user(target: u64, bytes: &[u8]) -> Result<(), i32> {
            probe_user_write(target, bytes.len())?;
            unsafe {
                core::ptr::copy_nonoverlapping(bytes.as_ptr(), target as *mut u8, bytes.len());
            }
            Ok(())
        }
        pub fn copy_from_user(bytes: &mut [u8], source: u64) -> Result<(), i32> {
            probe_user_write(source, bytes.len())?;
            unsafe {
                core::ptr::copy_nonoverlapping(
                    source as *const u8,
                    bytes.as_mut_ptr(),
                    bytes.len(),
                );
            }
            Ok(())
        }
    }
}
use ntdll::{status::*, sync::*, types::*};
static TEST_STATE: std::sync::Mutex<()> = std::sync::Mutex::new(());
fn state_guard() -> std::sync::MutexGuard<'static, ()> {
    let guard = TEST_STATE.lock().unwrap();
    ke::time::CLOCK.with(|clock| clock.set((100_000, 0)));
    ke::scheduler::ADVANCE_ON_YIELD.with(|delta| delta.set(0));
    ps::thread::set_tid(701);
    guard
}
fn pending(
    handles: &[HANDLE],
    all: bool,
    tid: u64,
    timeout: Option<i64>,
) -> ntdll::file::QueuedWait {
    match ntdll::file::begin_wait(handles, all, 100, tid, timeout).unwrap() {
        ntdll::file::WaitStart::Pending(wait) => wait,
        ntdll::file::WaitStart::Complete(status) => panic!("unexpected completion {status:#x}"),
    }
}
#[test]
fn event_and_semaphore_state_transitions_are_atomic() {
    let _guard = state_guard();
    let auto = create_event_kernel(1, true).unwrap();
    let manual = create_event_kernel(0, true).unwrap();
    assert_eq!(
        nt_wait_for_single_object_kernel(auto, 0, Some(0)),
        STATUS_SUCCESS
    );
    assert_eq!(
        nt_wait_for_single_object_kernel(auto, 0, Some(0)),
        STATUS_TIMEOUT
    );
    assert_eq!(
        nt_wait_for_single_object_kernel(manual, 0, Some(0)),
        STATUS_SUCCESS
    );
    assert_eq!(
        nt_wait_for_single_object_kernel(manual, 0, Some(0)),
        STATUS_SUCCESS
    );
    let mut previous = 123;
    unsafe {
        assert_eq!(nt_set_event(auto, &mut previous), STATUS_SUCCESS);
        assert_eq!(previous, 0);
        assert_eq!(nt_reset_event(auto, &mut previous), STATUS_SUCCESS);
        assert_eq!(previous, 1);
        assert_eq!(
            nt_set_event(auto, 1usize as *mut u32),
            STATUS_ACCESS_VIOLATION
        );
    }
    assert_eq!(
        nt_wait_for_single_object_kernel(auto, 0, Some(0)),
        STATUS_TIMEOUT
    );

    unsafe {
        assert_eq!(nt_set_event(auto, core::ptr::null_mut()), STATUS_SUCCESS);
    }
    assert_eq!(
        nt_wait_for_multiple_objects_kernel(2, &[auto, auto], 1, 0, Some(0)),
        STATUS_SUCCESS
    );
    assert_eq!(
        nt_wait_for_single_object_kernel(auto, 0, Some(0)),
        STATUS_TIMEOUT,
        "duplicate WaitAny handles must not restore consumed state"
    );
    let semaphore = create_semaphore_kernel(1, 2).unwrap();
    assert_eq!(
        nt_wait_for_multiple_objects_kernel(2, &[semaphore, auto], 0, 0, Some(0)),
        STATUS_TIMEOUT
    );
    assert_eq!(
        nt_wait_for_single_object_kernel(semaphore, 0, Some(0)),
        STATUS_SUCCESS,
        "failed WaitAll must not consume the semaphore"
    );
    unsafe {
        assert_eq!(
            nt_release_semaphore(semaphore, 2, &mut previous),
            STATUS_SUCCESS
        );
        assert_eq!(previous, 0);
        assert_eq!(
            nt_release_semaphore(semaphore, 1, &mut previous),
            0xc0000047u32 as i32
        );
        assert_eq!(
            nt_release_semaphore(semaphore, 0, &mut previous),
            STATUS_INVALID_PARAMETER
        );
    }
    assert_eq!(
        nt_wait_for_multiple_objects_kernel(2, &[auto, semaphore], 1, 0, Some(0)),
        1
    );
    assert_eq!(
        nt_wait_for_single_object_kernel(semaphore, 0, Some(0)),
        STATUS_SUCCESS
    );
    assert_eq!(
        nt_wait_for_single_object_kernel(semaphore, 0, Some(0)),
        STATUS_TIMEOUT
    );
    assert_eq!(
        nt_wait_for_multiple_objects_kernel(2, &[manual, manual], 0, 0, Some(0)),
        STATUS_INVALID_PARAMETER
    );
    unsafe {
        for h in [auto, manual, semaphore] {
            assert_eq!(nt_close_sync(h), STATUS_SUCCESS);
        }
    }
}
#[test]
fn mutant_creation_uses_user_copy_and_rejects_invalid_output() {
    let _guard = state_guard();
    unsafe {
        assert_eq!(
            nt_create_mutant(1usize as *mut HANDLE, 0, core::ptr::null_mut(), 0),
            STATUS_ACCESS_VIOLATION
        );
        let mut handle = core::ptr::null_mut();
        assert_eq!(
            nt_create_mutant(&mut handle, 0, core::ptr::null_mut(), 0),
            STATUS_SUCCESS
        );
        assert!(
            ntdll::file::lookup_handle(handle).unwrap().kind == ntdll::file::HandleKind::Mutant
        );
        assert_eq!(nt_close_sync(handle), STATUS_SUCCESS);
    }
}

#[test]
fn timer_wrappers_share_state_and_validate_arguments() {
    let _guard = state_guard();
    let mut timer = core::ptr::null_mut();
    let mut previous = 123u8;
    let due = -100_000i64;
    unsafe {
        assert_eq!(
            nt_create_timer(&mut timer, 0, core::ptr::null_mut(), 1),
            STATUS_SUCCESS
        );
        assert_eq!(
            nt_set_timer(
                timer,
                &due,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                0,
                -1,
                &mut previous
            ),
            STATUS_INVALID_PARAMETER
        );
        assert_eq!(
            nt_set_timer(
                timer,
                1usize as *const i64,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                0,
                0,
                &mut previous
            ),
            STATUS_ACCESS_VIOLATION
        );
        assert_eq!(
            nt_set_timer(
                timer,
                &due,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                0,
                0,
                &mut previous
            ),
            STATUS_SUCCESS
        );
        assert_eq!(previous, 0);
        let mut info = ntdll::timer::TimerBasicInformation {
            remaining_time: LargeInteger::new(0),
            timer_state: 123,
        };
        let mut length = 0;
        assert_eq!(
            ntdll::timer::nt_query_timer(
                timer,
                0,
                (&mut info as *mut ntdll::timer::TimerBasicInformation).cast(),
                core::mem::size_of_val(&info) as u32,
                &mut length
            ),
            STATUS_SUCCESS
        );
        assert_eq!(info.remaining_time.quad_part, 100_000);
        assert_eq!(info.timer_state, 0);
        assert_eq!(length, 16);
        assert_eq!(nt_cancel_timer(timer, &mut previous), STATUS_SUCCESS);
        assert_eq!(previous, 0);
        assert_eq!(nt_cancel_timer(timer, &mut previous), STATUS_SUCCESS);
        assert_eq!(nt_close_sync(timer), STATUS_SUCCESS);
        // Timer state slots must be reclaimed; repeated create/close exceeds
        // the fixed capacity and used to exhaust it permanently.
        for _ in 0..128 {
            assert_eq!(
                nt_create_timer(&mut timer, 0, core::ptr::null_mut(), 0),
                STATUS_SUCCESS
            );
            assert_eq!(nt_close_sync(timer), STATUS_SUCCESS);
        }
    }
}

#[test]
fn delay_observes_relative_and_absolute_deadlines() {
    let _guard = state_guard();
    ke::scheduler::ADVANCE_ON_YIELD.with(|delta| delta.set(1));
    let start = ke::time::get_time_snapshot();
    assert_eq!(nt_delay_execution_kernel(0, -17), STATUS_SUCCESS);
    assert_eq!(
        ke::time::get_time_snapshot().interrupt_time,
        start.interrupt_time + 17
    );
    assert_eq!(nt_delay_execution_kernel(0, 0), STATUS_SUCCESS);
    assert_eq!(nt_delay_execution_kernel(0, 100_000), STATUS_SUCCESS);
    let next = ke::time::get_system_time() + 13;
    assert_eq!(nt_delay_execution_kernel(0, next as i64), STATUS_SUCCESS);
    assert_eq!(ke::time::get_system_time(), next);
    ke::scheduler::ADVANCE_ON_YIELD.with(|delta| delta.set(0));
    assert_eq!(nt_status_to_dos_error(STATUS_NOT_SUPPORTED), 50);
    assert_eq!(nt_status_to_dos_error(STATUS_MUTEX_NOT_OWNED), 288);
    assert_eq!(nt_status_to_dos_error(0xc0000047u32 as i32), 298);
}

#[test]
fn section_basic_information_matches_native_layout_and_close_lifetime() {
    let _guard = state_guard();
    use ntdll::{file, section::*};
    assert_eq!(core::mem::size_of::<SectionBasicInformation>(), 24);
    assert_eq!(
        core::mem::offset_of!(SectionBasicInformation, base_address),
        0
    );
    assert_eq!(
        core::mem::offset_of!(SectionBasicInformation, attributes),
        8
    );
    assert_eq!(
        core::mem::offset_of!(SectionBasicInformation, maximum_size),
        16
    );
    unsafe {
        let mut section = core::ptr::null_mut();
        let mut size = 0x4000i64;
        assert_eq!(
            nt_create_section(
                &mut section,
                1,
                core::ptr::null_mut(),
                &mut size,
                4,
                0x0800_0000,
                core::ptr::null_mut()
            ),
            STATUS_SUCCESS
        );
        let mut invalid_output = core::ptr::null_mut();
        assert_eq!(
            nt_create_section(
                &mut invalid_output,
                1,
                core::ptr::null_mut(),
                &mut size,
                4,
                0x0800_0000,
                0x10000usize as HANDLE
            ),
            STATUS_INVALID_HANDLE
        );
        assert!(invalid_output.is_null());
        assert_eq!(
            nt_create_section(
                1usize as *mut HANDLE,
                1,
                core::ptr::null_mut(),
                &mut size,
                4,
                0x0800_0000,
                core::ptr::null_mut()
            ),
            STATUS_ACCESS_VIOLATION
        );
        let mut bytes = [0xccu8; 32];
        let mut length = 0usize;
        assert_eq!(
            nt_query_section(section, 0, bytes.as_mut_ptr().cast(), 16, &mut length),
            STATUS_INFO_LENGTH_MISMATCH
        );
        assert!(bytes.iter().all(|&b| b == 0xcc));
        assert_eq!(
            nt_query_section(section, 0, bytes.as_mut_ptr().cast(), 24, &mut length),
            STATUS_SUCCESS
        );
        assert_eq!(length, 24);
        assert_eq!(&bytes[..8], &[0; 8]);
        assert_eq!(&bytes[8..12], &0x0800_0000u32.to_le_bytes());
        assert_eq!(&bytes[12..16], &[0; 4]);
        assert_eq!(&bytes[16..24], &size.to_le_bytes());
        assert_eq!(&bytes[24..], &[0xcc; 8]);
        let mut base = 0x1234usize as PVOID;
        let mut offset = 17i64;
        let mut view_size = 0x4000usize;
        assert_eq!(
            nt_map_view_of_section(
                section,
                -1isize as HANDLE,
                &mut base,
                0,
                0,
                &mut offset,
                &mut view_size,
                1,
                0,
                4
            ),
            STATUS_NOT_IMPLEMENTED
        );
        assert_eq!(base as usize, 0x1234);
        assert_eq!(offset, 17);
        assert_eq!(view_size, 0x4000);
        assert_eq!(
            nt_unmap_view_of_section(-1isize as HANDLE, base),
            STATUS_NOT_IMPLEMENTED
        );
        assert!(file::free_handle(section));
        assert_eq!(
            nt_query_section(section, 0, bytes.as_mut_ptr().cast(), 24, &mut length),
            STATUS_INVALID_HANDLE
        );
        // Closing via NtClose's shared table must also discard section metadata.
        let replacement = file::alloc_handle(file::HandleKind::Event, 0);
        assert_eq!(
            nt_query_section(replacement, 0, bytes.as_mut_ptr().cast(), 24, &mut length),
            STATUS_INVALID_HANDLE
        );
        assert!(file::free_handle(replacement));
        for _ in 0..128 {
            assert_eq!(
                nt_create_section(
                    &mut section,
                    1,
                    core::ptr::null_mut(),
                    &mut size,
                    4,
                    0x0400_0000,
                    core::ptr::null_mut()
                ),
                STATUS_SUCCESS
            );
            assert_eq!(
                nt_query_section(section, 0, bytes.as_mut_ptr().cast(), 24, &mut length),
                STATUS_SUCCESS
            );
            assert_eq!(&bytes[8..12], &0x0400_0000u32.to_le_bytes());
            assert_eq!(nt_close_section(section), STATUS_SUCCESS);
        }
    }
}

#[test]
fn query_sync_objects_preserves_layout_state_and_error_outputs() {
    let _guard = state_guard();
    let event = create_event_kernel(1, true).unwrap();
    let semaphore = create_semaphore_kernel(3, 9).unwrap();
    let mutant = create_mutant_kernel(false).unwrap();
    unsafe {
        for (handle, query, expected) in [
            (
                event,
                nt_query_event as unsafe extern "C" fn(HANDLE, u32, PVOID, u32, *mut u32) -> i32,
                [1i32, 1],
            ),
            (semaphore, nt_query_semaphore, [3, 9]),
            (mutant, nt_query_mutant, [1, 0]),
        ] {
            let mut bytes = [0xcc; 24];
            let mut size = 0x1234;
            assert_eq!(
                query(handle, 0, bytes.as_mut_ptr().cast(), 8, &mut size),
                STATUS_SUCCESS
            );
            assert_eq!(size, 8);
            assert_eq!(&bytes[..4], &expected[0].to_le_bytes());
            assert_eq!(&bytes[4..8], &expected[1].to_le_bytes());
            assert_eq!(&bytes[8..], &[0xcc; 16]);
            bytes.fill(0xcc);
            assert_eq!(
                query(handle, 0, bytes.as_mut_ptr().cast(), 7, &mut size),
                STATUS_INFO_LENGTH_MISMATCH
            );
            assert_eq!(
                query(handle, 42, bytes.as_mut_ptr().cast(), 8, &mut size),
                STATUS_INVALID_INFO_CLASS
            );
            assert_eq!(
                query(handle, 0, 1usize as PVOID, 8, &mut size),
                STATUS_ACCESS_VIOLATION
            );
            assert_eq!(
                query(handle, 0, bytes.as_mut_ptr().cast(), 8, 1usize as *mut u32),
                STATUS_ACCESS_VIOLATION
            );
            assert_eq!(bytes, [0xcc; 24]);
            assert_eq!(
                query(
                    0x10000usize as HANDLE,
                    0,
                    bytes.as_mut_ptr().cast(),
                    8,
                    &mut size
                ),
                STATUS_INVALID_HANDLE
            );
        }
        let mut bytes = [0xcc; 8];
        assert_eq!(
            nt_query_event(
                semaphore,
                0,
                bytes.as_mut_ptr().cast(),
                8,
                core::ptr::null_mut()
            ),
            STATUS_OBJECT_TYPE_MISMATCH
        );
        for handle in [event, semaphore, mutant] {
            assert_eq!(nt_close_sync(handle), STATUS_SUCCESS);
        }
    }
}

#[test]
fn mutant_recursion_ownership_aliases_and_abandonment() {
    let _guard = state_guard();
    use ntdll::file::*;
    ps::thread::set_tid(701);
    let mutant = create_mutant_kernel(true).unwrap();
    let alias = duplicate_sync_handle(mutant).unwrap();
    let event = create_event_kernel(0, true).unwrap();
    let mut info = [0xcc; 16];
    let mut previous = 123;
    unsafe {
        assert_eq!(
            nt_wait_for_single_object_kernel(alias, 0, Some(0)),
            STATUS_SUCCESS
        );
        assert_eq!(
            nt_query_mutant(
                mutant,
                0,
                info.as_mut_ptr().cast(),
                8,
                core::ptr::null_mut()
            ),
            STATUS_SUCCESS
        );
        assert_eq!(&info[..4], &(-1i32).to_le_bytes());
        assert_eq!(&info[4..8], &[1, 0, 0, 0]);
        assert_eq!(nt_release_mutant(alias, &mut previous), STATUS_SUCCESS);
        assert_eq!(previous as i32, -1);
        ps::thread::set_tid(702);
        assert_eq!(
            nt_release_mutant(mutant, &mut previous),
            STATUS_MUTEX_NOT_OWNED
        );
        assert_eq!(
            nt_wait_for_single_object_kernel(mutant, 0, Some(0)),
            STATUS_TIMEOUT
        );
        ps::thread::set_tid(701);
        assert_eq!(
            nt_query_mutant(
                alias,
                1,
                info.as_mut_ptr().cast(),
                16,
                core::ptr::null_mut()
            ),
            STATUS_SUCCESS
        );
        assert_eq!(&info[..8], &100u64.to_le_bytes());
        assert_eq!(&info[8..], &701u64.to_le_bytes());
        assert_eq!(
            nt_wait_for_multiple_objects_kernel(2, &[mutant, alias], 0, 0, Some(0)),
            STATUS_INVALID_PARAMETER
        );
        assert_eq!(
            nt_release_mutant(mutant, 1usize as *mut u32),
            STATUS_ACCESS_VIOLATION
        );
        assert_eq!(nt_release_mutant(mutant, &mut previous), STATUS_SUCCESS);
        assert_eq!(previous, 0);
        assert_eq!(
            nt_release_mutant(mutant, &mut previous),
            STATUS_MUTEX_NOT_OWNED
        );
        // Abandoning an owning thread releases all recursion, once.
        assert_eq!(
            nt_wait_for_single_object_kernel(mutant, 0, Some(0)),
            STATUS_SUCCESS
        );
        assert_eq!(
            nt_wait_for_single_object_kernel(mutant, 0, Some(0)),
            STATUS_SUCCESS
        );
        abandon_mutants(701);
        ps::thread::set_tid(702);
        assert_eq!(
            nt_query_mutant(
                mutant,
                0,
                info.as_mut_ptr().cast(),
                8,
                core::ptr::null_mut()
            ),
            STATUS_SUCCESS
        );
        assert_eq!(&info[..8], &[1, 0, 0, 0, 0, 1, 0, 0]);
        assert_eq!(
            nt_wait_for_multiple_objects_kernel(2, &[event, mutant], 0, 0, Some(0)),
            STATUS_ABANDONED_WAIT_0
        );
        assert_eq!(
            nt_wait_for_single_object_kernel(alias, 0, Some(0)),
            STATUS_SUCCESS
        );
        assert_eq!(nt_close_sync(mutant), STATUS_SUCCESS);
        assert_eq!(nt_release_mutant(alias, &mut previous), STATUS_SUCCESS);
        assert_eq!(previous as i32, -1);
        assert_eq!(nt_close_sync(alias), STATUS_SUCCESS);
        assert_eq!(nt_close_sync(event), STATUS_SUCCESS);
        ps::thread::set_tid(701);
    }
}

#[test]
fn wait_any_aliases_consume_shared_objects_once() {
    let _guard = state_guard();
    use ntdll::file::*;
    let semaphore = create_semaphore_kernel(2, 3).unwrap();
    let alias = duplicate_sync_handle(semaphore).unwrap();
    assert_eq!(
        nt_wait_for_multiple_objects_kernel(2, &[semaphore, alias], 1, 0, Some(0)),
        STATUS_SUCCESS
    );
    assert_eq!(lookup_handle(alias).unwrap().target as u32, 1);
    assert!(free_handle(semaphore));
    assert_eq!(
        nt_wait_for_single_object_kernel(alias, 0, Some(0)),
        STATUS_SUCCESS
    );
    assert_eq!(
        nt_wait_for_single_object_kernel(alias, 0, Some(0)),
        STATUS_TIMEOUT
    );
    assert!(free_handle(alias));
}

#[test]
fn timers_expire_repeat_cancel_and_follow_the_correct_clock() {
    let _guard = state_guard();
    use ntdll::{file::*, timer};
    ke::time::CLOCK.with(|clock| clock.set((1_000_000, 0)));
    let auto = timer::create_timer_kernel(1).unwrap();
    let manual = timer::create_timer_kernel(0).unwrap();
    let mut previous = 0xcc;
    let arm = |handle, due: i64, period, previous: *mut u8| unsafe {
        nt_set_timer(
            handle,
            &due,
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            0,
            period,
            previous,
        )
    };
    assert_eq!(arm(auto, -50_000, 2, &mut previous), STATUS_SUCCESS);
    assert_eq!(previous, 0);
    ke::time::advance(49_999);
    assert_eq!(
        nt_wait_for_single_object_kernel(auto, 0, Some(0)),
        STATUS_TIMEOUT
    );
    ke::time::advance(1);
    tick_timers();
    assert_eq!(
        nt_wait_for_single_object_kernel(auto, 0, Some(0)),
        STATUS_SUCCESS
    );
    assert_eq!(
        nt_wait_for_single_object_kernel(auto, 0, Some(0)),
        STATUS_TIMEOUT
    );
    // Expiration coalesces across several missed periods.
    ke::time::advance(70_000);
    tick_timers();
    assert_eq!(
        nt_wait_for_single_object_kernel(auto, 0, Some(0)),
        STATUS_SUCCESS
    );
    assert_eq!(
        nt_wait_for_single_object_kernel(auto, 0, Some(0)),
        STATUS_TIMEOUT
    );
    let mut bytes = [0xcc; 24];
    let mut size = 0;
    unsafe {
        assert_eq!(
            timer::nt_query_timer(auto, 0, bytes.as_mut_ptr().cast(), 16, &mut size),
            STATUS_SUCCESS
        );
        assert_eq!(i64::from_le_bytes(bytes[..8].try_into().unwrap()), 10_000);
        assert_eq!(&bytes[8..16], &[0; 8]);
        assert_eq!(&bytes[16..], &[0xcc; 8]);
        assert_eq!(size, 16);
    }
    // Rearming resets state, while cancelling preserves a signaled timer.
    assert_eq!(arm(manual, 0, 0, &mut previous), STATUS_SUCCESS);
    assert_eq!(
        nt_wait_for_single_object_kernel(manual, 0, Some(0)),
        STATUS_SUCCESS
    );
    assert_eq!(
        nt_wait_for_single_object_kernel(manual, 0, Some(0)),
        STATUS_SUCCESS
    );
    unsafe {
        assert_eq!(nt_cancel_timer(manual, &mut previous), STATUS_SUCCESS);
    }
    assert_eq!(previous, 1);
    assert_eq!(
        nt_wait_for_single_object_kernel(manual, 0, Some(0)),
        STATUS_SUCCESS
    );
    assert_eq!(arm(manual, -100_000, 0, &mut previous), STATUS_SUCCESS);
    assert_eq!(previous, 1);
    assert_eq!(
        nt_wait_for_single_object_kernel(manual, 0, Some(0)),
        STATUS_TIMEOUT
    );
    // A wall-clock adjustment must not fire a relative timer early.
    ke::time::CLOCK.with(|clock| {
        let (wall, uptime) = clock.get();
        clock.set((wall + 10_000_000, uptime));
    });
    assert_eq!(
        nt_wait_for_single_object_kernel(manual, 0, Some(0)),
        STATUS_TIMEOUT
    );
    ke::time::advance(100_000);
    assert_eq!(
        nt_wait_for_single_object_kernel(manual, 0, Some(0)),
        STATUS_SUCCESS
    );
    let absolute = ke::time::get_system_time() + 50_000;
    assert_eq!(
        arm(manual, absolute as i64, 0, &mut previous),
        STATUS_SUCCESS
    );
    ke::time::CLOCK.with(|clock| {
        let (_, uptime) = clock.get();
        clock.set((absolute, uptime));
    });
    assert_eq!(
        nt_wait_for_single_object_kernel(manual, 0, Some(0)),
        STATUS_SUCCESS
    );
    assert_eq!(arm(auto, -50_000, 0, &mut previous), STATUS_SUCCESS);
    unsafe {
        assert_eq!(nt_cancel_timer(auto, &mut previous), STATUS_SUCCESS);
    }
    ke::time::advance(100_000);
    assert_eq!(
        nt_wait_for_single_object_kernel(auto, 0, Some(0)),
        STATUS_TIMEOUT
    );
    // Bad return-length pointers must not first write the data buffer.
    bytes.fill(0xcc);
    unsafe {
        assert_eq!(
            timer::nt_query_timer(auto, 0, bytes.as_mut_ptr().cast(), 16, 1usize as *mut u32),
            STATUS_ACCESS_VIOLATION
        );
        assert_eq!(bytes, [0xcc; 24]);
        assert_eq!(nt_close_sync(auto), STATUS_SUCCESS);
        assert_eq!(nt_close_sync(manual), STATUS_SUCCESS);
    }
}

#[test]
fn queued_pulses_release_only_existing_satisfiable_waits() {
    let _guard = state_guard();
    for manual in [false, true] {
        let event = create_event_kernel(u32::from(!manual), false).unwrap();
        let semaphore = create_semaphore_kernel(0, 1).unwrap();
        let first = pending(&[event], false, 801, None);
        let second = pending(&[event], false, 802, None);
        let all = pending(&[event, semaphore], true, 803, None);
        // A failed output probe cannot pulse or release a waiter.
        assert_eq!(
            unsafe { nt_pulse_event(event, 1usize as *mut u32) },
            STATUS_ACCESS_VIOLATION
        );
        assert_eq!(first.poll(), None);
        let mut previous = 9;
        assert_eq!(
            unsafe { nt_pulse_event(event, &mut previous) },
            STATUS_SUCCESS
        );
        assert_eq!(previous, 0);
        assert_eq!(first.poll(), Some(STATUS_SUCCESS));
        assert_eq!(
            second.poll(),
            if manual { Some(STATUS_SUCCESS) } else { None }
        );
        assert_eq!(all.poll(), None, "pulse cannot satisfy a partial WaitAll");
        assert_eq!(
            nt_wait_for_single_object_kernel(event, 0, Some(0)),
            STATUS_TIMEOUT
        );
        assert_eq!(
            unsafe { nt_release_semaphore(semaphore, 1, core::ptr::null_mut()) },
            STATUS_SUCCESS
        );
        assert_eq!(
            all.poll(),
            None,
            "the pulse must not persist after the operation"
        );
        assert_eq!(
            unsafe { nt_pulse_event(event, core::ptr::null_mut()) },
            STATUS_SUCCESS
        );
        if !manual {
            assert_eq!(second.poll(), Some(STATUS_SUCCESS));
            assert_eq!(all.poll(), None);
            assert_eq!(
                unsafe { nt_pulse_event(event, core::ptr::null_mut()) },
                STATUS_SUCCESS
            );
        }
        assert_eq!(all.poll(), Some(STATUS_SUCCESS));
        assert_eq!(
            nt_wait_for_single_object_kernel(semaphore, 0, Some(0)),
            STATUS_TIMEOUT
        );
        drop((first, second, all));
        assert!(ntdll::file::free_handle(event));
        assert!(ntdll::file::free_handle(semaphore));
    }
}

#[test]
fn queued_waits_pin_object_identity_across_handle_close_and_reuse() {
    let _guard = state_guard();
    let event = create_event_kernel(1, false).unwrap();
    let alias = ntdll::file::duplicate_sync_handle(event).unwrap();
    let wait = pending(&[event, alias], false, 810, None);
    assert!(ntdll::file::free_handle(event));
    let replacement = create_event_kernel(1, true).unwrap();
    assert_eq!(replacement, event, "exercise actual handle slot reuse");
    assert_eq!(
        wait.poll(),
        None,
        "new handle state must not redirect a pending wait"
    );
    assert_eq!(
        unsafe { nt_set_event(alias, core::ptr::null_mut()) },
        STATUS_SUCCESS
    );
    assert_eq!(wait.poll(), Some(STATUS_SUCCESS));
    assert!(ntdll::file::free_handle(alias));
    drop(wait); // retires the old object after its last alias was closed
    assert_eq!(
        nt_wait_for_single_object_kernel(replacement, 0, Some(0)),
        STATUS_SUCCESS
    );
    assert!(ntdll::file::free_handle(replacement));
    // The old object has no remaining handles. Cancellation must reclaim it.
    for _ in 0..4200 {
        let event = create_event_kernel(1, false).unwrap();
        let wait = pending(&[event], false, 810, None);
        assert!(ntdll::file::free_handle(event));
        drop(wait);
    }
}

#[test]
fn queued_deadlines_use_wall_or_interrupt_time_and_timers_wake_waiters() {
    let _guard = state_guard();
    let event = create_event_kernel(0, false).unwrap();
    let relative = pending(&[event], false, 811, Some(-20));
    let absolute = pending(&[event], false, 812, Some(100_020));
    ke::time::CLOCK.with(|clock| clock.set((200_000, 19)));
    ntdll::file::tick_timers();
    assert_eq!(absolute.poll(), Some(STATUS_TIMEOUT));
    assert_eq!(relative.poll(), None);
    ke::time::CLOCK.with(|clock| clock.set((0, 20)));
    assert_eq!(relative.poll(), Some(STATUS_TIMEOUT));
    drop((relative, absolute));
    let timer = ntdll::timer::create_timer_kernel(1).unwrap();
    let timer_wait = pending(&[timer], false, 813, None);
    let due = -10;
    assert_eq!(
        unsafe {
            nt_set_timer(
                timer,
                &due,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                0,
                1,
                core::ptr::null_mut(),
            )
        },
        STATUS_SUCCESS
    );
    ke::time::advance(10);
    ntdll::file::tick_timers();
    assert_eq!(timer_wait.poll(), Some(STATUS_SUCCESS));
    assert_eq!(
        nt_wait_for_single_object_kernel(timer, 0, Some(0)),
        STATUS_TIMEOUT
    );
    drop(timer_wait);
    assert!(ntdll::file::free_handle(timer));
    assert!(ntdll::file::free_handle(event));
    // Exercise the syscall's yielding path without real-time sleeps.
    let event = create_event_kernel(0, false).unwrap();
    ke::scheduler::ADVANCE_ON_YIELD.with(|delta| delta.set(3));
    assert_eq!(
        nt_wait_for_single_object_kernel(event, 0, Some(-7)),
        STATUS_TIMEOUT
    );
    assert!(ntdll::file::free_handle(event));
}

#[test]
fn queued_thread_exit_cancels_its_wait_and_abandons_owned_mutants() {
    let _guard = state_guard();
    let mutant = create_mutant_kernel(true).unwrap();
    let event = create_event_kernel(0, false).unwrap();
    let exiting = pending(&[event], false, 701, None);
    let acquiring = pending(&[mutant], false, 820, None);
    ntdll::file::abandon_mutants(701);
    assert_eq!(exiting.poll(), Some(STATUS_THREAD_IS_TERMINATING));
    assert_eq!(acquiring.poll(), Some(STATUS_ABANDONED_WAIT_0));
    let replacement = pending(&[event], false, 821, None);
    drop(exiting); // a stale guard cannot remove the new wait in its reused slot
    assert_eq!(replacement.poll(), None);
    assert_eq!(
        unsafe { nt_set_event(event, core::ptr::null_mut()) },
        STATUS_SUCCESS
    );
    assert_eq!(replacement.poll(), Some(STATUS_SUCCESS));
    drop((replacement, acquiring));
    ps::thread::set_tid(820);
    assert_eq!(
        unsafe { nt_release_mutant(mutant, core::ptr::null_mut()) },
        STATUS_SUCCESS
    );
    for handle in [mutant, event] {
        assert!(ntdll::file::free_handle(handle));
    }
}

#[test]
fn waitall_recursion_overflow_does_not_consume_other_objects() {
    let _guard = state_guard();
    let mutant = create_mutant_kernel(true).unwrap();
    let event = create_event_kernel(1, true).unwrap();
    ntdll::file::with_sync_handles(&[mutant], |entries| {
        if let ntdll::file::DispatcherState::Mutant(state) = &mut entries[0].dispatcher {
            state.depth = 0x8000_0001; // SignalState = MINLONG
        }
        Ok(())
    })
    .unwrap();
    assert_eq!(
        nt_wait_for_multiple_objects_kernel(2, &[event, mutant], 0, 0, Some(0)),
        STATUS_MUTANT_LIMIT_EXCEEDED
    );
    assert_eq!(
        nt_wait_for_single_object_kernel(event, 0, Some(0)),
        STATUS_SUCCESS
    );
    for handle in [mutant, event] {
        assert!(ntdll::file::free_handle(handle));
    }
}

#[test]
fn public_wait_helpers_share_state_and_resume_after_another_thread_signals() {
    let _guard = state_guard();
    let event = create_event_kernel(1, false).unwrap();
    let (tx, rx) = std::sync::mpsc::sync_channel(0);
    let event_value = event as usize;
    let signaler = std::thread::spawn(move || {
        ps::thread::set_tid(900);
        rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        assert!(ntdll::wait::signal_event(event_value as HANDLE));
    });
    ke::scheduler::YIELD_HOOK
        .with(|hook| *hook.borrow_mut() = Some(Box::new(move || tx.send(()).unwrap())));
    assert_eq!(
        ntdll::wait::nt_wait_for_single_object_kernel(event, 0, None),
        STATUS_SUCCESS
    );
    signaler.join().unwrap();
    assert_eq!(
        ntdll::wait::nt_wait_for_single_object_kernel(event, 0, Some(0)),
        STATUS_TIMEOUT
    );
    assert!(!ntdll::wait::signal_event(usize::MAX as HANDLE));
    assert!(!ntdll::wait::release_mutex(event));
    assert_eq!(ntdll::wait::WAIT_TYPE_ALL, 0);
    assert_eq!(ntdll::wait::WAIT_TYPE_ANY, 1);
    assert!(ntdll::file::free_handle(event));
}

#[test]
fn wait_capacity_and_thread_cancellation_release_every_pin() {
    let _guard = state_guard();
    let event = create_event_kernel(1, false).unwrap();
    let waits: Vec<_> = (0..256)
        .map(|_| pending(&[event], false, 830, None))
        .collect();
    assert!(matches!(
        ntdll::file::begin_wait(&[event], false, 100, 831, None),
        Err(STATUS_NO_MEMORY)
    ));
    ntdll::file::abandon_mutants(830);
    let replacement = pending(&[event], false, 831, None);
    assert!(waits
        .iter()
        .all(|wait| wait.poll() == Some(STATUS_THREAD_IS_TERMINATING)));
    drop(waits);
    assert_eq!(replacement.poll(), None);
    assert_eq!(
        unsafe { nt_set_event(event, core::ptr::null_mut()) },
        STATUS_SUCCESS
    );
    assert_eq!(replacement.poll(), Some(STATUS_SUCCESS));
    drop(replacement);
    assert!(ntdll::file::free_handle(event));
}

#[test]
fn signal_and_wait_validates_both_objects_and_preserves_signal_order() {
    let _guard = state_guard();
    let signal = create_event_kernel(1, false).unwrap();
    let target = create_event_kernel(1, false).unwrap();
    let waiting = pending(&[signal], false, 840, None);
    // Bad wait target/timeout must not publish the signal.
    assert_eq!(
        signal_and_wait_kernel(signal, usize::MAX as HANDLE, 0, Some(0)),
        STATUS_INVALID_HANDLE
    );
    assert_eq!(
        unsafe { nt_signal_and_wait_for_single_object(signal, target, 0, 1usize as *const i64) },
        STATUS_ACCESS_VIOLATION
    );
    assert_eq!(waiting.poll(), None);
    assert_eq!(
        signal_and_wait_kernel(signal, target, 0, Some(0)),
        STATUS_TIMEOUT
    );
    assert_eq!(waiting.poll(), Some(STATUS_SUCCESS));
    drop(waiting);
    assert_eq!(
        nt_wait_for_single_object_kernel(signal, 0, Some(0)),
        STATUS_TIMEOUT
    );
    assert_eq!(
        signal_and_wait_kernel(signal, signal, 0, Some(0)),
        STATUS_SUCCESS
    );
    assert_eq!(
        nt_wait_for_single_object_kernel(signal, 0, Some(0)),
        STATUS_TIMEOUT
    );
    let semaphore = create_semaphore_kernel(1, 1).unwrap();
    assert_eq!(
        signal_and_wait_kernel(semaphore, target, 0, Some(0)),
        STATUS_SEMAPHORE_LIMIT_EXCEEDED
    );
    assert_eq!(
        nt_wait_for_single_object_kernel(semaphore, 0, Some(0)),
        STATUS_SUCCESS
    );
    assert_eq!(
        signal_and_wait_kernel(semaphore, semaphore, 0, Some(0)),
        STATUS_SUCCESS
    );
    let mutant = create_mutant_kernel(true).unwrap();
    ps::thread::set_tid(841);
    assert_eq!(
        signal_and_wait_kernel(mutant, target, 0, Some(0)),
        STATUS_MUTEX_NOT_OWNED
    );
    ps::thread::set_tid(701);
    assert_eq!(
        signal_and_wait_kernel(mutant, target, 0, Some(0)),
        STATUS_TIMEOUT
    );
    ps::thread::set_tid(841);
    assert_eq!(
        nt_wait_for_single_object_kernel(mutant, 0, Some(0)),
        STATUS_SUCCESS
    );
    let timer = ntdll::timer::create_timer_kernel(1).unwrap();
    assert_eq!(
        signal_and_wait_kernel(timer, target, 0, Some(0)),
        STATUS_OBJECT_TYPE_MISMATCH
    );
    for handle in [signal, target, semaphore, mutant, timer] {
        assert!(ntdll::file::free_handle(handle));
    }
}

#[test]
fn kernel32_sync_wrappers_translate_failures_and_preserve_success_last_error() {
    let _guard = state_guard();
    use kernel32::{error::*, sync as win32};
    let event = create_event_kernel(1, false).unwrap();
    let mutex = create_mutant_kernel(false).unwrap();
    let semaphore = create_semaphore_kernel(0, 1).unwrap();
    set_last_error(1234);
    assert_eq!(unsafe { win32::set_event(event) }, 1);
    assert_eq!(get_last_error(), 1234);
    assert_eq!(unsafe { win32::wait_for_single_object(event, 0) }, 0);
    assert_eq!(unsafe { win32::wait_for_single_object(event, 0) }, 0x102);
    assert_eq!(get_last_error(), 1234);
    assert_eq!(unsafe { win32::set_event(usize::MAX as HANDLE) }, 0);
    assert_eq!(get_last_error(), 6);
    assert_eq!(unsafe { win32::release_mutex(mutex) }, 0);
    assert_eq!(get_last_error(), 288);
    assert_eq!(
        unsafe { win32::release_semaphore(semaphore, 2, core::ptr::null_mut()) },
        0
    );
    assert_eq!(get_last_error(), 298);
    assert_eq!(
        unsafe { win32::signal_object_and_wait(semaphore, semaphore, 0, 0) },
        0
    );
    assert_eq!(
        unsafe { win32::signal_object_and_wait(event, usize::MAX as HANDLE, 0, 0) },
        u32::MAX
    );
    assert_eq!(get_last_error(), 6);
    assert_eq!(unsafe { win32::wait_for_single_object(event, 0) }, 0x102);
    ke::scheduler::ADVANCE_ON_YIELD.with(|delta| delta.set(5_000));
    assert_eq!(unsafe { win32::wait_for_single_object(event, 1) }, 0x102);
    for handle in [event, mutex, semaphore] {
        assert!(ntdll::file::free_handle(handle));
    }
}

#[test]
fn win32_sleep_uses_relative_native_time_and_zero_yields() {
    let _guard = state_guard();
    let before = ke::time::get_time_snapshot().interrupt_time;
    ke::scheduler::ADVANCE_ON_YIELD.with(|delta| delta.set(5_000));
    assert_eq!(kernel32::sleep_ex(3, false), 0);
    assert_eq!(
        ke::time::get_time_snapshot().interrupt_time - before,
        30_000
    );
    let before = ke::time::get_time_snapshot().interrupt_time;
    assert_eq!(kernel32::sleep_ex(0, true), 0);
    assert_eq!(ke::time::get_time_snapshot().interrupt_time - before, 5_000);
    // INFINITE must stay asleep even after the finite DWORD millisecond range.
    ke::scheduler::YIELD_HOOK.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(|| {
            ke::time::advance(u64::from(u32::MAX) * 10_000 + 1);
            ke::scheduler::YIELD_HOOK.with(|slot| {
                *slot.borrow_mut() = Some(Box::new(|| panic!("infinite delay still pending")));
            });
        }));
    });
    let result = std::panic::catch_unwind(|| kernel32::sleep_ex(u32::MAX, false));
    assert!(result.is_err());
}

#[test]
fn win32_waitable_timer_uses_trusted_interval_and_normalizes_bool() {
    let _guard = state_guard();
    use kernel32::sync_ext::*;
    let null = core::ptr::null();
    let handle = unsafe { create_waitable_timer_w(null, 0, null.cast()) };
    assert!(!handle.is_null());
    let interval = -30_000i64;
    kernel32::error::set_last_error(879);
    assert_eq!(
        unsafe { set_waitable_timer(handle, &interval, 0, null.cast(), null.cast(), 0) },
        1
    );
    assert_eq!(kernel32::error::get_last_error(), 879);
    assert_eq!(
        nt_wait_for_single_object_kernel(handle, 0, Some(0)),
        STATUS_TIMEOUT
    );
    ke::time::advance(30_000);
    assert_eq!(
        nt_wait_for_single_object_kernel(handle, 0, Some(0)),
        STATUS_SUCCESS
    );
    assert_eq!(
        nt_wait_for_single_object_kernel(handle, 0, Some(0)),
        STATUS_TIMEOUT
    );
    // Win32 BOOL 256 must remain true at the native BOOLEAN boundary;
    // unsupported wake-from-suspend cannot silently arm the timer.
    assert_eq!(
        unsafe { set_waitable_timer(handle, &interval, 0, null.cast(), null.cast(), 256) },
        0
    );
    assert_eq!(kernel32::error::get_last_error(), 50);
    ke::time::advance(30_000);
    assert_eq!(
        nt_wait_for_single_object_kernel(handle, 0, Some(0)),
        STATUS_TIMEOUT
    );
    assert_eq!(
        unsafe { set_waitable_timer(handle, &interval, 0, null.cast(), null.cast(), 0) },
        1
    );
    assert_eq!(unsafe { cancel_waitable_timer(handle) }, 1);
    ke::time::advance(30_000);
    assert_eq!(
        nt_wait_for_single_object_kernel(handle, 0, Some(0)),
        STATUS_TIMEOUT
    );
    ntdll::file::free_handle(handle);
}

#[test]
fn object_flags_are_per_handle_and_protected_close_is_non_destructive() {
    let _guard = state_guard();
    let event = create_event_kernel(1, true).unwrap();
    let alias = ntdll::file::duplicate_sync_handle(event).unwrap();
    let input = [7u8, 255u8];
    unsafe {
        assert_eq!(
            ntdll::object::nt_set_information_object(event, 4, input.as_ptr() as _, 2),
            STATUS_SUCCESS
        );
        let mut output = [0xffu8; 4];
        let mut length = 0;
        assert_eq!(
            ntdll::object::nt_query_object(event, 4, output.as_mut_ptr() as _, 2, &mut length),
            STATUS_SUCCESS
        );
        assert_eq!(output, [1, 1, 255, 255]);
        assert_eq!(length, 2);
        assert_eq!(ntdll::object::query_handle_flags(alias), Ok(0));
        assert_eq!(nt_close_sync(event), STATUS_HANDLE_NOT_CLOSABLE);
        assert_eq!(
            nt_wait_for_single_object_kernel(alias, 0, Some(0)),
            STATUS_SUCCESS
        );
        assert_eq!(
            nt_wait_for_single_object_kernel(event, 0, Some(0)),
            STATUS_TIMEOUT
        );
        assert_eq!(nt_close_sync(alias), STATUS_SUCCESS);
        assert_eq!(ntdll::object::set_handle_flags(event, 2, 0), Ok(()));
        assert_eq!(ntdll::object::query_handle_flags(event), Ok(1));
        assert_eq!(nt_close_sync(event), STATUS_SUCCESS);
        assert_eq!(nt_close_sync(event), STATUS_INVALID_HANDLE);
    }
    let reused = create_event_kernel(0, false).unwrap();
    assert_eq!(ntdll::object::query_handle_flags(reused), Ok(0));
    assert!(ntdll::file::free_handle(reused));
}

#[test]
fn object_flag_capture_rejects_bad_buffers_without_mutation() {
    let _guard = state_guard();
    let event = create_event_kernel(0, false).unwrap();
    let input = [1u8, 1u8];
    unsafe {
        assert_eq!(
            ntdll::object::nt_set_information_object(event, 4, input.as_ptr() as _, 3),
            STATUS_INFO_LENGTH_MISMATCH
        );
        assert_eq!(
            ntdll::object::nt_set_information_object(event, 4, 1usize as _, 2),
            STATUS_ACCESS_VIOLATION
        );
        assert_eq!(ntdll::object::query_handle_flags(event), Ok(0));
        let mut output = [0x55u8; 4];
        let mut length = 0;
        assert_eq!(
            ntdll::object::nt_query_object(event, 4, output.as_mut_ptr() as _, 4, &mut length),
            STATUS_INFO_LENGTH_MISMATCH
        );
        assert_eq!(length, 2);
        assert_eq!(output, [0x55; 4]);
        assert_eq!(
            ntdll::object::nt_query_object(event, 4, output.as_mut_ptr() as _, 2, 1usize as _),
            STATUS_ACCESS_VIOLATION
        );
        assert_eq!(output, [0x55; 4]);
        assert_eq!(
            ntdll::object::nt_query_object(
                usize::MAX as _,
                4,
                output.as_mut_ptr() as _,
                2,
                &mut length
            ),
            STATUS_INVALID_HANDLE
        );
        assert_eq!(output, [0x55; 4]);
        assert_eq!(
            ntdll::object::nt_query_object(event, 0, output.as_mut_ptr() as _, 4, &mut length),
            STATUS_NOT_IMPLEMENTED
        );
        assert_eq!(
            ntdll::object::nt_query_object(event, 99, output.as_mut_ptr() as _, 4, &mut length),
            STATUS_INVALID_INFO_CLASS
        );
        assert_eq!(
            ntdll::object::nt_set_information_object(event, 0, input.as_ptr() as _, 2),
            STATUS_INVALID_INFO_CLASS
        );
    }
    assert!(ntdll::file::free_handle(event));
}

#[test]
fn kernel32_handle_flags_report_errors_and_preserve_last_error_on_success() {
    let _guard = state_guard();
    use kernel32::{error, handle};
    let event = create_event_kernel(0, false).unwrap();
    unsafe {
        error::set_last_error(0x1234);
        assert_eq!(handle::set_handle_information(event, 3, 3), 1);
        let mut flags = 99;
        assert_eq!(handle::get_handle_information(event, &mut flags), 1);
        assert_eq!(flags, 3);
        assert_eq!(error::get_last_error(), 0x1234);
        assert_eq!(
            handle::set_handle_information(event, 0xffff_fffd, 0xffff_fffc),
            1
        );
        assert_eq!(handle::get_handle_information(event, &mut flags), 1);
        assert_eq!(flags, 2);
        assert_eq!(handle::close_handle(event), 0);
        assert_eq!(error::get_last_error(), 6);
        assert_eq!(handle::set_handle_information(event, 2, 0), 1);
        assert_eq!(handle::close_handle(event), 1);
        flags = 0xabcd;
        assert_eq!(handle::get_handle_information(event, &mut flags), 0);
        assert_eq!(flags, 0xabcd);
        assert_eq!(error::get_last_error(), 6);
        assert_eq!(handle::set_handle_information(event, 0, 0), 0);
        assert_eq!(handle::close_handle((-1isize) as HANDLE), 0);
        assert_eq!(handle::close_handle((-2isize) as HANDLE), 0);
        assert_eq!(handle::is_valid_handle(0x12345usize as _), 0);
    }
}

#[test]
fn protected_timer_retains_its_deadline_and_handle() {
    let _guard = state_guard();
    let timer = ntdll::timer::create_timer_kernel(1).unwrap();
    assert_eq!(
        ntdll::timer::set_timer_kernel(
            timer,
            -100,
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            0,
            0
        ),
        STATUS_SUCCESS
    );
    ntdll::object::set_handle_flags(timer, 2, 2).unwrap();
    assert_eq!(unsafe { nt_close_sync(timer) }, STATUS_HANDLE_NOT_CLOSABLE);
    ke::time::advance(100);
    assert_eq!(
        nt_wait_for_single_object_kernel(timer, 0, Some(0)),
        STATUS_SUCCESS
    );
    ntdll::object::set_handle_flags(timer, 2, 0).unwrap();
    assert_eq!(unsafe { nt_close_sync(timer) }, STATUS_SUCCESS);
}

#[test]
fn concurrent_close_removes_a_handle_once() {
    let _guard = state_guard();
    let event = create_event_kernel(0, false).unwrap();
    let raw = event as usize;
    let gate = std::sync::Arc::new(std::sync::Barrier::new(3));
    let workers: Vec<_> = (0..2)
        .map(|_| {
            let gate = gate.clone();
            std::thread::spawn(move || {
                gate.wait();
                ntdll::file::close_handle(raw as _).map(|(_, retired)| retired)
            })
        })
        .collect();
    gate.wait();
    let results: Vec<_> = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect();
    assert_eq!(
        results.iter().filter(|result| **result == Ok(true)).count(),
        1
    );
    assert_eq!(
        results
            .iter()
            .filter(|result| **result == Err(STATUS_INVALID_HANDLE))
            .count(),
        1
    );
}
