//! Kernel-owned install UI language persistence over the live registry tree.
//!
//! This is a child of `registry`, so it uses the same tree lifetime lock and
//! mutation notifications as native registry services. It never creates keys.
use super::*;

const INSTALL_NAME: &[u16] = &[
    73, 110, 115, 116, 97, 108, 108, 76, 97, 110, 103, 117, 97, 103, 101,
];
const PREVIOUS_NAME: &[u16] = &[
    80, 114, 101, 118, 105, 111, 117, 115, 73, 110, 115, 116, 97, 108, 108, 76, 97, 110, 103, 117,
    97, 103, 101,
];
const PATH: &[&[u8]] = &[
    b"MACHINE",
    b"SYSTEM",
    b"CurrentControlSet",
    b"Control",
    b"NLS",
    b"Language",
];

/// Write the installed language using the kernel registry namespace. Missing
/// installation keys are returned as a failure, never silently fabricated.
///
/// The trusted kernel caller bypasses user handle-table slots and user DACL
/// checks, as does the original ZwOpenKey without OBJ_FORCE_ACCESS_CHECK.
/// The current registry provider is volatile; this does not flush a disk hive.
pub fn persist(language: u16) -> NTSTATUS {
    let _operation = REGISTRY_OPERATIONS.lock();
    let root = REGISTRY_ROOT.lock();
    let Some(mut current) = *root else {
        return STATUS_OBJECT_NAME_NOT_FOUND;
    };
    // REGISTRY_ROOT is already the named \REGISTRY object, not its parent.
    // Consume the object-manager prefix here rather than seeking a duplicate
    // REGISTRY child. This is the absolute path used by the original image.
    // SAFETY: operation/root guards pin the root for this name check.
    let Some(root_key) = (unsafe { current.as_ref() }) else {
        return STATUS_OBJECT_NAME_NOT_FOUND;
    };
    if !ascii_name_matches(&root_key.name[..root_key.name_len as usize], b"\\REGISTRY") {
        return STATUS_OBJECT_NAME_NOT_FOUND;
    }
    for component in PATH {
        // SAFETY: operation and root locks pin all traversed tree nodes.
        let Some(next) = (unsafe { find_ascii_child(current, component) }) else {
            return STATUS_OBJECT_NAME_NOT_FOUND;
        };
        current = next;
    }
    // SAFETY: the same locks serialize mutation and prevent key deletion.
    unsafe { persist_at_key(current, language) }
}

/// # Safety
/// `parent` is pinned by the registry operation lock for the entire traversal.
unsafe fn find_ascii_child(parent: *mut RegistryKey, name: &[u8]) -> Option<*mut RegistryKey> {
    // SAFETY: parent is live under the caller's operation lock.
    let parent = unsafe { parent.as_ref() }?;
    for child in parent.children[..parent.child_count].iter().flatten() {
        // SAFETY: registry children remain live under the same operation lock.
        let child_ref = unsafe { child.as_ref() }?;
        let key = child_ref.name.get(..child_ref.name_len as usize)?;
        if ascii_name_matches(key, name) {
            return Some(*child);
        }
    }
    None
}

fn ascii_name_matches(name: &[u16], ascii: &[u8]) -> bool {
    name.len() == ascii.len()
        && name
            .iter()
            .zip(ascii)
            .all(|(unit, byte)| same_value_unit(*unit, u16::from(*byte)))
}

