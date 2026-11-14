//! Shared BTL guest-memory access.
//!
//! Provides a page-chunked copy path that every non-x86 BTL backend
//! re-uses. The host architecture supplies a [`GuestPaging`] impl
//! wrapping its own page-table walk; the shared layer handles the
//! validation loop, per-page chunking, and copy direction.

/// Page-table access required to translate guest virtual addresses.
pub trait GuestPaging {
    /// Validate that every page in `va..va+len` is mapped and has the
    /// access rights required by the copy direction. `write` selects
    /// the required permission.
    fn validate(&self, va: u64, len: usize, write: bool) -> bool;

    /// Translate `va` to a host-accessible pointer (or physical
    /// address on identity-mapped kernels). Returns `None` when the
    /// page is not mapped.
    fn translate(&self, va: u64) -> Option<u64>;

    /// Copy one page chunk while its host mapping remains alive.
    /// `buf` points to a valid kernel buffer of `len` bytes.
    unsafe fn copy_page(&self, va: u64, buf: *mut u8, len: usize, from_guest: bool) -> bool {
        let Some(host) = self.translate(va) else { return false; };
        if from_guest { core::ptr::copy(host as *const u8, buf, len); }
        else { core::ptr::copy(buf, host as *mut u8, len); }
        true
    }
}

/// Guest memory access surface used by the BTL syscall bridge and
/// interpreter. Object-safe so the shared layer can take `&mut dyn`.
pub trait GuestMemory {
    /// Copy bytes from guest VA into `buf`; returns bytes copied.
    fn read_virt(&mut self, va: u64, buf: &mut [u8]) -> usize;
    /// Copy bytes into guest VA; returns `true` when all copied.
    fn write_virt(&mut self, va: u64, data: &[u8]) -> bool;
    /// Validate writable output before a service performs host IO.
    fn probe_write(&mut self, _va: u64, _len: usize) -> bool { false }
    /// Instruction fetch must enforce guest execute permissions. Data-only
    /// adapters fail closed until they explicitly implement executable memory.
    fn fetch_virt(&mut self, _va: u64, _buf: &mut [u8]) -> usize {
        0
    }
}

/// Null guest-memory view. It fails closed, making it suitable for
/// register-only ABIs and as a safe default in bridges that do not
/// yet have an active guest address space.
pub struct NullGuestMemory;

impl GuestMemory for NullGuestMemory {
    fn read_virt(&mut self, _va: u64, _buf: &mut [u8]) -> usize {
        0
    }

    fn write_virt(&mut self, _va: u64, _data: &[u8]) -> bool {
        false
    }
}

/// Adapter for architectures whose `arch` layer exposes validated
/// leaf PTEs and a VA translation. Permission checking uses the same
/// interpretation as the native page-fault path, while copying is
/// shared with every other BTL backend.
#[derive(Clone, Copy, Debug, Default)]
pub struct ArchLeafPaging;

impl GuestPaging for ArchLeafPaging {
    fn validate(&self, va: u64, len: usize, write: bool) -> bool {
        // Never let a guest use a kernel alias, even if a bad PTE has U set.
        if va < 0x10000 || va.checked_add(len as u64).is_none_or(|end| end > 0x0000_8000_0000_0000) {
            return false;
        }
        let Some(end) = va.checked_add(len as u64) else {
            return false;
        };
        let mut page = va & !0xFFF;
        while page < end {
            let Some(entry) = crate::arch::leaf_entry(page) else {
                return false;
            };
            if !crate::arch::page_user_accessible(entry, write) {
                return false;
            }
            let Some(next_page) = page.checked_add(0x1000) else {
                return false;
            };
            page = next_page;
        }
        true
    }

    fn translate(&self, va: u64) -> Option<u64> {
        // The page walk returns a physical address, not a permanent kernel VA.
        let _ = va;
        None
    }

    unsafe fn copy_page(&self, va: u64, buf: *mut u8, len: usize, from_guest: bool) -> bool {
        #[cfg(not(test))]
        {
            let Some(physical) = crate::arch::translate_virt(va) else { return false; };
            let Ok(guard) = crate::mm::vm::direct_map_guard(physical & !0xFFF, 0x1000) else { return false; };
            let alias = (guard.va() + (physical & 0xFFF)) as *mut u8;
            if from_guest { core::ptr::copy(alias, buf, len); }
            else { core::ptr::copy(buf, alias, len); }
            true
        }
        #[cfg(test)]
        { let _ = (va, buf, len, from_guest); false }
    }
}

/// Page-chunked guest memory backed by any [`GuestPaging`] impl.
pub struct GuestAddressSpace<P> {
    paging: P,
}

