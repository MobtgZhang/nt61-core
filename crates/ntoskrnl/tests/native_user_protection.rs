//! Include the kernel wrapper unchanged; physical mappings are retained host
//! pages. The page walker, atomic replacement and invalidation calls are real.
#![allow(dead_code)]
use std::sync::Mutex;

#[path = "../src/mm/native_user_protection.rs"]
mod native_user_protection;
#[path = "../src/mm/user_page_protection.rs"]
mod user_page_protection;
#[derive(Debug, PartialEq)]
pub enum MmStatus {
    Ok,
    InvalidAddress,
}
fn user_mapping_available(base: u64, size: u64) -> bool {
    base >= 4096 && base.checked_add(size).is_some_and(|end| end <= 1 << 47)
}
static INVALIDATED: Mutex<Vec<u64>> = Mutex::new(Vec::new());
mod ke {
    pub mod tlb_shootdown {
        pub fn invalidate_va_all_cpus_complete(va: u64) {
            crate::INVALIDATED.lock().unwrap().push(va);
        }
    }
}
mod mm {
    pub mod vm {
        pub fn direct_map_phys(physical: u64, size: u64) -> Result<u64, ()> {
            assert_eq!(size, 4096);
            assert_eq!(physical & 4095, 0);
            Ok(physical)
        }
        pub fn direct_unmap_va(_: u64, size: u64) {
            assert_eq!(size, 4096);
        }
    }
}
#[repr(align(4096))]
struct Table([u64; 512]);

#[test]
#[cfg(target_arch = "x86_64")]
fn noaccess_restore_and_invalid_range_through_kernel_wrapper() {
    use native_user_protection::protect_user_range as protect;
    let mut tables = [
        Table([0; 512]),
        Table([0; 512]),
        Table([0; 512]),
        Table([0; 512]),
    ];
    for level in 0..3 {
        tables[level].0[0] = tables[level + 1].0.as_ptr() as u64 | 7;
    }
    let root = tables[0].0.as_ptr() as u64;
    let original = 0x12345000 | 7 | (1 << 63);
    tables[3].0[1] = original;
    tables[3].0[2] = original + 4096;
    assert_eq!(protect(root, 8191, 2, 1), MmStatus::Ok);
    assert_eq!(tables[3].0[1] & 4, 0, "NOACCESS still allows user reads");
    assert_eq!(tables[3].0[2] & 4, 0);
    assert_eq!(*INVALIDATED.lock().unwrap(), [4096, 8192]);
    assert_eq!(protect(root, 4096, 8192, 4), MmStatus::Ok);
    assert_eq!(tables[3].0[1], original);
    assert_eq!(tables[3].0[2], original + 4096);
    let before = tables[3].0;
    assert_eq!(protect(root, 4096, 12288, 1), MmStatus::InvalidAddress);
    assert_eq!(
        tables[3].0, before,
        "missing last page must not revoke prefix"
    );
    assert_eq!(INVALIDATED.lock().unwrap().len(), 4);
}
