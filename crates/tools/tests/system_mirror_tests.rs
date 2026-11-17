//! Production mirror writeback with only the block device and lock adapted.
#![allow(dead_code)]
extern crate alloc;
use std::sync::Mutex;

mod ke {
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
}

struct EspRamdisk {
    base: *const u8,
    size: usize,
    block_size: u32,
}
static SYS_RAMDISK: ke::sync::Spinlock<Option<EspRamdisk>> = ke::sync::Spinlock::new(None);
static DISKS: Mutex<Vec<Vec<u8>>> = Mutex::new(Vec::new());
static WRITES: Mutex<Vec<(usize, u64)>> = Mutex::new(Vec::new());
static FAIL_WRITE: Mutex<Option<usize>> = Mutex::new(None);
static FAIL_FLUSH: Mutex<Option<usize>> = Mutex::new(None);
static FLUSHES: Mutex<usize> = Mutex::new(0);
static TEST_LOCK: Mutex<()> = Mutex::new(());

mod drivers {
    pub mod storage {
        pub mod disk {
            pub struct DiskInfo {
                pub sector_count: u64,
            }
            pub fn disk_count() -> usize {
                crate::DISKS.lock().unwrap().len()
            }
            pub fn get_disk(index: usize) -> Option<DiskInfo> {
                crate::DISKS
                    .lock()
                    .unwrap()
                    .get(index)
                    .map(|disk| DiskInfo {
                        sector_count: disk.len() as u64 / 512,
                    })
            }
            pub fn disk_read(index: usize, offset: u64, count: usize, out: &mut [u8]) -> usize {
                let disks = crate::DISKS.lock().unwrap();
                let Some(bytes) = disks
                    .get(index)
                    .and_then(|disk| disk.get(offset as usize..offset as usize + count))
                else {
                    return 0;
                };
                out[..count].copy_from_slice(bytes);
                count
            }
            pub fn disk_write(index: usize, offset: u64, count: usize, input: &[u8]) -> usize {
                let mut writes = crate::WRITES.lock().unwrap();
                if *crate::FAIL_WRITE.lock().unwrap() == Some(writes.len()) {
                    return 0;
                }
                let mut disks = crate::DISKS.lock().unwrap();
                let Some(bytes) = disks
                    .get_mut(index)
                    .and_then(|disk| disk.get_mut(offset as usize..offset as usize + count))
                else {
                    return 0;
                };
                bytes.copy_from_slice(&input[..count]);
                writes.push((index, offset));
                count
            }
            pub fn flush(_: usize) -> bool {
                let mut n = crate::FLUSHES.lock().unwrap();
                *n += 1;
                *crate::FAIL_FLUSH.lock().unwrap() != Some(*n)
            }
        }
    }
}

#[path = "../../fs/src/fs/system_mirror.rs"]
mod system_mirror;

fn setup() -> Vec<u8> {
    setup_sectors(8)
}
fn setup_sectors(sectors: usize) -> Vec<u8> {
    let mut mirror = vec![0; sectors * 512];
    mirror[3..11].copy_from_slice(b"NTFS    ");
    mirror[11..13].copy_from_slice(&512u16.to_le_bytes());
    mirror[28..32].copy_from_slice(&4u32.to_le_bytes());
    mirror[72..80].copy_from_slice(&0x1234u64.to_le_bytes());
    mirror[510..512].copy_from_slice(&[0x55, 0xaa]);
    let mut disk = vec![0; (sectors + 4) * 512];
    disk[4 * 512..].copy_from_slice(&mirror);
    // C: is on disk one; disk zero must never be modified.
    *DISKS.lock().unwrap() = vec![vec![0x7e; (sectors + 4) * 512], disk];
    *SYS_RAMDISK.lock() = Some(EspRamdisk {
        base: mirror.as_ptr(),
        size: mirror.len(),
        block_size: 512,
    });
    system_mirror::reset(mirror.len());
    WRITES.lock().unwrap().clear();
    *FAIL_WRITE.lock().unwrap() = None;
    *FAIL_FLUSH.lock().unwrap() = None;
    *FLUSHES.lock().unwrap() = 0;
    mirror
}

