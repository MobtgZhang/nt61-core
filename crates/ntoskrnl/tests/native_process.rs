//! Host execution of the actual native process adapter and handle storage.
//! Only the process registry, current-thread pointer and user-copy boundary
//! are supplied by this fixture; no query/open implementation is duplicated.
#![allow(dead_code, non_camel_case_types)]
extern crate alloc;
#[path = "native_process/kernel32.rs"]
pub mod kernel32;
#[path = "native_process/ntdll.rs"]
pub mod ntdll;
pub mod libs {
    pub use crate::ntdll;
}
pub mod ke {
    pub mod time {
        pub struct TimeSnapshot { pub system_time: u64, pub interrupt_time: u64 }
        pub fn get_time_snapshot() -> TimeSnapshot { TimeSnapshot { system_time: 0, interrupt_time: 0 } }
    }
    pub mod sync {
        pub struct Spinlock<T>(std::sync::Mutex<T>);
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
    pub mod scheduler {
        pub fn with_current_thread(f: impl FnOnce(&mut crate::ps::thread::Ethread)) {
            let pointer = crate::ps::thread::get_current_ethread();
            if !pointer.is_null() {
                f(unsafe { &mut *pointer });
            }
        }
    }
}
pub mod ps {
    pub mod job {
        pub fn close_native_job(_: u64) {
            panic!("process-only fixture closed a job");
        }
    }
    pub mod process {
        use std::sync::atomic::{AtomicPtr, AtomicU32};
        pub const PID_SYSTEM: u64 = 4;
        pub struct Affinity {
            pub mask: [u64; 1],
        }
        pub struct HandleTable {
            pub slots: [u64; 8],
            pub valid: [bool; 8],
            pub next_slot: usize,
            pub handle_count: AtomicU32,
        }
        pub struct Eprocess {
            pub unique_process_id: u64,
            pub exit_status: i32,
            pub peb: *mut (),
            pub affinity: Affinity,
            pub base_priority: i8,
            pub spare5: u64,
            pub wow64_flag: bool,
            pub wow64_peb32_va: u32,
            pub object_table: *mut HandleTable,
        }
        impl Eprocess {
            pub fn is_wow64_process(&self) -> bool {
                self.wow64_flag
            }
            pub fn get_wow64_peb32_va(&self) -> u32 {
                self.wow64_peb32_va
            }
        }
        pub static TARGET: AtomicPtr<Eprocess> = AtomicPtr::new(core::ptr::null_mut());
        pub fn get_by_pid(pid: u64) -> Option<&'static mut Eprocess> {
            let pointer = TARGET.load(std::sync::atomic::Ordering::SeqCst);
            unsafe { pointer.as_mut() }.filter(|process| process.unique_process_id == pid)
        }
        pub fn next_user_pid() -> Option<u64> {
            None
        }
        pub fn create_user_process_from_path(
            _: &str,
            _: u64,
            _: Option<u64>,
        ) -> Option<&'static mut Eprocess> {
            None
        }
    }
    pub mod thread {
        use std::sync::atomic::{AtomicPtr, Ordering};
        pub enum KThreadState {
            Running,
            Terminated,
        }
        pub struct Kthread {
            pub process: *mut super::process::Eprocess,
            pub state: KThreadState,
        }
        pub struct Ethread {
            pub kthread: Kthread,
        }
        pub static CURRENT: AtomicPtr<Ethread> = AtomicPtr::new(core::ptr::null_mut());
        pub fn get_current_ethread() -> *mut Ethread {
            CURRENT.load(Ordering::SeqCst)
        }
    }
}
pub mod mm {
    pub mod pool {
        pub enum PoolType {
            NonPaged,
        }
        pub fn allocate(_: PoolType, _: usize) -> *mut u8 {
            core::ptr::null_mut()
        }
        pub fn free(_: *mut u8) {}
    }
    pub mod user_copy {
        use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
        pub static FAIL_WRITE: AtomicU64 = AtomicU64::new(0);
        pub static REJECT_USER_ACCESS: AtomicBool = AtomicBool::new(false);
        pub fn probe_user_write(address: u64, _: usize) -> Result<(), i32> {
            if address < 0x1000 || REJECT_USER_ACCESS.load(Ordering::SeqCst) {
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
            if FAIL_WRITE.load(Ordering::SeqCst) == address {
                return Err(0xc0000005u32 as i32);
            }
            unsafe { (address as *mut T).write_unaligned(*value) };
            Ok(())
        }
        pub fn copy_from_user(bytes: &mut [u8], source: u64) -> Result<(), i32> {
            probe_user_write(source, bytes.len())?;
            unsafe {
                core::ptr::copy_nonoverlapping(source as *const u8, bytes.as_mut_ptr(), bytes.len())
            };
            Ok(())
        }
        pub fn copy_to_user(target: u64, bytes: &[u8]) -> Result<(), i32> {
            probe_user_write(target, bytes.len())?;
            unsafe {
                core::ptr::copy_nonoverlapping(bytes.as_ptr(), target as *mut u8, bytes.len())
            };
            Ok(())
        }
    }
}

use ntdll::{file::*, process::*, status::*, types::*};
use std::sync::atomic::Ordering;

#[test]
fn process_query_uses_64_bit_layout_real_targets_and_safe_outputs() {
    assert_eq!(core::mem::size_of::<ProcessBasicInformation>(), 48);
    let mut process = ps::process::Eprocess {
        unique_process_id: 731,
        exit_status: 0x103,
        peb: 0x7fffff000000usize as *mut (),
        affinity: ps::process::Affinity { mask: [0xa5] },
        base_priority: -3,
        spare5: 417,
        wow64_flag: true,
        wow64_peb32_va: 0x7efde000,
        object_table: core::ptr::null_mut(),
    };
    ps::process::TARGET.store(&mut process, Ordering::SeqCst);
    let mut thread = ps::thread::Ethread {
        kthread: ps::thread::Kthread {
            process: &mut process,
            state: ps::thread::KThreadState::Running,
        },
    };
    ps::thread::CURRENT.store(&mut thread, Ordering::SeqCst);
    let handle = alloc_handle(HandleKind::Process, 731);
    let wrong_kind = alloc_handle(HandleKind::Event, 731);
    let missing_pid = alloc_handle(HandleKind::Process, 999);
    let mut output = [0xaau8; 64];
    let mut length = 0;
    unsafe {
        for target in [handle, nt_current_process()] {
            assert_eq!(
                nt_query_information_process(
                    target,
                    0,
                    output.as_mut_ptr().cast(),
                    48,
                    &mut length
                ),
                STATUS_SUCCESS
            );
            assert_eq!(length, 48);
            assert_eq!(&output[0..4], &0x103i32.to_le_bytes());
            assert_eq!(&output[4..8], &[0; 4]);
            assert_eq!(&output[8..16], &0x7fffff000000u64.to_le_bytes());
            assert_eq!(&output[16..24], &0xa5u64.to_le_bytes());
            assert_eq!(&output[24..28], &(-3i32).to_le_bytes());
            assert_eq!(&output[28..32], &[0; 4]);
            assert_eq!(&output[32..40], &731u64.to_le_bytes());
            assert_eq!(&output[40..48], &417u64.to_le_bytes());
            assert_eq!(&output[48..], &[0xaa; 16]);
        }
        for size in [0, 24, 47, 49, 64] {
            output.fill(0xaa);
            assert_eq!(
                nt_query_information_process(
                    handle,
                    0,
                    output.as_mut_ptr().cast(),
                    size,
                    &mut length
                ),
                STATUS_INFO_LENGTH_MISMATCH
            );
            assert_eq!(length, 48);
            assert_eq!(output, [0xaa; 64]);
        }
        for (target, status) in [
            (core::ptr::null_mut(), STATUS_INVALID_HANDLE),
            (731usize as HANDLE, STATUS_INVALID_HANDLE),
            (wrong_kind, STATUS_OBJECT_TYPE_MISMATCH),
            (missing_pid, STATUS_INVALID_HANDLE),
        ] {
            assert_eq!(
                nt_query_information_process(
                    target,
                    0,
                    output.as_mut_ptr().cast(),
                    48,
                    &mut length
                ),
                status
            );
        }
        assert_eq!(
            nt_query_information_process(handle, 0, 1usize as PVOID, 48, &mut length),
            STATUS_ACCESS_VIOLATION
        );
        assert_eq!(
            nt_query_information_process(
                handle,
                0,
                output.as_mut_ptr().cast(),
                48,
                1usize as *mut u32
            ),
            STATUS_ACCESS_VIOLATION
        );
        assert_eq!(
            nt_query_information_process(handle, 26, output.as_mut_ptr().cast(), 8, &mut length),
            STATUS_SUCCESS
        );
        assert_eq!(&output[..8], &0x7efde000u64.to_le_bytes());
        assert_eq!(length, 8);
        (*ps::process::TARGET.load(Ordering::SeqCst)).wow64_flag = false;
        assert_eq!(
            nt_query_information_process(handle, 26, output.as_mut_ptr().cast(), 8, &mut length),
            STATUS_SUCCESS
        );
        assert_eq!(&output[..8], &[0; 8]);
        for size in [0, 16, 17, 64] {
            output.fill(0xaa);
            assert_eq!(
                nt_query_information_process(
                    handle,
                    27,
                    output.as_mut_ptr().cast(),
                    size,
                    &mut length
                ),
                STATUS_NOT_IMPLEMENTED
            );
            assert_eq!(output, [0xaa; 64]);
        }
        assert_eq!(
            nt_query_information_process(handle, 999, output.as_mut_ptr().cast(), 64, &mut length),
            STATUS_INVALID_INFO_CLASS
        );
        let before = handle_count();
        let mut opened = core::ptr::null_mut();
        let mut cid = client_id {
            unique_process: 731usize as HANDLE,
            unique_thread: core::ptr::null_mut(),
        };
        assert_eq!(
            nt_open_process(1usize as *mut HANDLE, 0, core::ptr::null_mut(), &mut cid),
            STATUS_ACCESS_VIOLATION
        );
        assert_eq!(handle_count(), before);
        mm::user_copy::FAIL_WRITE.store((&mut opened as *mut HANDLE) as u64, Ordering::SeqCst);
        assert_eq!(
            nt_open_process(&mut opened, 0, core::ptr::null_mut(), &mut cid),
            STATUS_ACCESS_VIOLATION
        );
        assert_eq!(
            handle_count(),
            before,
            "failed output copy must release the allocated handle"
        );
        mm::user_copy::FAIL_WRITE.store(0, Ordering::SeqCst);
        assert_eq!(
            nt_open_process(&mut opened, 0, core::ptr::null_mut(), &mut cid),
            STATUS_SUCCESS
        );
        assert_eq!(lookup_handle(opened).unwrap().target, 731);
        free_handle(opened);
        assert_eq!(
            nt_open_process(
                &mut opened,
                0,
                core::ptr::null_mut(),
                1usize as *mut client_id
            ),
            STATUS_ACCESS_VIOLATION
        );
        cid.unique_process = 999usize as HANDLE;
        assert_eq!(
            nt_open_process(&mut opened, 0, core::ptr::null_mut(), &mut cid),
            0xc000000bu32 as i32
        );
        // Kernel32 receives a PID and returns a handle by value. Reject every
        // user-memory operation to prove this trusted path does not probe its
        // own stack while the native user entry still enforces that boundary.
        mm::user_copy::REJECT_USER_ACCESS.store(true, Ordering::SeqCst);
        cid.unique_process = 731usize as HANDLE;
        assert_eq!(
            nt_open_process(&mut opened, 0, core::ptr::null_mut(), &mut cid),
            STATUS_ACCESS_VIOLATION
        );
        let trusted = kernel32::process::open_process(0, 0, 731);
        assert!(!trusted.is_null());
        assert_eq!(lookup_handle(trusted).unwrap().target, 731);
        free_handle(trusted);
        assert!(kernel32::process::open_process(0, 0, 999).is_null());
        assert_eq!(kernel32::error::get_last_error(), 87);
        mm::user_copy::REJECT_USER_ACCESS.store(false, Ordering::SeqCst);
    }
    for handle in [handle, wrong_kind, missing_pid] {
        free_handle(handle);
    }
    ps::thread::CURRENT.store(core::ptr::null_mut(), Ordering::SeqCst);
    ps::process::TARGET.store(core::ptr::null_mut(), Ordering::SeqCst);
}