impl<P: GuestPaging> GuestAddressSpace<P> {
    pub fn new(paging: P) -> Self {
        Self { paging }
    }

    fn copy_chunked(&mut self, va: u64, buf: *mut u8, len: usize, from_guest: bool) -> usize {
        if !self.paging.validate(va, len, !from_guest) {
            return 0;
        }
        let mut copied = 0usize;
        let mut guest_va = va;
        while copied < len {
            let bytes_in_page = (0x1000 - (guest_va & 0xFFF)) as usize;
            let chunk = (len - copied).min(bytes_in_page);
            unsafe {
                if !self.paging.copy_page(guest_va, buf.add(copied), chunk, from_guest) { break; }
            }
            copied += chunk;
            guest_va = guest_va.wrapping_add(chunk as u64);
        }
        copied
    }
}

impl<P: GuestPaging> GuestMemory for GuestAddressSpace<P> {
    fn read_virt(&mut self, va: u64, buf: &mut [u8]) -> usize {
        self.copy_chunked(va, buf.as_mut_ptr(), buf.len(), true)
    }

    fn write_virt(&mut self, va: u64, data: &[u8]) -> bool {
        self.copy_chunked(va, data.as_ptr() as *mut u8, data.len(), false) == data.len()
    }
}

// The active process adapter deliberately uses checked user-copy rather than
// treating translate_virt's physical address as a host pointer. In particular,
// LoongArch/RISC-V mappings are not generally identity mappings.
#[cfg(not(test))]
pub struct CurrentProcessMemory;

#[cfg(not(test))]
impl GuestMemory for CurrentProcessMemory {
    fn read_virt(&mut self, va: u64, buf: &mut [u8]) -> usize {
        if crate::mm::user_copy::copy_from_user(buf, va).is_ok() { buf.len() } else { 0 }
    }
    fn write_virt(&mut self, va: u64, data: &[u8]) -> bool {
        crate::mm::user_copy::copy_to_user(va, data).is_ok()
    }
    fn fetch_virt(&mut self, va: u64, buf: &mut [u8]) -> usize {
        if !crate::mm::user_copy::is_user_range(va, buf.len()) { return 0; }
        let Some(end) = va.checked_add(buf.len() as u64) else { return 0; };
        let mut page = va & !0xFFF;
        while page < end {
            if !crate::arch::leaf_entry(page).is_some_and(crate::arch::page_user_executable) { return 0; }
            page += 0x1000;
        }
        self.read_virt(va, buf)
    }
}

/// Flat test double: a single host buffer masquerading as guest
/// memory at VA 0. Every byte is readable and writable.
#[cfg(test)]
pub struct FlatPaging {
    pub base: *mut u8,
    pub len: usize,
}

#[cfg(test)]
impl GuestPaging for FlatPaging {
    fn validate(&self, va: u64, len: usize, _write: bool) -> bool {
        va.checked_add(len as u64)
            .map(|end| end <= self.len as u64)
            .unwrap_or(false)
    }

    fn translate(&self, va: u64) -> Option<u64> {
        if va < self.len as u64 {
            Some(self.base as u64 + va)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_write_roundtrip_within_page() {
        let mut backing = [0u8; 0x1000];
        let paging = FlatPaging {
            base: backing.as_mut_ptr(),
            len: 0x1000,
        };
        let mut mem = GuestAddressSpace::new(paging);
        assert!(mem.write_virt(0x10, &[1, 2, 3, 4]));
        let mut out = [0u8; 4];
        assert_eq!(mem.read_virt(0x10, &mut out), 4);
        assert_eq!(out, [1, 2, 3, 4]);
    }

    #[test]
    fn cross_page_copy_chunks_correctly() {
        let mut backing = [0u8; 0x2000];
        let paging = FlatPaging {
            base: backing.as_mut_ptr(),
            len: 0x2000,
        };
        let mut mem = GuestAddressSpace::new(paging);
        let data: alloc::vec::Vec<u8> = (0..0x10).collect();
        // Write across the first-page/second-page boundary at 0x1000.
        assert!(mem.write_virt(0x0FFC, &data));
        let mut out = [0u8; 0x10];
        assert_eq!(mem.read_virt(0x0FFC, &mut out), 0x10);
        assert_eq!(out, data.as_slice());
    }

    #[test]
    fn unmapped_range_fails_closed() {
        let mut backing = [0u8; 0x100];
        let paging = FlatPaging {
            base: backing.as_mut_ptr(),
            len: 0x100,
        };
        let mut mem = GuestAddressSpace::new(paging);
        assert_eq!(mem.read_virt(0x1000, &mut [0u8; 4]), 0);
        assert!(!mem.write_virt(0x1000, &[0u8; 4]));
    }
}