#[test]
fn dirty_ranges_cross_bitmap_words_and_include_the_last_partial_word() {
    let _serial = TEST_LOCK.lock().unwrap();
    let mut mirror = setup_sectors(130);
    // This mutation crosses both a sector boundary and the first bitmap word.
    mutate(&mut mirror, 64 * 512 - 2, &[1, 2, 3, 4]);
    mutate(&mut mirror, 129 * 512 + 511, &[5]);
    system_mirror::flush().unwrap();
    assert_eq!(
        *WRITES.lock().unwrap(),
        vec![(1, 67 * 512), (1, 68 * 512), (1, 133 * 512)]
    );
    assert_eq!(DISKS.lock().unwrap()[1][4 * 512..], mirror);
}
fn mutate(mirror: &mut [u8], offset: usize, bytes: &[u8]) {
    let _guard = SYS_RAMDISK.lock();
    system_mirror::mark(offset, bytes.len());
    mirror[offset..offset + bytes.len()].copy_from_slice(bytes);
}

#[test]
fn dirty_sectors_survive_remount_without_touching_other_disk() {
    let _serial = TEST_LOCK.lock().unwrap();
    let mut mirror = setup();
    mutate(&mut mirror, 512 + 510, &[1, 2, 3, 4]);
    mutate(&mut mirror, 7 * 512, b"saved-mode");
    system_mirror::flush().unwrap();
    assert_eq!(
        *WRITES.lock().unwrap(),
        vec![(1, 5 * 512), (1, 6 * 512), (1, 11 * 512)]
    );
    let disks = DISKS.lock().unwrap();
    assert!(disks[0].iter().all(|byte| *byte == 0x7e));
    let remounted = disks[1][4 * 512..].to_vec();
    assert_eq!(remounted, mirror);
    drop(disks);
    system_mirror::flush().unwrap();
    assert_eq!(WRITES.lock().unwrap().len(), 3);
}

#[test]
fn failed_write_and_failed_flush_keep_the_entire_retry_set() {
    let _serial = TEST_LOCK.lock().unwrap();
    let mut mirror = setup();
    mutate(&mut mirror, 512, b"metadata");
    mutate(&mut mirror, 3 * 512, b"data");
    *FAIL_WRITE.lock().unwrap() = Some(1);
    assert_eq!(system_mirror::flush(), Err(0xc0000001));
    *FAIL_WRITE.lock().unwrap() = None;
    // The next boundary's final flush fails after both writes succeed.
    *FAIL_FLUSH.lock().unwrap() = Some(3);
    assert_eq!(system_mirror::flush(), Err(0xc0000001));
    *FAIL_FLUSH.lock().unwrap() = None;
    system_mirror::flush().unwrap();
    assert_eq!(
        *WRITES.lock().unwrap(),
        vec![
            (1, 5 * 512),
            (1, 5 * 512),
            (1, 7 * 512),
            (1, 5 * 512),
            (1, 7 * 512)
        ]
    );
}

#[test]
fn unmatched_ambiguous_or_unflushable_media_reject_before_writing() {
    let _serial = TEST_LOCK.lock().unwrap();
    let mut mirror = setup();
    mutate(&mut mirror, 512, b"saved-mode");
    DISKS.lock().unwrap()[1][4 * 512 + 72] ^= 1;
    assert_eq!(system_mirror::flush(), Err(0xc00000bb));
    assert!(WRITES.lock().unwrap().is_empty());
    DISKS.lock().unwrap()[1][4 * 512 + 72] ^= 1;
    let clone = DISKS.lock().unwrap()[1].clone();
    DISKS.lock().unwrap().push(clone);
    assert_eq!(system_mirror::flush(), Err(0xc00000bb));
    assert!(WRITES.lock().unwrap().is_empty());
    DISKS.lock().unwrap().pop();
    *FAIL_FLUSH.lock().unwrap() = Some(1);
    assert_eq!(system_mirror::flush(), Err(0xc00000bb));
    assert!(WRITES.lock().unwrap().is_empty());
    *FAIL_FLUSH.lock().unwrap() = None;
    system_mirror::flush().unwrap();
    assert_eq!(WRITES.lock().unwrap().len(), 1);
}