/// # Safety
/// The caller owns the key exclusively, or holds the registry operation lock.
unsafe fn persist_at_key(key: *mut RegistryKey, language: u16) -> NTSTATUS {
    let mut previous = [0u8; MAX_VALUE_DATA_SIZE];
    // SAFETY: exclusive key ownership covers the value lookup and copy.
    let previous_size = unsafe {
        get_value(key, INSTALL_NAME).and_then(|(_, index)| {
            let value = (*key).values.get(index)?.as_ref()?;
            let size = usize::try_from(value.data_size).ok()?;
            let bytes = value.data.get(..size)?;
            previous[..size].copy_from_slice(bytes);
            Some(size)
        })
    };
    if let Some(size) = previous_size {
        // The old value's type and raw bytes are intentionally independent:
        // Win7 writes these exact bytes as REG_SZ, even for a non-string old type.
        // Failure to write PreviousInstallLanguage does not cancel final write.
        // SAFETY: caller retains exclusive key ownership.
        let _ = unsafe { write_string(key, PREVIOUS_NAME, &previous[..size]) };
    }
    let mut bytes = [0u8; 10];
    for (index, shift) in [12, 8, 4, 0].into_iter().enumerate() {
        let nibble = ((language >> shift) & 15) as u8;
        let digit = if nibble < 10 {
            b'0' + nibble
        } else {
            b'A' + nibble - 10
        };
        bytes[index * 2] = digit;
    }
    // SAFETY: caller retains exclusive key ownership; fixed new data fits.
    unsafe { write_string(key, INSTALL_NAME, &bytes) }
}

