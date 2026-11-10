//! Reclaimable static heap for the freestanding desktop processes.

use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;
use core::mem::MaybeUninit;
use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicPtr, Ordering};

pub type AllocFailHook =
    unsafe fn(requested: usize, align: usize, free_blocks: usize, largest_free: usize);

pub type HeapTraceHook = unsafe fn(event: HeapTrace);

pub enum HeapTrace {
    Alloc {
        size: usize,
        align: usize,
        pointer: *mut u8,
    },
    Free {
        size: usize,
        align: usize,
        pointer: *mut u8,
    },
}

const TRACE_THRESHOLD: usize = 64 * 1024;

static ALLOC_TRACE_HOOK: AtomicPtr<()> = AtomicPtr::new(ptr::null_mut());
static ALLOC_FAIL_HOOK: AtomicPtr<()> = AtomicPtr::new(ptr::null_mut());

pub fn set_heap_trace_hook(hook: Option<HeapTraceHook>) {
    let raw = hook.map(|hook| hook as *mut ()).unwrap_or(ptr::null_mut());
    ALLOC_TRACE_HOOK.store(raw, Ordering::Release);
}

pub fn set_alloc_fail_hook(hook: Option<AllocFailHook>) {
    let raw = hook.map(|hook| hook as *mut ()).unwrap_or(ptr::null_mut());
    ALLOC_FAIL_HOOK.store(raw, Ordering::Release);
}

const HEADER_SIZE: usize = core::mem::size_of::<FreeBlock>();
const BLOCK_ALIGNMENT: usize = 16;
const BLOCK_MAGIC: u32 = 0x4e545531;

#[repr(C)]
struct FreeBlock {
    magic: u32,
    block: usize,
    size: usize,
    next: *mut FreeBlock,
}

pub struct StaticHeapAllocator<const SIZE: usize> {
    heap: UnsafeCell<MaybeUninit<[u8; SIZE]>>,
    locked: AtomicBool,
    free_head: AtomicPtr<FreeBlock>,
}

unsafe impl<const SIZE: usize> Sync for StaticHeapAllocator<SIZE> {}

impl<const SIZE: usize> StaticHeapAllocator<SIZE> {
    pub const fn new() -> Self {
        Self {
            heap: UnsafeCell::new(MaybeUninit::uninit()),
            locked: AtomicBool::new(false),
            free_head: AtomicPtr::new(ptr::null_mut()),
        }
    }

    fn lock(&self) {
        while self
            .locked
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
    }

    fn unlock(&self) {
        self.locked.store(false, Ordering::Release);
    }

    fn heap_range(&self) -> (*mut u8, usize) {
        (self.heap.get() as *mut u8, SIZE)
    }

    fn initialize(&self) -> *mut FreeBlock {
        let (base, size) = self.heap_range();
        let aligned_first = (base as usize).next_multiple_of(BLOCK_ALIGNMENT);
        if aligned_first >= base as usize + size {
            return ptr::null_mut();
        }
        let block = aligned_first as *mut FreeBlock;
        unsafe {
            ptr::write(
                block,
                FreeBlock {
                    magic: BLOCK_MAGIC,
                    block: 0,
                    size: base as usize + size - aligned_first,
                    next: ptr::null_mut(),
                },
            );
        }
        block
    }

    fn free_stats(&self) -> (usize, usize) {
        let mut count = 0;
        let mut largest = 0;
        let mut current = self.free_head.load(Ordering::Acquire);
        while !current.is_null() {
            count += 1;
            let size = unsafe { (*current).size };
            if size > largest {
                largest = size;
            }
            current = unsafe { (*current).next };
        }
        (count, largest)
    }
}

#[cfg(test)]
mod tests {
    use super::StaticHeapAllocator;
    use core::alloc::{GlobalAlloc, Layout};

    #[test]
    fn high_alignment_allocations_are_reclaimable() {
        static HEAP: StaticHeapAllocator<{ 128 * 1024 }> = StaticHeapAllocator::new();

        unsafe {
            let first = HEAP.alloc(Layout::from_size_align(64, 4096).unwrap());
            assert!(!first.is_null());
            assert_eq!(first as usize % 4096, 0);
            HEAP.dealloc(first, Layout::from_size_align(64, 4096).unwrap());

            let second = HEAP.alloc(Layout::from_size_align(64, 4096).unwrap());
            assert_eq!(second, first);
        }
    }
}

