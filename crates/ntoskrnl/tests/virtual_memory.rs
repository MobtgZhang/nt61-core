//! Exercise production virtual-memory entry points with guarded host copies.
//! No host mapping backend is provided: these tests cover capture and outputs,
//! not physical allocation or Windows virtual-memory parity.
#![allow(dead_code, non_camel_case_types)]
extern crate alloc;

pub mod ke {
    pub mod sync {
        pub struct Spinlock<T>(std::sync::Mutex<T>);
        unsafe impl<T> Sync for Spinlock<T> {}
        impl<T> Spinlock<T> {
            pub const fn new(value: T) -> Self { Self(std::sync::Mutex::new(value)) }
            pub fn lock(&self) -> std::sync::MutexGuard<'_, T> { self.0.lock().unwrap() }
        }
    }
}
pub mod mm {
    pub mod vas {
        pub fn allocate_user_va(_: u64, _: u64, _: u32) -> Option<u64> { None }
        pub fn free_user_va(_: u64) -> bool { false }
    }
    pub mod dynamic_paging {
        pub fn get_cr3() -> u64 { panic!("unexpected address-space switch") }
        pub fn set_cr3(_: u64) { panic!("unexpected address-space switch") }
    }
    pub mod user_copy {
        const ACCESS_VIOLATION: i32 = 0xc0000005u32 as i32;
        pub fn probe_user_write(address: u64, size: usize) -> Result<(), i32> {
            if size != 0 && address < 0x10000 { Err(ACCESS_VIOLATION) } else { Ok(()) }
        }
        pub fn copy_from_user(output: &mut [u8], address: u64) -> Result<(), i32> {
            probe_user_write(address, output.len())?;
            // SAFETY: callers only pass real host allocations above the guarded
            // low range; the fixture intentionally rejects addresses 0 and 1.
            unsafe { std::ptr::copy_nonoverlapping(address as *const u8, output.as_mut_ptr(), output.len()); }
            Ok(())
        }
        pub fn copy_to_user(address: u64, input: &[u8]) -> Result<(), i32> {
            probe_user_write(address, input.len())?;
            // SAFETY: same fixture allocation contract as copy_from_user.
            unsafe { std::ptr::copy_nonoverlapping(input.as_ptr(), address as *mut u8, input.len()); }
            Ok(())
        }
        pub fn read_user_object<T: Copy>(address: u64) -> Result<T, i32> {
            probe_user_write(address, std::mem::size_of::<T>())?;
            // SAFETY: fixture passes live host allocations after the guard.
            Ok(unsafe { std::ptr::read_unaligned(address as *const T) })
        }
        pub fn write_user_object<T>(address: u64, value: &T) -> Result<(), i32> {
            probe_user_write(address, std::mem::size_of::<T>())?;
            // SAFETY: fixture passes live host allocations after the guard.
            unsafe { std::ptr::copy_nonoverlapping(value as *const T as *const u8,
                address as *mut u8, std::mem::size_of::<T>()); }
            Ok(())
        }
    }
}
pub mod ps {
    pub mod process {
        pub struct Process { pub pml4_phys: u64 }
        pub fn get_by_pid(_: u64) -> Option<Process> { None }
    }
}
#[path = "../../libs/ntdll/src/ntdll/status.rs"]
pub mod status;
#[path = "../../libs/ntdll/src/ntdll/types.rs"]
pub mod types;
pub mod file {
    use super::types::HANDLE;
    #[derive(PartialEq)]
    pub enum HandleKind { Process }
    pub struct Entry { pub kind: HandleKind, pub target: u64 }
    pub fn lookup_handle(_: HANDLE) -> Option<Entry> { None }
}
pub mod string {
    pub unsafe fn alloc_string(_: u32) -> *mut u8 { std::ptr::null_mut() }
}
#[path = "../../libs/ntdll/src/ntdll/virtual_mem.rs"]
pub mod virtual_mem;

use status::*;
use types::{PVOID, HANDLE};

#[test]
fn free_captures_base_before_dereferencing() {
    let mut size = 0usize;
    assert_eq!(unsafe { virtual_mem::nt_free_virtual_memory(
        -1isize as HANDLE, 1usize as *mut PVOID, &mut size, 0x8000) }, STATUS_ACCESS_VIOLATION);
}

#[test]
fn protect_captures_base_before_dereferencing() {
    let mut size = 0x1000usize;
    let mut old = 0;
    assert_eq!(unsafe { virtual_mem::nt_protect_virtual_memory(
        -1isize as HANDLE, 1usize as *mut PVOID, &mut size, 4, &mut old) }, STATUS_ACCESS_VIOLATION);
}

#[test]
fn invalid_capture_leaves_other_outputs_unchanged() {
    let mut base = 0x100000usize as PVOID;
    let mut old = 0xabcdef01;
    assert_eq!(unsafe { virtual_mem::nt_protect_virtual_memory(
        -1isize as HANDLE, &mut base, 1usize as *mut usize, 4, &mut old) }, STATUS_ACCESS_VIOLATION);
    assert_eq!(base as usize, 0x100000);
    assert_eq!(old, 0xabcdef01);
}

#[test]
fn trusted_query_retains_production_output() {
    let mut information = virtual_mem::MemoryBasicInformation::default();
    let mut length = 0;
    let status = unsafe { virtual_mem::nt_query_virtual_memory_kernel(
        -1isize as HANDLE, 0x123456usize as PVOID, 0, &mut information,
        std::mem::size_of::<virtual_mem::MemoryBasicInformation>(), &mut length) };
    assert_eq!(status, STATUS_SUCCESS);
    assert_eq!(information.base_address as usize, 0x123000);
    assert_eq!(information.region_size, 0x1000);
    assert_eq!(length, std::mem::size_of_val(&information));
}
