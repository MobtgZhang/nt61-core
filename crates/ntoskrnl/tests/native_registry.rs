//! Run the production registry, ACL, token and access checker on bounded host memory.
#![allow(dead_code, non_camel_case_types, unused_imports)]
extern crate alloc;
#[macro_export]
macro_rules! kprintln {
    ($($arg:tt)*) => {};
}
#[path = "../src/se/acl.rs"]
pub mod acl;
#[path = "../src/se/descriptor.rs"]
pub mod descriptor;
#[path = "../src/se/integrity.rs"]
pub mod integrity;
#[path = "../src/se/seaccess.rs"]
pub mod seaccess;
#[path = "../src/se/sid.rs"]
pub mod sid;
#[path = "../src/se/token.rs"]
pub mod token;
#[path = "../src/se/tokenmgmt.rs"]
pub mod tokenmgmt;
pub mod hal {
    pub mod serial {
        pub fn write_string(_: &str) {}
    }
}
pub mod string {
    pub unsafe fn alloc_string(_: u32) -> *mut u8 {
        core::ptr::null_mut()
    }
}
pub mod se {
    pub use crate::{acl, descriptor, integrity, seaccess, sid, token, tokenmgmt};
}
#[path = "../../libs/ntdll/src/ntdll/locale.rs"]
pub mod locale;
#[path = "../../libs/ntdll/src/ntdll/registry.rs"]
pub mod registry;
pub mod privilege {
    // Locale privilege APIs are not exercised by this registry harness.
    pub fn enabled(_: u32) -> bool {
        false
    }
}
#[path = "../../libs/ntdll/src/ntdll/status.rs"]
pub mod status;
#[path = "../../libs/ntdll/src/ntdll/types.rs"]
pub mod types;
pub mod libs {
    pub mod ntdll {
        pub use crate::{registry, status, types};
        pub mod sync {
            pub unsafe fn nt_set_event(_: crate::types::HANDLE, _: *mut u32) -> i32 {
                0
            }
        }
    }
}
pub mod ps {
    pub mod process {
        pub struct Eprocess;
        impl Eprocess {
            pub fn get_token(&self) -> *mut crate::token::Token {
                TOKEN.with(|token| token.get())
            }
        }
        std::thread_local! { static TOKEN: core::cell::UnsafeCell<crate::token::Token> = core::cell::UnsafeCell::new(crate::token::Token::new_primary(crate::sid::sid::well_known(crate::sid::WellKnownSid::LocalSystem))); }

        pub fn with_user_token<T>(run: impl FnOnce() -> T) -> T {
            struct Restore(crate::token::Token);
            impl Drop for Restore {
                fn drop(&mut self) {
                    TOKEN.with(|slot| unsafe {
                        core::ptr::swap(slot.get(), &mut self.0);
                    });
                }
            }
            // The harness token is thread-local; no registry call retains it.
            // Restore it even if an assertion unwinds.
            let previous = TOKEN.with(|slot| unsafe {
                core::ptr::replace(
                    slot.get(),
                    crate::token::Token::new_primary(crate::sid::SID_USERS),
                )
            });
            let _restore = Restore(previous);
            run()
        }
    }
}
pub mod arch {
    pub mod common {
        pub mod percpu {
            pub fn get_current_process() -> *mut crate::ps::process::Eprocess {
                core::ptr::NonNull::dangling().as_ptr()
            }
        }
    }
}
pub mod ke {
    pub mod sync {
        pub use ::spin::Mutex as Spinlock;
    }
    pub mod time {
        pub fn get_system_time() -> u64 {
            123456789
        }
    }
}
pub mod mm {
    pub mod heap {
        pub struct Heap;
        pub static KERNEL_HEAP: Heap = Heap;
        impl Heap {
            pub fn configured_region(&self) -> Option<(usize, usize)> {
                Some((0, 0))
            }
            pub fn configure(&self, _: *mut u8, _: usize) {}
        }
    }
    pub mod pool {
        pub enum PoolType {
            NonPaged,
        }
        std::thread_local! {
            pub static FAIL_AFTER: core::cell::Cell<Option<usize>> = const { core::cell::Cell::new(None) };
        }
        pub static LIVE: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
        pub fn allocate(_: PoolType, size: usize) -> *mut u8 {
            let fail = FAIL_AFTER.with(|limit| match limit.get() {
                Some(0) => true,
                Some(value) => {
                    limit.set(Some(value - 1));
                    false
                }
                None => false,
            });
            if fail {
                return core::ptr::null_mut();
            }
            let layout = std::alloc::Layout::from_size_align(size + 16, 16).unwrap();
            let allocation = unsafe { std::alloc::alloc(layout) };
            if allocation.is_null() {
                return allocation;
            }
            unsafe {
                allocation.cast::<usize>().write(size + 16);
            }
            LIVE.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            unsafe { allocation.add(16) }
        }
        pub fn free(pointer: *mut u8) {
            let allocation = unsafe { pointer.sub(16) };
            let size = unsafe { allocation.cast::<usize>().read() };
            let layout = std::alloc::Layout::from_size_align(size, 16).unwrap();
            unsafe {
                std::alloc::dealloc(allocation, layout);
            }
            LIVE.fetch_sub(1, core::sync::atomic::Ordering::Relaxed);
        }
    }
    pub mod user_copy {
        use std::cell::RefCell;
        thread_local! { static RANGES: RefCell<Vec<(u64, usize, bool)>> = const { RefCell::new(Vec::new()) }; }
        pub fn allow(pointer: u64, size: usize, writable: bool) {
            RANGES.with(|ranges| ranges.borrow_mut().push((pointer, size, writable)));
        }
        fn probe(address: u64, size: usize, write: bool) -> Result<(), i32> {
            RANGES.with(|ranges| {
                if address != 0
                    && ranges.borrow().iter().any(|&(base, length, writable)| {
                        (!write || writable)
                            && address >= base
                            && address
                                .checked_add(size as u64)
                                .is_some_and(|end| end <= base + length as u64)
                    })
                {
                    Ok(())
                } else {
                    Err(0xc0000005u32 as i32)
                }
            })
        }
        pub fn probe_user_read(address: u64, size: usize) -> Result<(), i32> {
            probe(address, size, false)
        }
        pub fn probe_user_write(address: u64, size: usize) -> Result<(), i32> {
            probe(address, size, true)
        }
        pub fn copy_from_user(dst: &mut [u8], src: u64) -> Result<(), i32> {
            probe(src, dst.len(), false)?;
            unsafe {
                core::ptr::copy_nonoverlapping(src as *const u8, dst.as_mut_ptr(), dst.len());
            }
            Ok(())
        }
        pub fn copy_to_user(dst: u64, src: &[u8]) -> Result<(), i32> {
            probe(dst, src.len(), true)?;
            unsafe {
                core::ptr::copy_nonoverlapping(src.as_ptr(), dst as *mut u8, src.len());
            }
            Ok(())
        }
        pub fn read_user_object<T: Copy>(src: u64) -> Result<T, i32> {
            probe(src, core::mem::size_of::<T>(), false)?;
            Ok(unsafe { core::ptr::read_unaligned(src as *const T) })
        }
        pub fn write_user_object<T: Copy>(dst: u64, value: &T) -> Result<(), i32> {
            probe(dst, core::mem::size_of::<T>(), true)?;
            unsafe {
                core::ptr::write_unaligned(dst as *mut T, *value);
            }
            Ok(())
        }
        pub fn read_user_unicode_string(address: u64, max: usize) -> Result<Vec<u16>, i32> {
            let header = read_user_object::<crate::types::UnicodeString>(address)?;
            if header.length == 0
                || header.length % 2 != 0
                || header.length as usize > max
                || header.maximum_length < header.length
            {
                return Err(0xc0000005u32 as i32);
            }
            let mut bytes = vec![0; header.length as usize];
            copy_from_user(&mut bytes, header.buffer as u64)?;
            Ok(bytes
                .chunks_exact(2)
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .collect())
        }
    }
}

