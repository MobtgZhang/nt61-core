//! Production global allocator routing with owned host page extents. Tests
//! allocation ownership and rollback; QEMU covers the real page mapper.
#![allow(dead_code)]
use std::{alloc::{GlobalAlloc, Layout, System}, cell::RefCell};

#[macro_export]
macro_rules! boot_print_always { ($($t:tt)*) => { () }; }
pub mod ke {
    pub mod sync {
        pub struct Spinlock<T>(std::sync::Mutex<T>);
        unsafe impl<T> Sync for Spinlock<T> {}
        impl<T> Spinlock<T> {
            pub const fn new(t: T) -> Self { Self(std::sync::Mutex::new(t)) }
            pub fn lock(&self) -> std::sync::MutexGuard<'_, T> { self.0.lock().unwrap() }
        }
    }
    pub mod bugcheck {
        pub enum BugCheckCode { BadPoolCaller }
        pub fn bugcheck_with(_: BugCheckCode, _: u64, _: u64, _: u64, _: u64) -> ! { panic!("bad free") }
    }
}
pub mod hal { pub mod serial {
    pub fn write_string_always(_: &str) {} pub fn write_string(_: &str) {}
    pub fn write_hex_u64(_: u64) {}
} }
#[derive(Default)]
struct Backend {
    ready: bool, fail_allocate: bool, fail_map: bool,
    extents: Vec<(u64, u64, u64)>, frees: Vec<(u64, u64)>, maps: Vec<(u64, u64)>,
}
thread_local! { static BACKEND: RefCell<Backend> = RefCell::new(Backend::default()); }
#[path = "../src/mm/large_heap.rs"]
pub(crate) mod large_heap;
#[path = "../src/mm/heap.rs"]
pub mod heap;
pub mod kernel_heap_map {
    pub fn address(base: u64, bytes: u64) -> Option<u64> {
        super::BACKEND.with(|state| {
            let mut state = state.borrow_mut(); state.maps.push((base, bytes));
            if state.fail_map { return None; }
            state.extents.iter().find(|e| e.0 == base && e.1 * 4096 == bytes).map(|e| e.2)
        })
    }
}
pub mod mm {
    pub fn is_initialized() -> bool { super::BACKEND.with(|s| s.borrow().ready) }
    pub mod init_state { pub fn verify_region(_: Option<(*mut u8, usize)>, _: &str) {} }
    pub mod frame {
        use super::super::*;
        pub fn allocate_pages(pages: u64) -> Option<u64> {
            BACKEND.with(|state| {
                let mut state = state.borrow_mut();
                if state.fail_allocate { return None; }
                let layout = Layout::from_size_align(pages as usize * 4096, 4096).unwrap();
                // SAFETY: this fixture owns and tracks the complete extent.
                let base = unsafe { System.alloc(layout) } as u64;
                if base == 0 { return None; }
                let physical = 0x1_0000_0000 + (state.frees.len() as u64 + state.extents.len() as u64) * (64 << 20);
                state.extents.push((physical, pages, base)); Some(physical)
            })
        }
        pub fn free_pages(base: u64, pages: u64) {
            BACKEND.with(|state| {
                let mut state = state.borrow_mut();
                let index = state.extents.iter().position(|e| e.0 == base && e.1 == pages).expect("owned exact extent");
                let (_, _, pointer) = state.extents.remove(index); state.frees.push((base, pages));
                // SAFETY: removing the tracked extent transfers it back once.
                unsafe { System.dealloc(pointer as *mut u8, Layout::from_size_align(pages as usize * 4096, 4096).unwrap()); }
            })
        }
    }
    pub(crate) use crate::large_heap;
    pub use crate::heap;
}

fn reset() { BACKEND.with(|s| { assert!(s.borrow().extents.is_empty()); *s.borrow_mut() = Backend::default(); }); }

