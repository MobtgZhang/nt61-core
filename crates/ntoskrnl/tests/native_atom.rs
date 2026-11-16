//! Production atom and LUID services against explicitly granted host memory.
#![allow(dead_code)]
#[path = "native_atom/ntdll.rs"]
mod ntdll;
use ntdll::{atom::*, luid::*, status::*};
use std::sync::{Arc, Barrier, Mutex};
static SERIAL: Mutex<()> = Mutex::new(());
mod ke {
    pub mod sync {
        pub struct Spinlock<T>(std::sync::Mutex<T>);
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
mod mm {
    pub mod user_copy {
        use std::cell::RefCell;
        thread_local! { static RANGES: RefCell<Vec<(u64, usize, bool)>> = const { RefCell::new(Vec::new()) }; }
        pub fn allow(address: u64, size: usize, write: bool) {
            RANGES.with(|ranges| ranges.borrow_mut().push((address, size, write)));
        }
        fn probe(address: u64, size: usize, write: bool) -> Result<(), i32> {
            RANGES.with(|ranges| {
                if address != 0
                    && ranges.borrow().iter().any(|&(base, len, writable)| {
                        (!write || writable)
                            && address >= base
                            && address
                                .checked_add(size as u64)
                                .is_some_and(|end| end <= base + len as u64)
                    })
                {
                    Ok(())
                } else {
                    Err(0xc0000005u32 as i32)
                }
            })
        }
        pub fn probe_user_write(address: u64, size: usize) -> Result<(), i32> {
            probe(address, size, true)
        }
        pub fn copy_from_user(dst: &mut [u8], src: u64) -> Result<(), i32> {
            probe(src, dst.len(), false)?;
            // SAFETY: the fixture grants only live allocations.
            unsafe {
                std::ptr::copy_nonoverlapping(src as *const u8, dst.as_mut_ptr(), dst.len());
            }
            Ok(())
        }
        pub fn copy_to_user(dst: u64, src: &[u8]) -> Result<(), i32> {
            probe(dst, src.len(), true)?;
            // SAFETY: the fixture grants only live allocations.
            unsafe {
                std::ptr::copy_nonoverlapping(src.as_ptr(), dst as *mut u8, src.len());
            }
            Ok(())
        }
        pub fn write_user_object<T: Copy>(address: u64, value: &T) -> Result<(), i32> {
            probe_user_write(address, std::mem::size_of::<T>())?;
            // SAFETY: the fixture probes the complete output, including unaligned cases.
            unsafe {
                (address as *mut T).write_unaligned(*value);
            }
            Ok(())
        }
    }
}
fn input(name: &str) -> Vec<u16> {
    let units: Vec<_> = name.encode_utf16().collect();
    mm::user_copy::allow(units.as_ptr() as u64, units.len() * 2, false);
    units
}
fn add(name: &str) -> u16 {
    let units = input(name);
    let mut atom = 0u16;
    mm::user_copy::allow(&mut atom as *mut _ as u64, 2, true);
    assert_eq!(
        nt_add_atom(units.as_ptr(), (units.len() * 2) as u32, &mut atom),
        0
    );
    atom
}
fn find(name: &str) -> (i32, u16) {
    let units = input(name);
    let mut atom = 0;
    mm::user_copy::allow(&mut atom as *mut _ as u64, 2, true);
    (
        nt_find_atom(units.as_ptr(), (units.len() * 2) as u32, &mut atom),
        atom,
    )
}
#[repr(C, align(4))]
struct Buffer([u8; 528]);
fn query(atom: u16, class: u32, len: u32) -> (i32, Buffer, u32) {
    let mut buffer = Buffer([0xab; 528]);
    let mut required = 0xdeadbeefu32;
    mm::user_copy::allow(buffer.0.as_mut_ptr() as u64, buffer.0.len(), true);
    mm::user_copy::allow(&mut required as *mut _ as u64, 4, true);
    let status = nt_query_information_atom(atom, class, buffer.0.as_mut_ptr(), len, &mut required);
    (status, buffer, required)
}
fn word(buffer: &Buffer, offset: usize) -> u16 {
    u16::from_le_bytes(buffer.0[offset..offset + 2].try_into().unwrap())
}
#[test]
fn add_find_delete_preserves_first_spelling_and_reference_count() {
    let _guard = SERIAL.lock().unwrap();
    let atom = add("MixedCase");
    assert!(atom >= 0xc000);
    assert_eq!(add("MIXEDCASE"), atom);
    assert_eq!(find("mixedcase"), (0, atom));
    let (status, buffer, required) = query(atom, 0, 528);
    assert_eq!(
        (status, word(&buffer, 0), word(&buffer, 2), required),
        (0, 2, 0, 26)
    );
    assert_eq!(word(&buffer, 6), b'M' as u16);
    assert_eq!(buffer.0[26], 0xab);
    assert_eq!(nt_delete_atom(atom), 0);
    assert_eq!(word(&query(atom, 0, 528).1, 0), 1);
    assert_eq!(nt_delete_atom(atom), 0);
    assert_eq!(find("mixedcase").0, STATUS_OBJECT_NAME_NOT_FOUND);
    assert_eq!(nt_delete_atom(atom), STATUS_INVALID_HANDLE);
}
#[test]
fn integer_atoms_are_pinned_and_do_not_enter_the_string_table() {
    let _guard = SERIAL.lock().unwrap();
    assert_eq!(add("#49151"), 49151);
    assert_eq!(find("#00012"), (0, 12));
    assert_eq!(nt_delete_atom(12), 0);
    let (status, buffer, required) = query(12, 0, 528);
    assert_eq!(
        (
            status,
            word(&buffer, 0),
            word(&buffer, 2),
            word(&buffer, 4),
            required
        ),
        (0, 1, 1, 6, 14)
    );
    for name in ["#0", "#49152", "#9999999999"] {
        assert_eq!(find(name).0, STATUS_INVALID_PARAMETER);
    }
    for name in ["#", "#x", "#+1"] {
        let atom = add(name);
        assert!(atom >= 0xc000);
        nt_delete_atom(atom);
    }
    assert_eq!(nt_delete_atom(0), STATUS_INVALID_HANDLE);
}
#[test]
fn basic_query_handles_short_truncated_and_odd_byte_capacities() {
    let _guard = SERIAL.lock().unwrap();
    let atom = add("abcdef");
    for len in [0, 1, 5] {
        let (status, buffer, required) = query(atom, 0, len);
        assert_eq!((status, required), (STATUS_INFO_LENGTH_MISMATCH, 6));
        assert!(buffer.0.iter().all(|&byte| byte == 0xab));
    }
    for len in [6, 7, 8, 9] {
        let (status, buffer, _) = query(atom, 0, len);
        assert_eq!((status, word(&buffer, 4)), (STATUS_BUFFER_TOO_SMALL, 12));
        assert_eq!(buffer.0[len as usize], 0xab);
    }
    for len in [10, 11, 16, 17, 20] {
        let (status, buffer, required) = query(atom, 0, len);
        let copied = ((len - 8) / 2).min(6) as usize;
        assert_eq!((status, word(&buffer, 4)), (0, (copied * 2) as u16));
        assert_eq!(word(&buffer, 6 + copied * 2), 0);
        assert_eq!(required as usize, 8 + copied * 2);
        assert_eq!(buffer.0[required as usize], 0xab);
    }
    nt_delete_atom(atom);
}
#[test]
fn length_limits_embedded_nul_and_unicode_comparison() {
    let _guard = SERIAL.lock().unwrap();
    let atom = add("éCase\0ignored");
    assert_eq!(find("ÉCASE"), (0, atom));
    nt_delete_atom(atom);
    let atom = add(&"x".repeat(255));
    assert_eq!(word(&query(atom, 0, 528).1, 4), 510);
    nt_delete_atom(atom);
    assert_eq!(find(&"x".repeat(256)).0, STATUS_INVALID_PARAMETER);
    assert_eq!(find("").0, STATUS_OBJECT_NAME_INVALID);
}
#[test]
fn output_probes_precede_atom_mutation_and_invalid_classes_do_not_write() {
    let _guard = SERIAL.lock().unwrap();
    let units = input("probe-before-add");
    assert_eq!(
        nt_add_atom(units.as_ptr(), (units.len() * 2) as u32, 0x1000 as *mut _),
        STATUS_ACCESS_VIOLATION
    );
    assert_eq!(find("probe-before-add").0, STATUS_OBJECT_NAME_NOT_FOUND);
    assert_eq!(
        nt_add_atom(0x1000 as *const _, 2, std::ptr::null_mut()),
        STATUS_ACCESS_VIOLATION
    );
    assert_eq!(
        nt_add_atom(0x1001 as *const _, 2, std::ptr::null_mut()),
        0x80000002u32 as i32
    );
    let (status, buffer, required) = query(1, 77, 528);
    assert_eq!((status, required), (STATUS_INVALID_INFO_CLASS, 0));
    assert!(buffer.0.iter().all(|&byte| byte == 0xab));
    assert_eq!(
        nt_query_information_atom(1, 0, 0x1001 as *mut _, 8, std::ptr::null_mut()),
        0x80000002u32 as i32
    );
}
#[test]
fn table_query_reports_total_and_partial_ids_without_overwrite() {
    let _guard = SERIAL.lock().unwrap();
    let first = add("table-first");
    let second = add("table-second");
    let (status, buffer, required) = query(0, 1, 8);
    assert_eq!((status, required), (0, 8));
    assert_eq!(u32::from_le_bytes(buffer.0[..4].try_into().unwrap()), 2);
    assert!([word(&buffer, 4), word(&buffer, 6)].contains(&first));
    assert!([word(&buffer, 4), word(&buffer, 6)].contains(&second));
    let (status, buffer, required) = query(0, 1, 6);
    assert_eq!(
        (status, required, buffer.0[6]),
        (STATUS_INFO_LENGTH_MISMATCH, 4, 0xab)
    );
    nt_delete_atom(first);
    nt_delete_atom(second);
}
#[test]
fn concurrent_acquisitions_share_one_atom_and_release_all_references() {
    let _guard = SERIAL.lock().unwrap();
    let barrier = Arc::new(Barrier::new(8));
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let atom = add("concurrent-name");
                barrier.wait();
                assert_eq!(find("CONCURRENT-NAME"), (0, atom));
                barrier.wait();
                assert_eq!(nt_delete_atom(atom), 0);
                atom
            })
        })
        .collect();
    let ids: Vec<_> = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect();
    assert!(ids.iter().all(|&id| id == ids[0]));
    assert_eq!(find("concurrent-name").0, STATUS_OBJECT_NAME_NOT_FOUND);
}
#[test]
fn luid_allocation_is_unique_under_concurrency_and_probes_output() {
    assert_eq!(
        nt_allocate_locally_unique_id(std::ptr::null_mut()),
        STATUS_ACCESS_VIOLATION
    );
    assert_eq!(
        nt_allocate_locally_unique_id(0x1001 as *mut _),
        0x80000002u32 as i32
    );
    let threads: Vec<_> = (0..8)
        .map(|_| {
            std::thread::spawn(|| {
                let mut values = Vec::new();
                let mut value = 0u64;
                mm::user_copy::allow(&mut value as *mut _ as u64, 8, true);
                for _ in 0..256 {
                    assert_eq!(nt_allocate_locally_unique_id(&mut value), 0);
                    values.push(value);
                }
                values
            })
        })
        .collect();
    let mut values: Vec<_> = threads
        .into_iter()
        .flat_map(|thread| thread.join().unwrap())
        .collect();
    values.sort_unstable();
    values.dedup();
    assert_eq!(values.len(), 2048);
    assert!(values[0] >= 0x1000);
}