/// # Safety
/// The caller serializes writes and pins the key for notification traversal.
unsafe fn write_string(key: *mut RegistryKey, name: &[u16], bytes: &[u8]) -> NTSTATUS {
    // SAFETY: caller guarantees the key's mutation and lifetime contract.
    if unsafe { set_value(key, name, ValueType::String, bytes) } {
        // SAFETY: successful set_value required and retained a valid key.
        unsafe { (*key).last_write_time = crate::ke::time::get_system_time() as i64 };
        notify_registry_change(key);
        STATUS_SUCCESS
    } else {
        // Preserve the live registry provider's NtSetValueKey failure mapping.
        STATUS_INVALID_PARAMETER
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::boxed::Box;
    use alloc::vec::Vec;

    fn read(key: &RegistryKey, name: &[u16]) -> Option<(ValueType, Vec<u8>)> {
        key.values.iter().flatten().find_map(|value| {
            let stored = &value.name[..value.name_len as usize];
            (stored == name).then(|| {
                (
                    value.value_type,
                    value.data[..value.data_size as usize].to_vec(),
                )
            })
        })
    }

    #[test]
    fn install_language_persists_hex_and_raw_previous_bytes() {
        let _test = REGISTRY_TEST_LOCK.lock();
        let mut key = Box::new(RegistryKey::new(&[]));
        let raw = [0x11, 0, 0xff, 0x31, 0x42];
        // SAFETY: this test exclusively owns the boxed key.
        assert!(unsafe { set_value(&mut *key, INSTALL_NAME, ValueType::Binary, &raw) });
        assert_eq!(unsafe { persist_at_key(&mut *key, 0xabcd) }, STATUS_SUCCESS);
        assert_eq!(
            read(&key, PREVIOUS_NAME),
            Some((ValueType::String, raw.to_vec()))
        );
        assert_eq!(
            read(&key, INSTALL_NAME),
            Some((ValueType::String, b"A\0B\0C\0D\0\0\0".to_vec()))
        );
    }

    #[test]
    fn install_language_missing_previous_and_all_hex_edges() {
        let _test = REGISTRY_TEST_LOCK.lock();
        for (language, expected) in [
            (0, b"0\x000\x000\x000\x00\x00\x00".as_slice()),
            (0xffff, b"F\0F\0F\0F\0\0\0".as_slice()),
            (0x409, b"0\x004\x000\x009\x00\x00\x00".as_slice()),
        ] {
            let mut key = Box::new(RegistryKey::new(&[]));
            // SAFETY: this test exclusively owns the boxed key.
            assert_eq!(
                unsafe { persist_at_key(&mut *key, language) },
                STATUS_SUCCESS
            );
            assert_eq!(read(&key, PREVIOUS_NAME), None);
            assert_eq!(
                read(&key, INSTALL_NAME),
                Some((ValueType::String, expected.to_vec()))
            );
        }
    }

    #[test]
    fn full_value_table_ignores_previous_failure_but_updates_existing_install() {
        let _test = REGISTRY_TEST_LOCK.lock();
        let mut key = Box::new(RegistryKey::new(&[]));
        // SAFETY: this test exclusively owns the boxed key.
        unsafe {
            assert!(set_value(
                &mut *key,
                INSTALL_NAME,
                ValueType::String,
                &[1, 2]
            ));
            for unit in 1..64 {
                assert!(set_value(&mut *key, &[unit], ValueType::Binary, &[]));
            }
            assert_eq!(persist_at_key(&mut *key, 0x804), STATUS_SUCCESS);
        }
        assert_eq!(read(&key, PREVIOUS_NAME), None);
        assert_eq!(
            read(&key, INSTALL_NAME),
            Some((ValueType::String, b"0\x008\x000\x004\x00\x00\x00".to_vec()))
        );
    }

    #[test]
    fn full_value_table_reports_final_install_creation_failure() {
        let _test = REGISTRY_TEST_LOCK.lock();
        let mut key = Box::new(RegistryKey::new(&[]));
        // SAFETY: this test exclusively owns the boxed key.
        unsafe {
            for unit in 1..=64 {
                assert!(set_value(&mut *key, &[unit], ValueType::Binary, &[]));
            }
            assert_eq!(persist_at_key(&mut *key, 0x804), STATUS_INVALID_PARAMETER);
        }
        assert_eq!(read(&key, INSTALL_NAME), None);
        assert_eq!(read(&key, PREVIOUS_NAME), None);
    }

    #[test]
    fn install_language_path_component_is_case_insensitive_without_aliasing_root() {
        let mut parent = Box::new(RegistryKey::new(&[]));
        let mut child = Box::new(RegistryKey::new(&[110, 76, 115]));
        parent.children[0] = Some(&mut *child);
        parent.child_count = 1;
        // SAFETY: both boxes are exclusively owned and child remains live.
        unsafe {
            assert_eq!(
                find_ascii_child(&mut *parent, b"NLS"),
                Some(&mut *child as *mut _)
            );
            assert_eq!(find_ascii_child(&mut *parent, b"REGISTRY"), None);
        }
    }
}

#[cfg(test)]
mod namespace_tests {
    use super::*;
    use alloc::{boxed::Box, vec::Vec};

    struct RestoreRoot(Option<*mut RegistryKey>);
    impl Drop for RestoreRoot {
        fn drop(&mut self) {
            let _operation = REGISTRY_OPERATIONS.lock();
            *REGISTRY_ROOT.lock() = self.0;
        }
    }

    #[test]
    fn install_language_persist_uses_canonical_existing_absolute_path() {
        let _test = REGISTRY_TEST_LOCK.lock();
        let mut nodes: Vec<Box<RegistryKey>> = core::iter::once(b"\\REGISTRY".as_slice())
            .chain(PATH.iter().copied())
            .map(|name| {
                let units: Vec<u16> = name.iter().map(|byte| u16::from(*byte)).collect();
                Box::new(RegistryKey::new(&units))
            })
            .collect();
        for index in 0..nodes.len() - 1 {
            let child = &mut *nodes[index + 1] as *mut RegistryKey;
            let parent = &mut *nodes[index] as *mut RegistryKey;
            nodes[index].children[0] = Some(child);
            nodes[index].child_count = 1;
            nodes[index + 1].parent = parent;
        }
        let _restore = {
            let _operation = REGISTRY_OPERATIONS.lock();
            let old = REGISTRY_ROOT.lock().replace(&mut *nodes[0]);
            RestoreRoot(old)
        };
        assert_eq!(persist(0xabcd), STATUS_SUCCESS);
        let leaf = nodes.last().unwrap();
        let install = leaf
            .values
            .iter()
            .flatten()
            .find(|value| &value.name[..value.name_len as usize] == INSTALL_NAME)
            .unwrap();
        assert_eq!(&install.data[..10], b"A\0B\0C\0D\0\0\0");
        // Removing a real path component must fail without fabricating it or
        // falling back to a SYSTEM sibling outside MACHINE.
        let index = nodes.len() - 2;
        nodes[index].children[0] = None;
        nodes[index].child_count = 0;
        assert_eq!(persist(0x804), STATUS_OBJECT_NAME_NOT_FOUND);
        assert_eq!(nodes[index].child_count, 0);
        assert_eq!(nodes[0].child_count, 1);
    }
}