#[test]
fn before_mm_and_small_allocations_never_call_the_page_backend() {
    reset();
    unsafe {
        assert!(mm::large_heap::allocate(Layout::from_size_align(2 << 20, 8).unwrap()).is_null());
        BACKEND.with(|s| s.borrow_mut().ready = true);
        assert!(mm::large_heap::allocate(Layout::from_size_align(4096, 8).unwrap()).is_null());
    }
    BACKEND.with(|s| assert!(s.borrow().maps.is_empty()));
}

#[test]
fn aligned_buffers_keep_their_data_and_release_exact_buddy_extents() {
    reset(); BACKEND.with(|s| s.borrow_mut().ready = true);
    for alignment in [1, 8, 4096, 2 << 20] {
        let layout = Layout::from_size_align(6_600_003, alignment).unwrap();
        unsafe {
            let pointer = mm::large_heap::allocate(layout);
            assert!(!pointer.is_null()); assert_eq!(pointer as usize % alignment, 0);
            pointer.write(0x51); pointer.add(layout.size() - 1).write(0xa7);
            assert_eq!(pointer.read(), 0x51); assert_eq!(pointer.add(layout.size() - 1).read(), 0xa7);
            BACKEND.with(|s| {
                let s = s.borrow(); let (base, pages, virtual_base) = s.extents[0];
                assert_ne!(base, virtual_base);
                assert!(pages.is_power_of_two()); assert_eq!(s.maps.last(), Some(&(base, pages * 4096)));
                assert!(pointer as u64 >= virtual_base && pointer as u64 + layout.size() as u64 <= virtual_base + pages * 4096);
            });
            mm::large_heap::release(pointer, layout);
        }
        BACKEND.with(|s| assert!(s.borrow().extents.is_empty()));
    }
    BACKEND.with(|s| assert_eq!(s.borrow().frees.len(), 4));
}

#[test]
fn mapping_failure_rolls_back_every_reserved_page() {
    reset(); BACKEND.with(|s| { let mut s = s.borrow_mut(); s.ready = true; s.fail_map = true; });
    unsafe { assert!(mm::large_heap::allocate(Layout::from_size_align(6 << 20, 16).unwrap()).is_null()); }
    BACKEND.with(|s| { let s = s.borrow(); assert!(s.extents.is_empty()); assert_eq!(s.frees.len(), 1); });
}

#[test]
fn page_exhaustion_does_not_attempt_mapping_or_free_foreign_pages() {
    reset(); BACKEND.with(|s| { let mut s = s.borrow_mut(); s.ready = true; s.fail_allocate = true; });
    unsafe { assert!(mm::large_heap::allocate(Layout::from_size_align(1 << 20, 8).unwrap()).is_null()); }
    BACKEND.with(|s| { let s = s.borrow(); assert!(s.maps.is_empty() && s.frees.is_empty()); });
}

#[test]
fn global_wrapper_routes_and_releases_bootstrap_and_page_allocations() {
    reset();
    let mut storage = vec![0u64; 512 * 1024];
    mm::heap::KERNEL_HEAP.configure(storage.as_mut_ptr() as *mut u8, storage.len() * 8);
    let allocator = mm::heap::KernelHeapAllocator;
    let layout = Layout::from_size_align(1 << 20, 16).unwrap();
    unsafe {
        let early = allocator.alloc(layout); assert!(!early.is_null());
        assert!(early as usize >= storage.as_ptr() as usize && (early as usize) < storage.as_ptr() as usize + storage.len() * 8);
        BACKEND.with(|s| s.borrow_mut().ready = true);
        let pages = allocator.alloc(layout); assert!(!pages.is_null());
        BACKEND.with(|s| assert_eq!(s.borrow().extents.len(), 1));
        allocator.dealloc(early, layout); allocator.dealloc(pages, layout);
        BACKEND.with(|s| s.borrow_mut().fail_allocate = true);
        let fallback = allocator.alloc(layout); assert!(!fallback.is_null()); allocator.dealloc(fallback, layout);
    }
    BACKEND.with(|s| assert!(s.borrow().extents.is_empty()));
}
