//! Large global allocations backed by owned physical pages after MM startup.
//! Bootstrap allocations and pool free lists continue to use the small heap.
use core::{alloc::Layout, mem, ptr};

const PAGE: usize = 4096;
pub(super) const THRESHOLD: usize = 1024 * 1024;
const MAGIC: usize = 0x4e54_3631_5041_4745;

#[repr(C)]
struct Header {
    magic: usize,
    base: u64,
    pages: u64,
    size: usize,
    alignment: usize,
}

fn pages_for(layout: Layout) -> Option<u64> {
    let bytes = layout.size().checked_add(mem::size_of::<Header>())?
        .checked_add(layout.align().max(mem::align_of::<Header>()) - 1)?;
    let pages = bytes.checked_add(PAGE - 1)? / PAGE;
    // Pass the exact power-of-two extent to both buddy and PFN accounting.
    // free_pages must release every page reserved by allocate_pages.
    u64::try_from(pages.checked_next_power_of_two()?).ok()
}

/// Null means no pages were retained. The caller can try its bootstrap heap.
pub(super) unsafe fn allocate(layout: Layout) -> *mut u8 {
    if layout.size() < THRESHOLD || !crate::mm::is_initialized() {
        return ptr::null_mut();
    }
    let Some(pages) = pages_for(layout) else { return ptr::null_mut(); };
    let Some(length) = pages.checked_mul(PAGE as u64) else { return ptr::null_mut(); };
    let Some(base) = crate::mm::frame::allocate_pages(pages) else { return ptr::null_mut(); };
    let Some(virtual_base) = super::kernel_heap_map::address(base, length) else {
        crate::mm::frame::free_pages(base, pages);
        return ptr::null_mut();
    };
    let address = usize::try_from(virtual_base).ok().and_then(|base| {
        base.checked_add(mem::size_of::<Header>())?
            .checked_add(layout.align().max(mem::align_of::<Header>()) - 1)
            .map(|p| p & !(layout.align().max(mem::align_of::<Header>()) - 1))
    });
    let valid = address.and_then(|p| p.checked_add(layout.size()))
        .zip(virtual_base.checked_add(length)).is_some_and(|(end, limit)| end as u64 <= limit);
    if !valid {
        crate::mm::frame::free_pages(base, pages);
        return ptr::null_mut();
    }
    let address = address.unwrap();
    // SAFETY: the owned extent is mapped writable above; checked arithmetic
    // places this aligned header and the complete payload inside that extent.
    unsafe { ptr::write((address - mem::size_of::<Header>()) as *mut Header, Header {
        magic: MAGIC, base, pages, size: layout.size(), alignment: layout.align(),
    }); }
    address as *mut u8
}

/// Caller passes a live allocation from allocate, with its original layout.
pub(super) unsafe fn release(pointer: *mut u8, layout: Layout) {
    // SAFETY: the global allocator identifies bootstrap pointers first; all
    // remaining live pointers have this private header immediately before them.
    let header = unsafe { ptr::read(pointer.sub(mem::size_of::<Header>()) as *const Header) };
    assert!(header.magic == MAGIC && header.size == layout.size()
        && header.alignment == layout.align() && Some(header.pages) == pages_for(layout),
        "invalid large kernel allocation");
    // Clear ownership metadata before returning the complete extent to MM.
    unsafe { ptr::write_bytes(pointer.sub(mem::size_of::<Header>()), 0, mem::size_of::<Header>()); }
    crate::mm::frame::free_pages(header.base, header.pages);
}