unsafe impl<const SIZE: usize> GlobalAlloc for StaticHeapAllocator<SIZE> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if layout.size() == 0 {
            return ptr::null_mut();
        }

        self.lock();
        if self.free_head.load(Ordering::Acquire).is_null() {
            let first = self.initialize();
            self.free_head.store(first, Ordering::Release);
        }

        let mut previous: *mut FreeBlock = ptr::null_mut();
        let mut current = self.free_head.load(Ordering::Acquire);
        while !current.is_null() {
            let block_start = current as usize;
            let aligned_pointer =
                (block_start + HEADER_SIZE).next_multiple_of(layout.align().max(BLOCK_ALIGNMENT));
            let requested =
                (aligned_pointer - block_start) + layout.size().next_multiple_of(BLOCK_ALIGNMENT);

            if requested <= unsafe { (*current).size } {
                let remaining = unsafe { (*current).size } - requested;
                if remaining >= HEADER_SIZE + BLOCK_ALIGNMENT {
                    let split = (block_start + requested) as *mut FreeBlock;
                    unsafe {
                        let next = (*current).next;
                        ptr::write(
                            split,
                            FreeBlock {
                                magic: BLOCK_MAGIC,
                                block: 0,
                                size: remaining,
                                next,
                            },
                        );
                        (*current).size = requested;
                        (*current).next = split;
                    }
                }

                let next = unsafe { (*current).next };
                if previous.is_null() {
                    self.free_head.store(next, Ordering::Release);
                } else {
                    unsafe { (*previous).next = next };
                }
                let pointer = aligned_pointer as *mut u8;
                unsafe {
                    ptr::write(
                        pointer.sub(HEADER_SIZE) as *mut FreeBlock,
                        FreeBlock {
                            magic: BLOCK_MAGIC,
                            block: block_start,
                            size: requested,
                            next: ptr::null_mut(),
                        },
                    );
                }
                self.unlock();
                if layout.size() >= TRACE_THRESHOLD {
                    let hook = ALLOC_TRACE_HOOK.load(Ordering::Acquire);
                    if !hook.is_null() {
                        let hook: HeapTraceHook = unsafe { core::mem::transmute(hook) };
                        unsafe {
                            hook(HeapTrace::Alloc {
                                size: layout.size(),
                                align: layout.align(),
                                pointer,
                            })
                        };
                    }
                }
                return pointer;
            }

            previous = current;
            current = unsafe { (*current).next };
        }

        let (free_blocks, largest_free) = self.free_stats();
        self.unlock();
        let hook = ALLOC_FAIL_HOOK.load(Ordering::Acquire);
        if !hook.is_null() {
            let hook: AllocFailHook = unsafe { core::mem::transmute(hook) };
            unsafe { hook(layout.size(), layout.align(), free_blocks, largest_free) };
        }
        ptr::null_mut()
    }

    unsafe fn dealloc(&self, pointer: *mut u8, _layout: Layout) {
        if pointer.is_null() {
            return;
        }

        let (base, size) = self.heap_range();
        let pointer_address = pointer as usize;
        let base_address = base as usize;
        if pointer_address < base_address + HEADER_SIZE || pointer_address > base_address + size {
            return;
        }

        let header = (pointer_address - HEADER_SIZE) as *mut FreeBlock;
        self.lock();
        if unsafe { (*header).magic } != BLOCK_MAGIC {
            self.unlock();
            return;
        }

        let block = unsafe { (*header).block } as *mut FreeBlock;
        unsafe {
            ptr::write(
                block,
                FreeBlock {
                    magic: BLOCK_MAGIC,
                    block: 0,
                    size: (*header).size,
                    next: ptr::null_mut(),
                },
            );
        }

        if _layout.size() >= TRACE_THRESHOLD {
            let hook = ALLOC_TRACE_HOOK.load(Ordering::Acquire);
            if !hook.is_null() {
                let hook: HeapTraceHook = unsafe { core::mem::transmute(hook) };
                unsafe {
                    hook(HeapTrace::Free {
                        size: _layout.size(),
                        align: _layout.align(),
                        pointer,
                    })
                };
            }
        }

        let mut previous: *mut FreeBlock = ptr::null_mut();
        let mut current = self.free_head.load(Ordering::Acquire);
        while !current.is_null() {
            if current > block {
                break;
            }
            previous = current;
            current = unsafe { (*current).next };
        }

        unsafe {
            (*block).next = current;
        }
        if previous.is_null() {
            self.free_head.store(block, Ordering::Release);
        } else {
            unsafe {
                (*previous).next = block;
            }
        }

        if !current.is_null() && (block as usize + unsafe { (*block).size }) == current as usize {
            unsafe {
                (*block).size += (*current).size;
                (*block).next = (*current).next;
            }
        }
        if !previous.is_null()
            && (previous as usize + unsafe { (*previous).size }) == block as usize
        {
            unsafe {
                (*previous).size += (*block).size;
                (*previous).next = (*block).next;
            }
        }
        self.unlock();
    }
}