#[cfg(test)]
mod mutation_cases {
    use super::{mm::user_copy::allow, registry::*, status::*, types::*};
    fn pointer<T>(value: &T, write: bool) -> u64 {
        let address = value as *const T as u64;
        allow(address, core::mem::size_of::<T>(), write);
        address
    }
    fn writable<T>(value: &mut T) -> *mut T {
        allow(value as *mut T as u64, core::mem::size_of::<T>(), true);
        value
    }
    struct Name {
        units: Vec<u16>,
        header: UnicodeString,
    }
    impl Name {
        fn new(text: &str) -> Self {
            let mut units: Vec<u16> = text.encode_utf16().collect();
            let header = UnicodeString {
                length: (units.len() * 2) as u16,
                maximum_length: (units.len() * 2) as u16,
                buffer: units.as_mut_ptr(),
            };
            allow(header.buffer as u64, units.len() * 2, false);
            Self { units, header }
        }
        fn address(&self) -> *const UnicodeString {
            pointer(&self.header, false) as _
        }
    }
    fn create(text: &str, access: u32) -> HANDLE {
        init();
        let name = Name::new(text);
        let mut attributes = ObjectAttributes::new();
        attributes.object_name = name.address() as _;
        let mut handle = core::ptr::null_mut();
        let output = pointer(&mut handle, true) as _;
        assert_eq!(
            unsafe {
                nt_create_key(
                    output,
                    access,
                    pointer(&attributes, false) as _,
                    0,
                    core::ptr::null_mut(),
                    0,
                    core::ptr::null_mut(),
                )
            },
            STATUS_SUCCESS
        );
        handle
    }
    #[test]
    fn bootstrap_user_cannot_create_in_read_only_root_but_can_read_system_created_key() {
        let _serial = REGISTRY_TEST_LOCK.lock();
        init();
        let name = Name::new("BootstrapUserAccess");
        let mut attributes = ObjectAttributes::new();
        attributes.object_name = name.address() as _;
        let attr = pointer(&attributes, false) as *mut ObjectAttributes;
        let mut handle: HANDLE = core::ptr::null_mut();
        let out = pointer(&mut handle, true) as *mut HANDLE;
        let mut disposition = 0xccccccccu32;
        let action = pointer(&mut disposition, true) as *mut u32;
        // Match the native-registry fixture's access and root-directory inputs,
        // with the exact token identity used by create_user_process.
        crate::ps::process::with_user_token(|| unsafe {
            assert_eq!(
                nt_create_key(out, 0x1000f, attr, 0, core::ptr::null_mut(), 0, action),
                STATUS_ACCESS_DENIED
            );
            assert!(handle.is_null());
            assert_eq!(disposition, 0xcccccccc);
            assert_eq!(nt_open_key(out, 1, attr), STATUS_OBJECT_NAME_NOT_FOUND);
        });
        assert_eq!(
            unsafe { nt_create_key(out, 0x1000f, attr, 0, core::ptr::null_mut(), 0, action) },
            STATUS_SUCCESS
        );
        assert_eq!(disposition, 1, "denied create must not insert a key");
        let system_handle = handle;
        crate::ps::process::with_user_token(|| unsafe {
            assert_eq!(nt_open_key(out, 1, attr), STATUS_SUCCESS);
            assert_eq!(query(handle).1, name.units);
            assert_eq!(nt_reg_close_key(handle), STATUS_SUCCESS);
            handle = core::ptr::null_mut();
            assert_eq!(nt_open_key(out, 0x1000f, attr), STATUS_ACCESS_DENIED);
            assert!(handle.is_null());
        });
        assert_eq!(unsafe { nt_delete_key(system_handle) }, STATUS_SUCCESS);
    }
    fn query(handle: HANDLE) -> (i64, Vec<u16>) {
        let mut bytes = [0u64; 80];
        let mut length = 0;
        assert_eq!(
            unsafe {
                nt_query_key(
                    handle,
                    0,
                    pointer(&mut bytes, true) as _,
                    640,
                    pointer(&mut length, true) as _,
                )
            },
            STATUS_SUCCESS
        );
        let bytes = unsafe { core::slice::from_raw_parts(bytes.as_ptr().cast::<u8>(), 640) };
        let size = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        (
            i64::from_le_bytes(bytes[..8].try_into().unwrap()),
            bytes[16..16 + size]
                .chunks_exact(2)
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .collect(),
        )
    }
    #[test]
    fn install_language_flush_and_user_keys_share_one_absolute_namespace() {
        let _serial = REGISTRY_TEST_LOCK.lock();
        use crate::locale::*;
        let mut language = 0u16;
        let output = writable(&mut language);
        assert_eq!(nt_query_install_ui_language(output), STATUS_SUCCESS);
        assert_eq!(language, 0x409);
        assert_eq!(nt_is_ui_language_comitted(), STATUS_OBJECT_NAME_NOT_FOUND);
        assert_eq!(
            flush_install_ui_language(false, 0x804, 1),
            STATUS_UNSUCCESSFUL
        );
        // The installed bootstrap image lacks this configuration. A failed
        // open still leaves the new install-language value visible.
        assert_eq!(
            nt_flush_install_ui_language_from_user(0x804, 0),
            STATUS_OBJECT_NAME_NOT_FOUND
        );
        assert_eq!(nt_query_install_ui_language(output), STATUS_SUCCESS);
        assert_eq!(language, 0x804);
        assert_eq!(nt_query_default_ui_language(output), STATUS_SUCCESS);
        assert_eq!(language, 0x804);

        let key = create(
            "\\REGISTRY\\MACHINE\\SYSTEM\\CurrentControlSet\\Control\\NLS\\Language",
            0xf003f,
        );
        let name = Name::new("InstallLanguage");
        let old = [0x12u8, 0, 0xfe, 0x33, 0x45];
        allow(old.as_ptr() as u64, old.len(), false);
        assert_eq!(
            unsafe {
                nt_set_value_key(
                    key,
                    name.address() as _,
                    0,
                    3,
                    old.as_ptr() as _,
                    old.len() as u32,
                )
            },
            STATUS_SUCCESS
        );
        assert_eq!(
            nt_flush_install_ui_language_from_user(0xabcd_0411, 1),
            STATUS_SUCCESS
        );
        assert_eq!(nt_query_install_ui_language(output), STATUS_SUCCESS);
        assert_eq!(language, 0x411);
        assert_eq!(nt_query_default_ui_language(output), STATUS_SUCCESS);
        assert_eq!(language, 0x411);
        assert_eq!(nt_is_ui_language_comitted(), STATUS_SUCCESS);
        assert_eq!(
            nt_flush_install_ui_language_from_user(0xabcd_0411, 0),
            STATUS_UNSUCCESSFUL
        );
        assert_eq!(
            nt_flush_install_ui_language_from_user(0x411, 0),
            STATUS_SUCCESS
        );

        let lower =
            Name::new("\\registry\\machine\\system\\currentcontrolset\\control\\nls\\language");
        let mut attributes = ObjectAttributes::new();
        attributes.object_name = lower.address() as _;
        let mut opened = core::ptr::null_mut();
        assert_eq!(
            unsafe { nt_open_key(writable(&mut opened), 1, pointer(&attributes, false) as _) },
            STATUS_SUCCESS
        );
        for (text, expected) in [
            (
                "InstallLanguage",
                b"0\x004\x001\x001\x00\x00\x00".as_slice(),
            ),
            ("PreviousInstallLanguage", old.as_slice()),
        ] {
            let name = Name::new(text);
            let mut bytes = [0xa5u64; 8];
            let mut required = 0u32;
            assert_eq!(
                unsafe {
                    nt_query_value_key(
                        opened,
                        name.address() as _,
                        2,
                        writable(&mut bytes) as _,
                        64,
                        writable(&mut required),
                    )
                },
                STATUS_SUCCESS
            );
            let bytes = unsafe { core::slice::from_raw_parts(bytes.as_ptr().cast::<u8>(), 64) };
            assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 1);
            assert_eq!(
                u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize,
                expected.len()
            );
            assert_eq!(&bytes[12..12 + expected.len()], expected);
        }
        // KEY_NAME_INFORMATION must contain just one object-manager prefix.
        let mut bytes = [0u64; 128];
        let mut required = 0u32;
        assert_eq!(
            unsafe {
                nt_query_key(
                    opened,
                    3,
                    writable(&mut bytes) as _,
                    1024,
                    writable(&mut required),
                )
            },
            STATUS_SUCCESS
        );
        let bytes = unsafe { core::slice::from_raw_parts(bytes.as_ptr().cast::<u8>(), 1024) };
        let units: Vec<u16> = bytes[4..required as usize]
            .chunks_exact(2)
            .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
            .collect();
        assert_eq!(
            String::from_utf16(&units).unwrap(),
            "\\REGISTRY\\MACHINE\\SYSTEM\\CurrentControlSet\\Control\\NLS\\Language"
        );
        assert_eq!(unsafe { nt_reg_close_key(opened) }, STATUS_SUCCESS);
        assert_eq!(unsafe { nt_reg_close_key(key) }, STATUS_SUCCESS);
    }
    #[test]
    fn rename_preserves_handles_children_and_values_and_reports_collisions() {
        let _serial = REGISTRY_TEST_LOCK.lock();
        let handle = create("MutationParent", 0x1000f);
        let child = create("MutationParent\\Child", 0x1000f);
        let other = create("MutationCollision", 0x1000f);
        let value = Name::new("value");
        let data = 0x12345678u32;
        assert_eq!(
            unsafe {
                nt_set_value_key(
                    handle,
                    value.address() as _,
                    0,
                    4,
                    pointer(&data, false) as _,
                    4,
                )
            },
            STATUS_SUCCESS
        );
        let collision = Name::new("mutationcollision");
        assert_eq!(
            unsafe { nt_rename_key(handle, collision.address()) },
            STATUS_OBJECT_NAME_COLLISION
        );
        assert_eq!(
            query(handle).1,
            "MutationParent".encode_utf16().collect::<Vec<_>>()
        );
        let renamed = Name::new("MutationRenamed");
        assert_eq!(
            unsafe { nt_rename_key(handle, renamed.address()) },
            STATUS_SUCCESS
        );
        assert_eq!(
            query(handle),
            (123456789, "MutationRenamed".encode_utf16().collect())
        );
        assert_eq!(query(child).1, "Child".encode_utf16().collect::<Vec<_>>());
        let mut output = [0u64; 8];
        let mut length = 0;
        assert_eq!(
            unsafe {
                nt_query_value_key(
                    handle,
                    value.address() as _,
                    2,
                    pointer(&mut output, true) as _,
                    64,
                    pointer(&mut length, true) as _,
                )
            },
            STATUS_SUCCESS
        );
        let output = unsafe { core::slice::from_raw_parts(output.as_ptr().cast::<u8>(), 64) };
        assert_eq!(&output[12..16], &data.to_le_bytes());
        assert_eq!(unsafe { nt_delete_key(child) }, STATUS_SUCCESS);
        assert_eq!(unsafe { nt_delete_key(handle) }, STATUS_SUCCESS);
        assert_eq!(unsafe { nt_delete_key(other) }, STATUS_SUCCESS);
    }
    #[test]
    fn mutations_capture_buffers_enforce_rights_and_reject_zero_handles() {
        let _serial = REGISTRY_TEST_LOCK.lock();
        let handle = create("MutationDenied", 1);
        let name = Name::new("DeniedRename");
        assert_eq!(
            unsafe { nt_rename_key(handle, name.address()) },
            STATUS_ACCESS_DENIED
        );
        assert_eq!(
            unsafe { nt_rename_key(handle, 1usize as _) },
            STATUS_ACCESS_VIOLATION
        );
        assert_eq!(
            unsafe { nt_rename_key(core::ptr::null_mut(), name.address()) },
            STATUS_INVALID_HANDLE
        );
        let time = -123i64;
        assert_eq!(
            unsafe { nt_set_information_key(handle, 0, pointer(&time, false) as _, 8) },
            STATUS_ACCESS_DENIED
        );
        assert_eq!(
            unsafe { nt_set_information_key(handle, 0, pointer(&time, false) as _, 7) },
            STATUS_INFO_LENGTH_MISMATCH
        );
        assert_eq!(
            unsafe { nt_set_information_key(handle, 0, 1usize as _, 8) },
            STATUS_ACCESS_VIOLATION
        );
        assert_eq!(
            unsafe { nt_flush_key(core::ptr::null_mut()) },
            STATUS_INVALID_HANDLE
        );
        assert_eq!(
            unsafe { nt_reg_close_key(core::ptr::null_mut()) },
            STATUS_INVALID_HANDLE
        );
        assert_eq!(unsafe { nt_reg_close_key(handle) }, STATUS_SUCCESS);
    }
    #[test]
    fn write_time_round_trips_and_invalid_names_do_not_change_key() {
        let _serial = REGISTRY_TEST_LOCK.lock();
        let handle = create("MutationTime", 0x1000f);
        let time = -123i64;
        assert_eq!(
            unsafe { nt_set_information_key(handle, 0, pointer(&time, false) as _, 8) },
            STATUS_SUCCESS
        );
        assert_eq!(query(handle).0, time);
        for text in ["Bad\\Name", "Bad\0Name"] {
            let name = Name::new(text);
            assert_eq!(
                unsafe { nt_rename_key(handle, name.address()) },
                STATUS_OBJECT_NAME_INVALID
            );
        }
        assert_eq!(query(handle).0, time);
        assert_eq!(
            unsafe { nt_set_information_key(handle, 1, core::ptr::null_mut(), 0) },
            STATUS_NOT_SUPPORTED
        );
        assert_eq!(
            unsafe { nt_set_information_key(handle, 5, core::ptr::null_mut(), 0) },
            STATUS_INVALID_INFO_CLASS
        );
        assert_eq!(unsafe { nt_delete_key(handle) }, STATUS_SUCCESS);
    }
    #[test]
    fn granted_handle_rights_bound_reads_enumeration_and_value_mutations() {
        let _serial = REGISTRY_TEST_LOCK.lock();
        let writer = create("RightsBound", 2);
        let value = Name::new("v");
        let data = 0x12345678u32;
        assert_eq!(
            unsafe {
                nt_set_value_key(
                    writer,
                    value.address() as _,
                    0,
                    4,
                    pointer(&data, false) as _,
                    4,
                )
            },
            STATUS_SUCCESS
        );
        let mut bytes = [0u64; 16];
        let mut length = 0u32;
        let output = pointer(&mut bytes, true) as _;
        let result = pointer(&mut length, true) as _;
        assert_eq!(
            unsafe { nt_query_value_key(writer, value.address() as _, 2, output, 128, result) },
            STATUS_ACCESS_DENIED
        );
        assert_eq!(
            unsafe { nt_query_key(writer, 0, output, 128, result) },
            STATUS_ACCESS_DENIED
        );
        assert_eq!(
            unsafe { nt_enumerate_value_key(writer, 0, 2, output, 128, result) },
            STATUS_ACCESS_DENIED
        );
        assert_eq!(
            unsafe { nt_enumerate_key(writer, 0, 0, output, 128, result) },
            STATUS_ACCESS_DENIED
        );
        let reader = create("RightsBound", 1);
        assert_eq!(
            unsafe {
                nt_set_value_key(
                    reader,
                    value.address() as _,
                    0,
                    4,
                    pointer(&data, false) as _,
                    4,
                )
            },
            STATUS_ACCESS_DENIED
        );
        assert_eq!(
            unsafe { nt_delete_value_key(reader, value.address() as _) },
            STATUS_ACCESS_DENIED
        );
        assert_eq!(
            unsafe { nt_query_value_key(reader, value.address() as _, 2, output, 128, result) },
            STATUS_SUCCESS
        );
        let delete_only = create("RightsBound", 0x10000);
        assert_eq!(
            unsafe { nt_delete_value_key(delete_only, value.address() as _) },
            STATUS_ACCESS_DENIED
        );
        assert_eq!(
            unsafe { nt_delete_value_key(writer, value.address() as _) },
            STATUS_SUCCESS
        );
        assert_eq!(unsafe { nt_delete_key(delete_only) }, STATUS_SUCCESS);
    }

    #[test]
    fn value_wire_layouts_match_between_query_and_enumeration_with_exact_buffers() {
        let _serial = REGISTRY_TEST_LOCK.lock();
        let handle = create("LayoutValue", 0x1000f);
        let value = Name::new("x");
        let data = [0x11u8, 0x22, 0x33];
        assert_eq!(
            unsafe {
                nt_set_value_key(
                    handle,
                    value.address() as _,
                    0,
                    3,
                    pointer(&data, false) as _,
                    3,
                )
            },
            STATUS_SUCCESS
        );
        for (class, size) in [(0, 14usize), (1, 27), (2, 15)] {
            let mut query = [0xaau8; 64];
            let mut enumerated = [0xaau8; 64];
            let mut length = 0u32;
            // Register exactly the exposed payload; surrounding canaries catch
            // accidental stores through the old larger C layouts.
            allow(query.as_mut_ptr() as u64, size, true);
            allow(enumerated.as_mut_ptr() as u64, size, true);
            let result = pointer(&mut length, true) as _;
            assert_eq!(
                unsafe {
                    nt_query_value_key(
                        handle,
                        value.address() as _,
                        class,
                        query.as_mut_ptr() as _,
                        size as u32,
                        result,
                    )
                },
                STATUS_SUCCESS
            );
            assert_eq!(length, size as u32);
            assert_eq!(
                unsafe {
                    nt_enumerate_value_key(
                        handle,
                        0,
                        class,
                        enumerated.as_mut_ptr() as _,
                        size as u32,
                        result,
                    )
                },
                STATUS_SUCCESS
            );
            assert_eq!(length, size as u32);
            assert_eq!(query, enumerated);
            assert_eq!(&query[size..], &[0xaa; 64][size..]);
            assert_eq!(&query[..8], &[0, 0, 0, 0, 3, 0, 0, 0]);
            match class {
                0 => assert_eq!(&query[8..14], &[2, 0, 0, 0, b'x', 0]),
                1 => {
                    assert_eq!(&query[8..20], &[24, 0, 0, 0, 3, 0, 0, 0, 2, 0, 0, 0]);
                    assert_eq!(&query[20..22], &[b'x', 0]);
                    assert_eq!(&query[24..27], &data);
                }
                _ => assert_eq!(&query[8..15], &[3, 0, 0, 0, 0x11, 0x22, 0x33]),
            }
        }
        assert_eq!(unsafe { nt_delete_key(handle) }, STATUS_SUCCESS);
    }
    #[test]
    fn full_and_cached_key_information_report_live_child_and_value_statistics() {
        let _serial = REGISTRY_TEST_LOCK.lock();
        let parent = create("Statistics", 0x1000f);
        let child = create("Statistics\\Longest", 0x1000f);
        let value = Name::new("value");
        let data = [1u8, 2, 3];
        assert_eq!(
            unsafe {
                nt_set_value_key(
                    parent,
                    value.address() as _,
                    0,
                    3,
                    pointer(&data, false) as _,
                    3,
                )
            },
            STATUS_SUCCESS
        );
        let mut bytes = [0xccu8; 64];
        let mut length = 0u32;
        let address = pointer(&mut bytes, true) as _;
        let result = pointer(&mut length, true) as _;
        let word = |bytes: &[u8], at| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        assert_eq!(
            unsafe { nt_query_key(parent, 2, address, 44, result) },
            STATUS_SUCCESS
        );
        assert_eq!(length, 44);
        assert_eq!(word(&bytes, 12), u32::MAX);
        assert_eq!((word(&bytes, 20), word(&bytes, 24)), (1, 14));
        assert_eq!(
            (word(&bytes, 32), word(&bytes, 36), word(&bytes, 40)),
            (1, 10, 3)
        );
        assert_eq!(&bytes[44..], &[0xcc; 20]);
        bytes.fill(0xcc);
        assert_eq!(
            unsafe { nt_query_key(parent, 4, address, 36, result) },
            STATUS_SUCCESS
        );
        assert_eq!(length, 36);
        assert_eq!((word(&bytes, 12), word(&bytes, 16)), (1, 14));
        assert_eq!(
            (word(&bytes, 20), word(&bytes, 24), word(&bytes, 28)),
            (1, 10, 3)
        );
        assert_eq!(word(&bytes, 32), 20);
        assert_eq!(&bytes[36..], &[0xcc; 28]);
        assert_eq!(unsafe { nt_delete_key(child) }, STATUS_SUCCESS);
        assert_eq!(unsafe { nt_delete_key(parent) }, STATUS_SUCCESS);
    }

    #[test]
    fn short_information_buffers_distinguish_missing_header_from_truncated_payload() {
        let _serial = REGISTRY_TEST_LOCK.lock();
        let key = create("Partial", 0x1000f);
        let value = Name::new("v");
        let data = [1u8, 2, 3];
        assert_eq!(
            unsafe {
                nt_set_value_key(
                    key,
                    value.address() as _,
                    0,
                    3,
                    pointer(&data, false) as _,
                    3,
                )
            },
            STATUS_SUCCESS
        );
        let mut bytes = [0xccu8; 64];
        let mut result_bytes = [0u8; 5];
        // Both addresses are deliberately unaligned. Native copies must not
        // construct aligned Rust references into a caller's output region.
        allow(bytes.as_mut_ptr() as u64, 64, true);
        allow(result_bytes.as_mut_ptr() as u64, 5, true);
        let address = unsafe { bytes.as_mut_ptr().add(1) } as _;
        let result = unsafe { result_bytes.as_mut_ptr().add(1) } as _;
        assert_eq!(
            unsafe { nt_query_key(key, 1, address, 23, result) },
            STATUS_BUFFER_TOO_SMALL
        );
        assert_eq!(bytes, [0xcc; 64]);
        assert_eq!(
            u32::from_le_bytes(result_bytes[1..].try_into().unwrap()),
            38
        );
        assert_eq!(
            unsafe { nt_query_key(key, 1, address, 25, result) },
            STATUS_BUFFER_OVERFLOW
        );
        assert_eq!(&bytes[13..17], &u32::MAX.to_le_bytes());
        assert_eq!(bytes[25], b'P');
        assert_eq!(bytes[26], 0xcc);
        bytes.fill(0xcc);
        assert_eq!(
            unsafe { nt_query_value_key(key, value.address() as _, 2, address, 11, result) },
            STATUS_BUFFER_TOO_SMALL
        );
        assert_eq!(bytes, [0xcc; 64]);
        assert_eq!(
            unsafe { nt_enumerate_value_key(key, 0, 2, address, 13, result) },
            STATUS_BUFFER_OVERFLOW
        );
        assert_eq!(&bytes[9..14], &[3, 0, 0, 0, 1]);
        assert_eq!(bytes[14], 0xcc);
        assert_eq!(unsafe { nt_delete_key(key) }, STATUS_SUCCESS);
    }

    #[test]
    fn default_values_and_all_win7_value_type_tags_round_trip() {
        let _serial = REGISTRY_TEST_LOCK.lock();
        let key = create("ValueTypes", 0x1000f);
        let default = Name::new("");
        let mut output = [0u8; 16];
        let mut length = 0u32;
        let data = 0x22u8;
        for kind in (0..=11).chain([12, 0x100, 0x80000000, u32::MAX]) {
            assert_eq!(
                unsafe {
                    nt_set_value_key(
                        key,
                        default.address() as _,
                        0,
                        kind,
                        pointer(&data, false) as _,
                        1,
                    )
                },
                STATUS_SUCCESS
            );
            assert_eq!(
                unsafe {
                    nt_query_value_key(
                        key,
                        default.address() as _,
                        2,
                        pointer(&mut output, true) as _,
                        16,
                        pointer(&mut length, true) as _,
                    )
                },
                STATUS_SUCCESS
            );
            assert_eq!(u32::from_le_bytes(output[4..8].try_into().unwrap()), kind);
            assert_eq!(output[12], 0x22);
        }
        assert_eq!(
            unsafe { nt_delete_value_key(key, default.address() as _) },
            STATUS_SUCCESS
        );
        let original = Name::new("MixedCase");
        let alternate = Name::new("mixedcase");
        assert_eq!(
            unsafe {
                nt_set_value_key(
                    key,
                    original.address() as _,
                    0,
                    3,
                    pointer(&data, false) as _,
                    1,
                )
            },
            STATUS_SUCCESS
        );
        assert_eq!(
            unsafe {
                nt_set_value_key(
                    key,
                    alternate.address() as _,
                    0,
                    3,
                    core::ptr::null_mut(),
                    1,
                )
            },
            STATUS_ACCESS_VIOLATION
        );
        assert_eq!(
            unsafe { nt_delete_value_key(key, alternate.address() as _) },
            STATUS_SUCCESS
        );
        assert_eq!(unsafe { nt_delete_key(key) }, STATUS_SUCCESS);
    }
    #[test]
    fn create_and_open_map_generic_rights_preserve_disposition_and_resolve_root_handles() {
        let _serial = REGISTRY_TEST_LOCK.lock();
        init();
        let name = Name::new("GenericRoot");
        let mut attributes = ObjectAttributes::new();
        attributes.object_name = name.address() as _;
        let mut handle = core::ptr::null_mut();
        let mut action = 0u32;
        let attr = pointer(&attributes, false) as _;
        let out = pointer(&mut handle, true) as _;
        let disposition = pointer(&mut action, true) as _;
        assert_eq!(
            unsafe {
                nt_create_key(
                    out,
                    0x80000000,
                    attr,
                    0,
                    core::ptr::null_mut(),
                    0,
                    disposition,
                )
            },
            STATUS_SUCCESS
        );
        assert_eq!(action, 1);
        assert_eq!(
            unsafe {
                nt_create_key(
                    out,
                    0x80000000,
                    attr,
                    0,
                    core::ptr::null_mut(),
                    0,
                    disposition,
                )
            },
            STATUS_SUCCESS
        );
        assert_eq!(action, 2);
        let value = Name::new("v");
        assert_eq!(
            unsafe {
                nt_set_value_key(handle, value.address() as _, 0, 3, core::ptr::null_mut(), 0)
            },
            STATUS_ACCESS_DENIED
        );
        let parent = handle;
        let relative_name = Name::new("Child");
        attributes.root_directory = parent;
        attributes.object_name = relative_name.address() as _;
        let attr = pointer(&attributes, false) as _;
        assert_eq!(
            unsafe {
                nt_create_key(
                    out,
                    0x10000000,
                    attr,
                    0,
                    core::ptr::null_mut(),
                    0,
                    disposition,
                )
            },
            STATUS_SUCCESS
        );
        let child = handle;
        assert_eq!(query(child).1, "Child".encode_utf16().collect::<Vec<_>>());
        assert_eq!(
            unsafe { nt_open_key(out, 0x80000000, attr) },
            STATUS_SUCCESS
        );
        assert_eq!(query(handle).1, query(child).1);
        assert_eq!(unsafe { nt_reg_close_key(handle) }, STATUS_SUCCESS);
        assert_eq!(unsafe { nt_delete_key(child) }, STATUS_SUCCESS);
        attributes.root_directory = core::ptr::null_mut();
        attributes.object_name = name.address() as _;
        assert_eq!(
            unsafe { nt_open_key(out, 0x10000000, pointer(&attributes, false) as _) },
            STATUS_SUCCESS
        );
        assert_eq!(unsafe { nt_delete_key(handle) }, STATUS_SUCCESS);
    }
    #[test]
    fn full_key_names_follow_ancestor_renames_and_allow_zero_access_handles() {
        let _serial = REGISTRY_TEST_LOCK.lock();
        let parent = create("NameParent", 0x1000f);
        let child = create("NameParent\\Leaf", 0);
        let renamed = Name::new("NameRenamed");
        assert_eq!(
            unsafe { nt_rename_key(parent, renamed.address()) },
            STATUS_SUCCESS
        );
        let expected: Vec<u16> = "\\REGISTRY\\NameRenamed\\Leaf".encode_utf16().collect();
        let mut buffer = [0xccu8; 160];
        let mut needed = 0;
        let out = pointer(&mut buffer, true) as _;
        let result = pointer(&mut needed, true) as _;
        assert_eq!(
            unsafe { nt_query_key(child, 3, out, 3, result) },
            STATUS_BUFFER_TOO_SMALL
        );
        assert!(buffer.iter().all(|byte| *byte == 0xcc));
        assert_eq!(needed as usize, 4 + expected.len() * 2);
        assert_eq!(
            unsafe { nt_query_key(child, 3, out, 5, result) },
            STATUS_BUFFER_OVERFLOW
        );
        assert_eq!(buffer[4], b'\\');
        assert_eq!(buffer[5], 0xcc);
        assert_eq!(
            unsafe { nt_query_key(child, 3, out, needed, result) },
            STATUS_SUCCESS
        );
        let actual: Vec<u16> = buffer[4..needed as usize]
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        assert_eq!(actual, expected);
        assert_eq!(buffer[needed as usize], 0xcc);
        assert_eq!(unsafe { nt_reg_close_key(child) }, STATUS_SUCCESS);
        let cleanup = create("NameRenamed\\Leaf", 0x10000);
        assert_eq!(unsafe { nt_delete_key(cleanup) }, STATUS_SUCCESS);
        assert_eq!(unsafe { nt_delete_key(parent) }, STATUS_SUCCESS);
    }

    #[test]
    fn aligned_value_information_uses_native_offsets_and_bounds_every_copy() {
        let _serial = REGISTRY_TEST_LOCK.lock();
        let key = create("AlignedValues", 0x10003);
        let value = Name::new("ABC");
        let data = [0x11u8, 0x22, 0x33, 0x44, 0x55];
        assert_eq!(
            unsafe {
                nt_set_value_key(
                    key,
                    value.address() as _,
                    0,
                    3,
                    pointer(&data, false) as _,
                    data.len() as u32,
                )
            },
            STATUS_SUCCESS
        );
        for (class, header, data_offset, total) in [(3, 20, 32, 37), (4, 8, 8, 13)] {
            let mut bytes = [0xccu8; 64];
            let out = (pointer(&mut bytes, true) + 1) as PVOID;
            let mut needed = 0u32;
            let result = pointer(&mut needed, true) as _;
            assert_eq!(
                unsafe {
                    nt_query_value_key(key, value.address() as _, class, out, header - 1, result)
                },
                STATUS_BUFFER_TOO_SMALL
            );
            assert!(bytes.iter().all(|b| *b == 0xcc));
            assert_eq!(needed, total);
            assert_eq!(
                unsafe {
                    nt_query_value_key(key, value.address() as _, class, out, total - 1, result)
                },
                STATUS_BUFFER_OVERFLOW
            );
            assert_eq!(bytes[0], 0xcc);
            assert_eq!(bytes[total as usize], 0xcc);
            assert_eq!(
                unsafe { nt_enumerate_value_key(key, 0, class, out, total, result) },
                STATUS_SUCCESS
            );
            let captured = &bytes[1..1 + total as usize];
            if class == 3 {
                assert_eq!(
                    u32::from_le_bytes(captured[8..12].try_into().unwrap()),
                    data_offset
                );
                assert!(captured[26..32].iter().all(|b| *b == 0));
            } else {
                assert_eq!(u32::from_le_bytes(captured[..4].try_into().unwrap()), 3);
                assert_eq!(u32::from_le_bytes(captured[4..8].try_into().unwrap()), 5);
            }
            assert_eq!(&captured[data_offset as usize..], &data);
            assert_eq!(bytes[1 + total as usize], 0xcc);
        }
        assert_eq!(unsafe { nt_delete_key(key) }, STATUS_SUCCESS);
    }
    #[test]
    fn full_child_table_rejects_create_without_a_handle_or_pool_allocation() {
        let _serial = REGISTRY_TEST_LOCK.lock();
        let parent = create("CapacityParent", 0x1000f);
        let mut children = Vec::new();
        for index in 0..32 {
            children.push(create(&format!("CapacityParent\\K{index}"), 0x1000f));
        }
        let name = Name::new("CapacityParent\\Overflow");
        let mut attributes = ObjectAttributes::new();
        attributes.object_name = name.address() as _;
        let mut handle = 0xccccusize as HANDLE;
        let mut action = 0xccccu32;
        let before = super::mm::pool::LIVE.load(core::sync::atomic::Ordering::Relaxed);
        assert_eq!(
            unsafe {
                nt_create_key(
                    pointer(&mut handle, true) as _,
                    0x1000f,
                    pointer(&attributes, false) as _,
                    0,
                    core::ptr::null_mut(),
                    0,
                    pointer(&mut action, true) as _,
                )
            },
            STATUS_INSUFFICIENT_RESOURCES
        );
        assert_eq!(handle as usize, 0xcccc);
        assert_eq!(action, 0xcccc);
        assert_eq!(
            super::mm::pool::LIVE.load(core::sync::atomic::Ordering::Relaxed),
            before
        );
        assert_eq!(
            unsafe {
                nt_open_key(
                    pointer(&mut handle, true) as _,
                    1,
                    pointer(&attributes, false) as _,
                )
            },
            STATUS_OBJECT_NAME_NOT_FOUND
        );
        for child in children {
            assert_eq!(unsafe { nt_delete_key(child) }, STATUS_SUCCESS);
        }
        let recovered = create("CapacityParent\\Overflow", 0x1000f);
        assert_eq!(unsafe { nt_delete_key(recovered) }, STATUS_SUCCESS);
        assert_eq!(unsafe { nt_delete_key(parent) }, STATUS_SUCCESS);
    }
}
