//! Exercise the production alias publication with a recording page mapper.
#![allow(dead_code)]
use std::sync::Mutex;
static MAPS: Mutex<Vec<(u64, u64, u64)>> = Mutex::new(Vec::new());
static FAIL_AT: Mutex<Option<usize>> = Mutex::new(None);
pub mod arch {
    pub fn map_page(virtual_address: u64, physical: u64, flags: u64) -> bool {
        let mut maps = super::MAPS.lock().unwrap();
        if *super::FAIL_AT.lock().unwrap() == Some(maps.len()) { return false; }
        maps.push((virtual_address, physical, flags));
        true
    }
}
#[path = "../src/mm/kernel_heap_map.rs"]
mod kernel_heap_map;

#[test]
fn boot_mapping_is_published_only_after_every_supervisor_leaf_exists() {
    let physical = 0x1_2000_0000;
    assert_eq!(kernel_heap_map::address(physical, 4096), None);
    *FAIL_AT.lock().unwrap() = Some(2);
    assert!(!kernel_heap_map::init(physical, 4 * 4096));
    assert_eq!(kernel_heap_map::address(physical, 4096), None);
    MAPS.lock().unwrap().clear();
    *FAIL_AT.lock().unwrap() = None;
    assert!(kernel_heap_map::init(physical, 4 * 4096));
    let first = kernel_heap_map::address(physical, 4096).unwrap();
    assert!(first >> 63 != 0);
    for (index, &(va, pa, flags)) in MAPS.lock().unwrap().iter().enumerate() {
        assert_eq!(va, first + index as u64 * 4096);
        assert_eq!(pa, physical + index as u64 * 4096);
        assert_eq!(flags, 0x8000_0000_0000_0003);
    }
    // Allocations and frees in future roots use the same immutable aliases;
    // address lookup must never install a mapping in the caller's root.
    for _ in 0..10 {
        assert_eq!(kernel_heap_map::address(physical, 4 * 4096), Some(first));
        assert_eq!(kernel_heap_map::address(physical + 3 * 4096, 8192), None);
    }
    assert_eq!(MAPS.lock().unwrap().len(), 4);
}
