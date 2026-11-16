//! ntdll Registry API
//
//! Implements Windows NT registry functions for in-memory registry storage.

use crate::kprintln;
use crate::libs::ntdll::status::{
    STATUS_ACCESS_DENIED, STATUS_ACCESS_VIOLATION, STATUS_BUFFER_TOO_SMALL, STATUS_INVALID_HANDLE,
    STATUS_INVALID_PARAMETER, STATUS_OBJECT_NAME_NOT_FOUND, STATUS_SUCCESS,
};
use crate::libs::ntdll::types::{ObjectAttributes, UnicodeString, HANDLE, NTSTATUS, PVOID};
use crate::mm::user_copy::{
    copy_from_user, probe_user_write, read_user_object, read_user_unicode_string, write_user_object,
};
use core::ptr::null_mut;
use core::sync::atomic::{AtomicU32, Ordering};

/// Backslash character in UTF-16 encoding (0x5C).
const BACKSLASH_U16: u16 = 0x5C;

// KEY_* access rights (winnt.h).
const KEY_QUERY_VALUE: u32 = 0x0001;
const KEY_SET_VALUE: u32 = 0x0002;
const KEY_CREATE_SUB_KEY: u32 = 0x0004;
const KEY_ENUMERATE_SUB_KEYS: u32 = 0x0008;
const DELETE_RIGHT: u32 = 0x0001_0000;

/// Maximum number of simultaneously open key handles.
const MAX_KEY_HANDLES: usize = 256;

/// Maximum length of a registry value name (in UTF-16 characters).
const MAX_VALUE_NAME_LEN: usize = 128;

/// Maximum size of registry value data (in bytes).
const MAX_VALUE_DATA_SIZE: usize = 256;

/// Registry key types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyType {
    Root = 0,
    Directory = 1,
    SymbolicLink = 2,
}

/// Registry type tags are opaque DWORD values. The named REG_* constants do
/// not limit which values NtSetValueKey may preserve and return to callers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct ValueType(pub u32);

#[allow(non_upper_case_globals)]
impl ValueType {
    pub const None: Self = Self(0);
    pub const String: Self = Self(1);
    pub const ExpandString: Self = Self(2);
    pub const Binary: Self = Self(3);
    pub const DWord: Self = Self(4);
    pub const DWordBigEndian: Self = Self(5);
    pub const Link: Self = Self(6);
    pub const MultiString: Self = Self(7);
    pub const ResourceList: Self = Self(8);
    pub const FullResourceDescriptor: Self = Self(9);
    pub const ResourceRequirementsList: Self = Self(10);
    pub const QWord: Self = Self(11);
}

/// Registry value with fixed-size buffers.
///
/// SECURITY: The `name` and `data` fields use fixed-size arrays.
/// All API functions validate that input lengths do not exceed
/// `MAX_VALUE_NAME_LEN` (128) and `MAX_VALUE_DATA_SIZE` (256).
/// Data exceeding these limits is rejected rather than silently truncated.
pub struct RegValue {
    pub name: [u16; MAX_VALUE_NAME_LEN],
    pub name_len: u16,
    pub value_type: ValueType,
    pub data: [u8; MAX_VALUE_DATA_SIZE],
    pub data_size: u32,
}

impl RegValue {
    /// create a new RegValue with bounds checking.
    ///
    /// If `name.len() > MAX_VALUE_NAME_LEN` or `data.len() > MAX_VALUE_DATA_SIZE`,
    /// returns `None` instead of silently truncating. Callers should validate
    /// lengths before constructing values.
    pub fn new(name: &[u16], vtype: ValueType, data: &[u8]) -> Option<Self> {
        // Reject oversized input rather than truncating silently.
        // This prevents data loss and makes security issues visible.
        if name.len() > MAX_VALUE_NAME_LEN {
            return None;
        }
        if data.len() > MAX_VALUE_DATA_SIZE {
            return None;
        }

        let mut result = Self {
            name: [0; MAX_VALUE_NAME_LEN],
            name_len: name.len() as u16,
            value_type: vtype,
            data: [0; MAX_VALUE_DATA_SIZE],
            data_size: data.len() as u32,
        };

        result.name[..name.len()].copy_from_slice(name);
        result.data[..data.len()].copy_from_slice(data);

        Some(result)
    }

    /// create a new RegValue with truncation (for internal use where
    /// the caller has already validated or knowingly accepts truncation).
    ///
    /// This is a fallback for scenarios where truncation is acceptable
    /// (e.g., the caller knows the data is bounded).
    #[doc(hidden)]
    pub fn new_truncate(name: &[u16], vtype: ValueType, data: &[u8]) -> Self {
        let mut result = Self {
            name: [0; MAX_VALUE_NAME_LEN],
            name_len: name.len().min(MAX_VALUE_NAME_LEN) as u16,
            value_type: vtype,
            data: [0; MAX_VALUE_DATA_SIZE],
            data_size: data.len().min(MAX_VALUE_DATA_SIZE) as u32,
        };

        let name_len = name.len().min(MAX_VALUE_NAME_LEN);
        result.name[..name_len].copy_from_slice(&name[..name_len]);

        let data_len = data.len().min(MAX_VALUE_DATA_SIZE);
        result.data[..data_len].copy_from_slice(&data[..data_len]);

        result
    }
}

/// Registry key structure
pub struct RegistryKey {
    pub name: [u16; 256],
    pub name_len: u16,
    pub key_type: KeyType,
    pub last_write_time: i64,
    pub values: [Option<RegValue>; 64],
    pub value_count: usize,
    pub children: [Option<*mut RegistryKey>; 32],
    pub child_count: usize,
    pub parent: *mut RegistryKey,
    pub security: *const crate::se::seaccess::SecurityDescriptor,
}

impl RegistryKey {
    pub fn new(name: &[u16]) -> Self {
        let mut result = Self {
            name: [0; 256],
            name_len: 0,
            key_type: KeyType::Directory,
            last_write_time: 0,
            values: [const { None }; 64],
            value_count: 0,
            children: [const { None }; 32],
            child_count: 0,
            parent: null_mut(),
            security: core::ptr::null(),
        };

        let len = name.len().min(255);
        result.name[..len].copy_from_slice(&name[..len]);
        result.name_len = len as u16;

        result
    }
}

// Serialize native tree access before taking root, handle or notification locks.
// This keeps rename/delete and readers from retaining freed key pointers.
static REGISTRY_OPERATIONS: spin::Spinlock<()> = spin::Spinlock::new(());

/// Global registry state
static REGISTRY_ROOT: spin::Spinlock<Option<*mut RegistryKey>> = spin::Spinlock::new(None);

/// Key handle table — 256 slots, each holding an optional entry.
/// handle = slot_index + 1 (1-based, so 0 is an invalid handle).
/// Used by all Nt*Key functions to resolve HANDLE -> *mut RegistryKey.
static KEY_HANDLE_TABLE: spin::Spinlock<[Option<KeyHandleEntry>; MAX_KEY_HANDLES]> =
    spin::Spinlock::new([const { None }; MAX_KEY_HANDLES]);

const MAX_KEY_NOTIFICATIONS: usize = 32;

struct RegistryNotification {
    key_ptr: *mut RegistryKey,
    event_handle: HANDLE,
    watch_tree: bool,
}

static KEY_NOTIFICATIONS: spin::Spinlock<[Option<RegistryNotification>; MAX_KEY_NOTIFICATIONS]> =
    spin::Spinlock::new([const { None }; MAX_KEY_NOTIFICATIONS]);

/// Round-robin pointer for handle allocation. After a full scan fails,
/// the next allocation starts from this position to avoid always scanning
/// from slot 0. Only the high 32 bits of the slot index are stored so
/// this fits in AtomicU32 without separate LAST_SLOT.
static LAST_HANDLE_SLOT: AtomicU32 = AtomicU32::new(0);

pub struct KeyHandleEntry {
    pub key_ptr: *mut RegistryKey,
    pub ref_count: u32,
    /// Access rights the handle was opened with. Zero means "no access".
    pub desired_access: u32,
}

impl KeyHandleEntry {
    pub fn new(key: *mut RegistryKey) -> Self {
        Self {
            key_ptr: key,
            ref_count: 1,
            desired_access: 0,
        }
    }
}

mod spin {
    pub struct Spinlock<T> {
        data: core::cell::UnsafeCell<T>,
        flag: core::sync::atomic::AtomicU32,
    }

    unsafe impl<T> Send for Spinlock<T> {}
    unsafe impl<T> Sync for Spinlock<T> {}

    impl<T> Spinlock<T> {
        pub const fn new(data: T) -> Self {
            Self {
                data: core::cell::UnsafeCell::new(data),
                flag: core::sync::atomic::AtomicU32::new(0),
            }
        }
    }

    impl<T> Spinlock<T> {
        pub fn lock(&self) -> SpinlockGuard<'_, T> {
            while self
                .flag
                .compare_exchange(
                    0,
                    1,
                    core::sync::atomic::Ordering::SeqCst,
                    core::sync::atomic::Ordering::SeqCst,
                )
                .is_err()
            {
                core::hint::spin_loop();
            }
            SpinlockGuard { lock: self }
        }
    }

    pub struct SpinlockGuard<'a, T: 'a> {
        lock: &'a Spinlock<T>,
    }

    impl<'a, T> core::ops::Deref for SpinlockGuard<'a, T> {
        type Target = T;
        fn deref(&self) -> &T {
            unsafe { &*self.lock.data.get() }
        }
    }

    impl<'a, T> core::ops::DerefMut for SpinlockGuard<'a, T> {
        fn deref_mut(&mut self) -> &mut T {
            unsafe { &mut *self.lock.data.get() }
        }
    }

    impl<'a, T> Drop for SpinlockGuard<'a, T> {
        fn drop(&mut self) {
            self.lock
                .flag
                .store(0, core::sync::atomic::Ordering::SeqCst);
        }
    }
}

/// Allocate a handle in the key handle table using round-robin search.
///
/// Returns the slot index (0..255) if a free slot is found, or `None`
/// if the table is full. This implements round-robin by starting the
/// scan from `LAST_HANDLE_SLOT + 1` modulo 256.
fn allocate_handle_slot(key: *mut RegistryKey, desired_access: u32) -> Option<usize> {
    let start = LAST_HANDLE_SLOT.load(Ordering::Relaxed) as usize;
    let mut handle_table = KEY_HANDLE_TABLE.lock();

    // Round-robin: scan from (last_slot + 1) to (last_slot - 1), wrapping at 256.
    // We do at most two passes to cover all 256 slots.
    for offset in 0..MAX_KEY_HANDLES {
        let idx = (start + offset + 1) % MAX_KEY_HANDLES;
        if handle_table[idx].is_none() {
            let mut entry = KeyHandleEntry::new(key);
            entry.desired_access = desired_access;
            handle_table[idx] = Some(entry);
            // Update round-robin pointer to the slot we just used.
            LAST_HANDLE_SLOT.store(idx as u32, Ordering::Relaxed);
            return Some(idx);
        }
    }

    None
}

/// Deallocate a handle slot. Called by nt_reg_close_key.
pub fn deallocate_handle_slot(slot_index: usize) {
    if slot_index < MAX_KEY_HANDLES {
        let mut handle_table = KEY_HANDLE_TABLE.lock();
        handle_table[slot_index] = None;
    }
}

fn registry_key_is_descendant_or_same(key: *mut RegistryKey, ancestor: *mut RegistryKey) -> bool {
    if key.is_null() || ancestor.is_null() {
        return false;
    }
    let mut current = key;
    while !current.is_null() {
        if current == ancestor {
            return true;
        }
        current = unsafe { (*current).parent };
    }
    false
}

fn notify_registry_change(changed_key: *mut RegistryKey) {
    let mut notifications = KEY_NOTIFICATIONS.lock();
    for slot in notifications.iter_mut() {
        let Some(notification) = slot else { continue };
        let matches = notification.watch_tree
            && registry_key_is_descendant_or_same(changed_key, notification.key_ptr)
            || (!notification.watch_tree && changed_key == notification.key_ptr);
        if matches {
            let event_handle = notification.event_handle;
            unsafe {
                let _ = crate::libs::ntdll::sync::nt_set_event(event_handle, core::ptr::null_mut());
            }
            *slot = None;
        }
    }
}

/// Initialize the registry with a root key.
pub fn init() {
    let mut root = REGISTRY_ROOT.lock();
    if root.is_none() {
        let root_key = crate::mm::pool::allocate(
            crate::mm::pool::PoolType::NonPaged,
            core::mem::size_of::<RegistryKey>(),
        ) as *mut RegistryKey;

        if !root_key.is_null() {
            unsafe {
                // Zero out the entire structure first
                let size = core::mem::size_of::<RegistryKey>();
                let ptr = root_key as *mut u8;
                for i in 0..size {
                    core::ptr::write(ptr.add(i), 0);
                }

                // Set name characters individually
                (*root_key).name[0] = b'\\' as u16;
                (*root_key).name[1] = b'R' as u16;
                (*root_key).name[2] = b'E' as u16;
                (*root_key).name[3] = b'G' as u16;
                (*root_key).name[4] = b'I' as u16;
                (*root_key).name[5] = b'S' as u16;
                (*root_key).name[6] = b'T' as u16;
                (*root_key).name[7] = b'R' as u16;
                (*root_key).name[8] = b'Y' as u16;
                (*root_key).name_len = 9;
                (*root_key).key_type = KeyType::Root;
                (*root_key).parent = null_mut();
                (*root_key).security = registry_default_security();
            }
            *root = Some(root_key);
        }
    }
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// create a subkey under the given parent.
pub unsafe fn create_subkey(parent: *mut RegistryKey, name: &[u16]) -> Option<*mut RegistryKey> {
    // Check the insertion slot before allocation. Returning an unlinked key
    // would expose a successful create that could never be opened by name.
    // SAFETY: the caller pins the parent and serializes changes to its children.
    if name.is_empty()
        || name.len() > 255
        || name.iter().any(|unit| *unit == 0 || *unit == BACKSLASH_U16)
        || (!parent.is_null() && unsafe { (*parent).child_count >= (*parent).children.len() })
    {
        return None;
    }
    let key_ptr = crate::mm::pool::allocate(
        crate::mm::pool::PoolType::NonPaged,
        core::mem::size_of::<RegistryKey>(),
    ) as *mut RegistryKey;

    if !key_ptr.is_null() {
        unsafe {
            // Zero out the entire structure
            for i in 0..256 {
                (*key_ptr).name[i] = 0;
            }
            // Copy name one element at a time (avoid copy_from_slice)
            let len = name.len();
            (&mut (*key_ptr).name)[..len].copy_from_slice(&name[..len]);
            (*key_ptr).name_len = len as u16;
            (*key_ptr).key_type = KeyType::Directory;
            (*key_ptr).last_write_time = crate::ke::time::get_system_time() as i64;
            (*key_ptr).parent = parent;
            (*key_ptr).value_count = 0;
            (*key_ptr).child_count = 0;
            (*key_ptr).security = if parent.is_null() {
                registry_default_security()
            } else {
                (*parent).security
            };

            // Initialize arrays
            for i in 0..64 {
                (*key_ptr).values[i] = None;
            }
            for i in 0..32 {
                (*key_ptr).children[i] = None;
            }

            if !parent.is_null() && (*parent).child_count < 32 {
                let idx = (*parent).child_count;
                (*parent).children[idx] = Some(key_ptr);
                (*parent).child_count += 1;
            }
        }
        return Some(key_ptr);
    }
    None
}

/// Create the shared default DACL for the in-memory registry shadow tree.
fn registry_default_security() -> *const crate::se::seaccess::SecurityDescriptor {
    unsafe {
        use crate::se::acl::{AceFlags, Acl};
        use crate::se::seaccess::{SecurityDescriptor, ACCESS_GENERIC_ALL, ACCESS_GENERIC_READ};
        use crate::se::sid::{sid, WellKnownSid};

        let acl_ptr = crate::mm::pool::allocate(
            crate::mm::pool::PoolType::NonPaged,
            core::mem::size_of::<Acl>(),
        ) as *mut Acl;
        let sd_ptr = crate::mm::pool::allocate(
            crate::mm::pool::PoolType::NonPaged,
            core::mem::size_of::<SecurityDescriptor>(),
        ) as *mut SecurityDescriptor;
        if acl_ptr.is_null() || sd_ptr.is_null() {
            if !acl_ptr.is_null() {
                crate::mm::pool::free(acl_ptr as *mut u8);
            }
            if !sd_ptr.is_null() {
                crate::mm::pool::free(sd_ptr as *mut u8);
            }
            return core::ptr::null();
        }

        {
            core::ptr::write(acl_ptr, Acl::new());
            let acl = &mut *acl_ptr;
            if !acl.add_access_allowed(
                &sid::well_known(WellKnownSid::LocalSystem),
                ACCESS_GENERIC_ALL,
                AceFlags::NONE,
            ) || !acl.add_access_allowed(
                &sid::well_known(WellKnownSid::Administrators),
                ACCESS_GENERIC_ALL,
                AceFlags::NONE,
            ) || !acl.add_access_allowed(
                &sid::well_known(WellKnownSid::Users),
                ACCESS_GENERIC_READ,
                AceFlags::NONE,
            ) {
                crate::mm::pool::free(acl_ptr as *mut u8);
                crate::mm::pool::free(sd_ptr as *mut u8);
                return core::ptr::null();
            }

            core::ptr::write(
                sd_ptr,
                SecurityDescriptor {
                    revision: 1,
                    sbz1: 0,
                    control: 0x8004,
                    owner: sid::well_known(WellKnownSid::Administrators),
                    group: sid::well_known(WellKnownSid::Administrators),
                    sacl: core::ptr::null(),
                    dacl: acl_ptr,
                },
            );
        }
        sd_ptr
    }
}

/// Enforce the key object's DACL with the caller's process token.
fn registry_access_allowed(key: *mut RegistryKey, desired_access: u32) -> bool {
    unsafe {
        use crate::se::seaccess::{se_access_check, AccessCheckResult, ObTypeIndex};

        if key.is_null() {
            return false;
        }
        let current_process = crate::arch::common::percpu::get_current_process();
        if current_process.is_null() {
            return false;
        }
        let token = { (*current_process).get_token() };
        if token.is_null() {
            return false;
        }

        let security = {
            if (*key).security.is_null() {
                (*key).security = registry_default_security();
            }
            (*key).security
        };
        if security.is_null() {
            return false;
        }

        let (result, granted) = se_access_check(ObTypeIndex::Key, security, desired_access, token);
        result == AccessCheckResult::Allowed && (granted & desired_access) == desired_access
    }
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// Find a subkey by name (case-sensitive) under the given parent.
pub unsafe fn find_subkey(parent: *mut RegistryKey, name: &[u16]) -> Option<*mut RegistryKey> {
    if parent.is_null() {
        return None;
    }

    unsafe {
        for i in 0..(*parent).child_count {
            if let Some(child) = (*parent).children[i] {
                let child_name_len = (*child).name_len as usize;
                if child_name_len == name.len() {
                    let mut match_found = true;
                    for j in 0..child_name_len {
                        if (*child).name[j] != name[j] {
                            match_found = false;
                            break;
                        }
                    }
                    if match_found {
                        return Some(child);
                    }
                }
            }
        }
    }
    None
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// Set a value on a registry key. Returns `true` on success.
pub unsafe fn set_value(
    key: *mut RegistryKey,
    name: &[u16],
    vtype: ValueType,
    data: &[u8],
) -> bool {
    if key.is_null() {
        return false;
    }

    // SECURITY: Reject oversized data before any copying.
    if name.len() > MAX_VALUE_NAME_LEN || data.len() > MAX_VALUE_DATA_SIZE {
        kprintln!(
            subsystem: "REG",
            "    [REG] set_value REJECTED: name_len={} data_len={} (max: {} / {})",
            name.len(),
            data.len(),
            MAX_VALUE_NAME_LEN,
            MAX_VALUE_DATA_SIZE
        );
        return false;
    }

    unsafe {
        // Try to update existing value with the same name first.
        for i in 0..(*key).value_count {
            if let Some(ref mut value) = (*key).values[i] {
                let value_name_len = value.name_len as usize;
                if value_name_len == name.len() {
                    let mut match_found = true;
                    for j in 0..value_name_len {
                        if !same_value_unit(value.name[j], name[j]) {
                            match_found = false;
                            break;
                        }
                    }
                    if match_found {
                        // Update existing value.
                        // SAFETY: name.len() <= MAX_VALUE_NAME_LEN, data.len() <= MAX_VALUE_DATA_SIZE
                        // (checked above), so this is safe.
                        value.value_type = vtype;
                        value.data[..data.len()].copy_from_slice(data);
                        value.data_size = data.len() as u32;
                        return true;
                    }
                }
            }
        }

        // Add as a new value if there's room.
        if (*key).value_count < 64 {
            let idx = (*key).value_count;
            // Use truncate variant since we already validated the bounds above.
            (*key).values[idx] = Some(RegValue::new_truncate(name, vtype, data));
            (*key).value_count += 1;
            return true;
        }

        kprintln!(
            subsystem: "REG",
            "    [REG] set_value: value count exhausted (max 64)"
        );
    }
    false
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// Get a value by name. Returns `(ValueType, value_index)` on success.
pub unsafe fn get_value(key: *mut RegistryKey, name: &[u16]) -> Option<(ValueType, usize)> {
    if key.is_null() {
        return None;
    }

    unsafe {
        for i in 0..(*key).value_count {
            if let Some(ref value) = (*key).values[i] {
                let value_name_len = value.name_len as usize;
                if value_name_len == name.len() {
                    let mut match_found = true;
                    for j in 0..value_name_len {
                        if !same_value_unit(value.name[j], name[j]) {
                            match_found = false;
                            break;
                        }
                    }
                    if match_found {
                        return Some((value.value_type, i));
                    }
                }
            }
        }
    }
    None
}

// ============================================================================
// ntdll Registry API Functions
// ============================================================================

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// nt_create_key — creates or opens a registry key and returns a handle.
///
/// handle semantics: slot_index + 1 (1-based), so 0 is invalid.
pub unsafe extern "C" fn nt_create_key(
    key_handle: *mut HANDLE,
    desired_access: u32,
    object_attributes: *mut ObjectAttributes,
    title_index: u32,
    _class: *mut UnicodeString,
    create_options: u32,
    disposition: *mut u32,
) -> NTSTATUS {
    let _operation = REGISTRY_OPERATIONS.lock();
    let desired_access = match key_access_mask(desired_access) {
        Ok(access) => access,
        Err(status) => return status,
    };
    if let Err(status) = probe_user_write(key_handle as u64, core::mem::size_of::<HANDLE>()) {
        return status;
    }

    let _ = title_index;
    // This tree is volatile. Hive links and backup/restore opens need separate
    // implementations and may not be silently converted to normal creates.
    if create_options & !1 != 0 || !_class.is_null() {
        return crate::libs::ntdll::status::STATUS_NOT_SUPPORTED;
    }
    if !disposition.is_null() {
        if let Err(status) = probe_user_write(disposition as u64, 4) {
            return status;
        }
    }

    if key_handle.is_null() {
        return STATUS_OBJECT_NAME_NOT_FOUND;
    }

    let attributes: ObjectAttributes = if !object_attributes.is_null() {
        match read_user_object(object_attributes as u64) {
            Ok(value) => value,
            Err(_) => return STATUS_OBJECT_NAME_NOT_FOUND,
        }
    } else {
        return STATUS_OBJECT_NAME_NOT_FOUND;
    };
    let path = if !attributes.object_name.is_null() {
        match read_user_unicode_string(attributes.object_name as u64, MAX_VALUE_NAME_LEN * 2) {
            Ok(value) => value,
            Err(_) => return STATUS_OBJECT_NAME_NOT_FOUND,
        }
    } else {
        return STATUS_OBJECT_NAME_NOT_FOUND;
    };

    kprintln!(
        subsystem: "REG",
        "    [REG] NtCreateKey: path_len={}",
        path.len()
    );

    let root = REGISTRY_ROOT.lock();
    let mut current = match registry_start(&attributes, *root) {
        Ok(key) => key,
        Err(status) => return status,
    };
    let mut created = false;

    // Skip leading backslashes.
    let mut start = 0;
    while start < path.len() && path[start] == BACKSLASH_U16 {
        start += 1;
    }

    // Walk path components.
    while start < path.len() {
        let mut end = start;
        while end < path.len() && path[end] != BACKSLASH_U16 {
            end += 1;
        }

        let component = &path[start..end];
        if !component.is_empty() {
            let found = find_subkey(current, component);
            if let Some(found_key) = found {
                current = found_key;
            } else {
                if !registry_access_allowed(current, KEY_CREATE_SUB_KEY)
                    || !registry_access_allowed(current, desired_access)
                {
                    return STATUS_ACCESS_DENIED;
                }
                // Create only after checking the parent DACL. Children inherit
                // the same descriptor in this volatile implementation.
                let new_key = match create_subkey(current, component) {
                    Some(k) => k,
                    None => return crate::libs::ntdll::status::STATUS_INSUFFICIENT_RESOURCES,
                };
                current = new_key;
                created = true;
            }
        }
        start = end + 1;
    }

    drop(root);

    if !registry_access_allowed(current, desired_access) {
        return STATUS_ACCESS_DENIED;
    }

    // Allocate handle using round-robin.
    let slot_idx = match allocate_handle_slot(current, desired_access) {
        Some(idx) => idx,
        None => {
            kprintln!(
                subsystem: "REG",
                "    [REG] NtCreateKey: handle table FULL (256 slots)"
            );
            return STATUS_OBJECT_NAME_NOT_FOUND;
        }
    };

    let handle = (slot_idx as u32 + 1) as HANDLE;
    if let Err(status) = write_user_object(key_handle as u64, &handle) {
        KEY_HANDLE_TABLE.lock()[slot_idx] = None;
        return status;
    }
    let action = if created { 1u32 } else { 2u32 };
    if !disposition.is_null() {
        if let Err(status) = write_user_object(disposition as u64, &action) {
            KEY_HANDLE_TABLE.lock()[slot_idx] = None;
            return status;
        }
    }

    kprintln!(
        subsystem: "REG",
        "    [REG] NtCreateKey: handle=0x{:x} (slot={})",
        handle as usize as u32,
        slot_idx
    );

    STATUS_SUCCESS
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// nt_open_key — opens an existing registry key.
pub unsafe extern "C" fn nt_open_key(
    key_handle: *mut HANDLE,
    desired_access: u32,
    object_attributes: *mut ObjectAttributes,
) -> NTSTATUS {
    let _operation = REGISTRY_OPERATIONS.lock();
    let desired_access = match key_access_mask(desired_access) {
        Ok(access) => access,
        Err(status) => return status,
    };
    if let Err(status) = probe_user_write(key_handle as u64, core::mem::size_of::<HANDLE>()) {
        return status;
    }

    if key_handle.is_null() {
        return STATUS_OBJECT_NAME_NOT_FOUND;
    }

    let attributes: ObjectAttributes = if !object_attributes.is_null() {
        match read_user_object(object_attributes as u64) {
            Ok(value) => value,
            Err(_) => return STATUS_OBJECT_NAME_NOT_FOUND,
        }
    } else {
        return STATUS_OBJECT_NAME_NOT_FOUND;
    };
    let path = if !attributes.object_name.is_null() {
        match read_user_unicode_string(attributes.object_name as u64, MAX_VALUE_NAME_LEN * 2) {
            Ok(value) => value,
            Err(_) => return STATUS_OBJECT_NAME_NOT_FOUND,
        }
    } else {
        return STATUS_OBJECT_NAME_NOT_FOUND;
    };

    kprintln!(subsystem: "REG", "    [REG] NtOpenKey: path_len={}", path.len());

    let root = REGISTRY_ROOT.lock();
    let mut current = match registry_start(&attributes, *root) {
        Ok(key) => key,
        Err(status) => return status,
    };

    let mut start = 0;
    while start < path.len() && path[start] == BACKSLASH_U16 {
        start += 1;
    }

    while start < path.len() {
        let mut end = start;
        while end < path.len() && path[end] != BACKSLASH_U16 {
            end += 1;
        }

        let component = &path[start..end];
        if !component.is_empty() {
            let found = find_subkey(current, component);
            match found {
                Some(found_key) => current = found_key,
                None => return STATUS_OBJECT_NAME_NOT_FOUND,
            }
        }
        start = end + 1;
    }

    if !registry_access_allowed(current, desired_access) {
        return STATUS_ACCESS_DENIED;
    }

    let slot_idx = match allocate_handle_slot(current, desired_access) {
        Some(idx) => idx,
        None => {
            kprintln!(subsystem: "REG", "    [REG] NtOpenKey: handle table FULL");
            return STATUS_OBJECT_NAME_NOT_FOUND;
        }
    };

    let handle = (slot_idx as u32 + 1) as HANDLE;
    drop(root);
    if let Err(status) = write_user_object(key_handle as u64, &handle) {
        KEY_HANDLE_TABLE.lock()[slot_idx] = None;
        return status;
    }
    kprintln!(
        subsystem: "REG",
        "    [REG] NtOpenKey: handle=0x{:x} (slot={})",
        handle as usize as u32,
        slot_idx
    );

    STATUS_SUCCESS
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// nt_set_value_key — sets a value on an open key.
///
/// SECURITY: Validates data_size against MAX_VALUE_DATA_SIZE before copying.
pub unsafe extern "C" fn nt_set_value_key(
    key_handle: HANDLE,
    value_name: *mut UnicodeString,
    title_index: u32,
    value_type: u32,
    data: PVOID,
    data_size: u32,
) -> NTSTATUS {
    let _operation = REGISTRY_OPERATIONS.lock();
    let _ = title_index;

    // handle = slot_index + 1. Validate range first.
    let Some(handle_idx) = (key_handle as usize).checked_sub(1) else {
        return STATUS_INVALID_HANDLE;
    };
    if handle_idx >= MAX_KEY_HANDLES {
        return STATUS_INVALID_PARAMETER;
    }

    let handle_table = KEY_HANDLE_TABLE.lock();
    let entry = match &handle_table[handle_idx] {
        Some(e) => e,
        None => return STATUS_INVALID_PARAMETER,
    };

    if entry.desired_access & KEY_SET_VALUE == 0 {
        return STATUS_ACCESS_DENIED;
    }

    let key = entry.key_ptr;
    drop(handle_table);

    // Extract value name.
    let name = if !value_name.is_null() {
        match capture_value_name(value_name as u64) {
            Ok(value) => value,
            Err(_) => return STATUS_INVALID_PARAMETER,
        }
    } else {
        return STATUS_INVALID_PARAMETER;
    };

    // SECURITY: Reject oversized value name and data.
    if name.len() > MAX_VALUE_NAME_LEN {
        kprintln!(
            subsystem: "REG",
            "    [REG] NtSetValueKey: REJECTED name_len={} > {}",
            name.len(),
            MAX_VALUE_NAME_LEN
        );
        return STATUS_INVALID_PARAMETER;
    }
    if data_size as usize > MAX_VALUE_DATA_SIZE {
        kprintln!(
            subsystem: "REG",
            "    [REG] NtSetValueKey: REJECTED data_size={} > {}",
            data_size,
            MAX_VALUE_DATA_SIZE
        );
        return STATUS_INVALID_PARAMETER;
    }

    let vtype = ValueType(value_type);

    if data.is_null() && data_size != 0 {
        return STATUS_ACCESS_VIOLATION;
    }
    let mut data_bytes = alloc::vec![0u8; data_size as usize];
    let data_slice = if !data.is_null() && data_size > 0 {
        if let Err(status) = copy_from_user(&mut data_bytes, data as u64) {
            return status;
        }
        data_bytes.as_slice()
    } else {
        &[]
    };

    kprintln!(
        subsystem: "REG",
        "    [REG] NtSetValueKey: value_name_len={} data_size={}",
        name.len(),
        data_size
    );

    if set_value(key, &name, vtype, data_slice) {
        (*key).last_write_time = crate::ke::time::get_system_time() as i64;
        notify_registry_change(key);
        STATUS_SUCCESS
    } else {
        STATUS_INVALID_PARAMETER
    }
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// nt_query_value_key — queries a value's type and size.
pub unsafe extern "C" fn nt_query_value_key(
    key_handle: HANDLE,
    value_name: *mut UnicodeString,
    key_value_information_class: u32,
    key_value_information: PVOID,
    length: u32,
    result_length: *mut u32,
) -> NTSTATUS {
    let _operation = REGISTRY_OPERATIONS.lock();
    let Some(handle_idx) = (key_handle as usize).checked_sub(1) else {
        return STATUS_INVALID_HANDLE;
    };
    if handle_idx >= MAX_KEY_HANDLES {
        return STATUS_OBJECT_NAME_NOT_FOUND;
    }

    let handle_table = KEY_HANDLE_TABLE.lock();
    let entry = match &handle_table[handle_idx] {
        Some(e) => e,
        None => return STATUS_OBJECT_NAME_NOT_FOUND,
    };

    if entry.desired_access & KEY_QUERY_VALUE == 0 {
        return STATUS_ACCESS_DENIED;
    }
    let key = entry.key_ptr;
    drop(handle_table);

    let name = if !value_name.is_null() {
        match capture_value_name(value_name as u64) {
            Ok(value) => value,
            Err(_) => return STATUS_OBJECT_NAME_NOT_FOUND,
        }
    } else {
        return STATUS_OBJECT_NAME_NOT_FOUND;
    };

    // Find the value and its data directly from the key.
    let found = find_value(key, &name);
    let Some(value) = found else {
        return STATUS_OBJECT_NAME_NOT_FOUND;
    };
    registry_information::value_information(
        &value.name[..value.name_len as usize],
        value.value_type,
        &value.data[..value.data_size as usize],
        key_value_information_class,
        key_value_information,
        length,
        result_length,
    )
}

/// Finds the stored value, retaining its original name spelling.
unsafe fn find_value<'a>(key: *mut RegistryKey, name: &[u16]) -> Option<&'a RegValue> {
    if key.is_null() {
        return None;
    }
    unsafe {
        for i in 0..(*key).value_count {
            if let Some(ref value) = (*key).values[i] {
                let value_name_len = value.name_len as usize;
                if value_name_len == name.len() {
                    let mut match_found = true;
                    for j in 0..value_name_len {
                        if !same_value_unit(value.name[j], name[j]) {
                            match_found = false;
                            break;
                        }
                    }
                    if match_found {
                        return Some(value);
                    }
                }
            }
        }
    }
    None
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// nt_delete_key — deletes a key opened with DELETE access.
///
/// Honours NtDeleteKey semantics where the in-memory tree allows them:
/// the key must have no subkeys, its parent's child slot is cleared, and
/// every open handle and pending notification referencing it is
/// invalidated so no caller keeps using freed memory. Root keys cannot
/// be deleted.
pub unsafe extern "C" fn nt_delete_key(key_handle: HANDLE) -> NTSTATUS {
    let _operation = REGISTRY_OPERATIONS.lock();
    const STATUS_CANNOT_DELETE: NTSTATUS = -1073741791;

    let Some(handle_idx) = (key_handle as usize).checked_sub(1) else {
        return STATUS_INVALID_HANDLE;
    };
    if handle_idx >= MAX_KEY_HANDLES {
        kprintln!(
            subsystem: "REG",
            "    [REG] NtDeleteKey: handle 0x{:x} out of range",
            key_handle as u64
        );
        return STATUS_INVALID_HANDLE;
    }

    let (key, parent) = {
        let handle_table = KEY_HANDLE_TABLE.lock();
        match &handle_table[handle_idx] {
            Some(entry) => {
                if entry.desired_access & DELETE_RIGHT == 0 {
                    kprintln!(
                        subsystem: "REG",
                        "    [REG] NtDeleteKey: handle 0x{:x} lacks DELETE access",
                        key_handle as u64
                    );
                    return STATUS_ACCESS_DENIED;
                }
                if entry.key_ptr.is_null() {
                    return STATUS_INVALID_HANDLE;
                }
                let key = entry.key_ptr;
                let parent = (*key).parent;
                (key, parent)
            }
            None => {
                kprintln!(
                    subsystem: "REG",
                    "    [REG] NtDeleteKey: handle 0x{:x} not found",
                    key_handle as u64
                );
                return STATUS_INVALID_HANDLE;
            }
        }
    };

    if parent.is_null() {
        kprintln!(subsystem: "REG", "    [REG] NtDeleteKey: cannot delete a root key");
        return STATUS_CANNOT_DELETE;
    }
    if (*key).child_count != 0 {
        kprintln!(subsystem: "REG", "    [REG] NtDeleteKey: key still has subkeys");
        return STATUS_CANNOT_DELETE;
    }

    // Remove the key from its parent's child list, compacting the slots.
    {
        let count = (*parent).child_count;
        let mut found: Option<usize> = None;
        for i in 0..count {
            if (*parent).children[i] == Some(key) {
                found = Some(i);
                break;
            }
        }
        if let Some(idx) = found {
            for i in idx..count - 1 {
                (*parent).children[i] = (*parent).children[i + 1];
            }
            (*parent).children[count - 1] = None;
            (*parent).child_count -= 1;
        }
    }

    // Invalidate every open handle to the deleted key.
    {
        let mut handle_table = KEY_HANDLE_TABLE.lock();
        for slot in handle_table.iter_mut() {
            if let Some(entry) = slot {
                if entry.key_ptr == key {
                    *slot = None;
                }
            }
        }
    }

    // Drop pending notifications watching this key so they cannot touch
    // freed memory.
    {
        let mut notifications = KEY_NOTIFICATIONS.lock();
        for slot in notifications.iter_mut() {
            if let Some(notification) = slot {
                if notification.key_ptr == key {
                    *slot = None;
                }
            }
        }
    }

    crate::mm::pool::free(key as *mut u8);
    kprintln!(subsystem: "REG", "    [REG] NtDeleteKey: key deleted");
    STATUS_SUCCESS
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// nt_delete_value_key — deletes a named value from a key.
pub unsafe extern "C" fn nt_delete_value_key(
    key_handle: HANDLE,
    value_name: *mut UnicodeString,
) -> NTSTATUS {
    let _operation = REGISTRY_OPERATIONS.lock();
    if value_name.is_null() {
        kprintln!(subsystem: "REG", "    [REG] NtDeleteValueKey: value_name is null");
        return STATUS_INVALID_PARAMETER;
    }

    let Some(handle_idx) = (key_handle as usize).checked_sub(1) else {
        return STATUS_INVALID_HANDLE;
    };
    if handle_idx >= MAX_KEY_HANDLES {
        kprintln!(
            subsystem: "REG",
            "    [REG] NtDeleteValueKey: handle 0x{:x} out of range",
            key_handle as u64
        );
        return STATUS_INVALID_HANDLE;
    }

    let key = {
        let handle_table = KEY_HANDLE_TABLE.lock();
        match &handle_table[handle_idx] {
            Some(entry) => {
                if entry.desired_access & KEY_SET_VALUE == 0 {
                    return STATUS_ACCESS_DENIED;
                }
                entry.key_ptr
            }
            None => {
                kprintln!(
                    subsystem: "REG",
                    "    [REG] NtDeleteValueKey: handle 0x{:x} not found",
                    key_handle as u64
                );
                return STATUS_INVALID_HANDLE;
            }
        }
    };

    if key.is_null() {
        kprintln!(
            subsystem: "REG",
            "    [REG] NtDeleteValueKey: handle 0x{:x} resolves to null key",
            key_handle as u64
        );
        return STATUS_INVALID_HANDLE;
    }

    // Extract ValueName UTF-16 string.
    let name_slice = match capture_value_name(value_name as u64) {
        Ok(value) => value,
        Err(_) => return STATUS_OBJECT_NAME_NOT_FOUND,
    };
    let name_len_u16 = name_slice.len();

    unsafe {
        let key_ref = &mut *key;
        let mut found_idx: Option<usize> = None;
        for i in 0..key_ref.value_count {
            if let Some(ref v) = key_ref.values[i] {
                let v_len = v.name_len as usize;
                if v_len == name_len_u16 {
                    let mut eq = true;
                    for j in 0..v_len {
                        if !same_value_unit(v.name[j], name_slice[j]) {
                            eq = false;
                            break;
                        }
                    }
                    if eq {
                        found_idx = Some(i);
                        break;
                    }
                }
            }
        }

        match found_idx {
            None => {
                kprintln!(
                    subsystem: "REG",
                    "    [REG] NtDeleteValueKey: value not found, name_len={}",
                    name_len_u16
                );
                STATUS_OBJECT_NAME_NOT_FOUND
            }
            Some(idx) => {
                // Swap-remove: the array stays compact.
                let last = key_ref.value_count - 1;
                if idx != last {
                    key_ref.values[idx] = key_ref.values[last].take();
                } else {
                    key_ref.values[idx] = None;
                }
                key_ref.value_count -= 1;
                key_ref.last_write_time = crate::ke::time::get_system_time() as i64;
                notify_registry_change(key);
                kprintln!(
                    subsystem: "REG",
                    "    [REG] NtDeleteValueKey: deleted value at idx={}, count={}",
                    idx,
                    key_ref.value_count
                );
                STATUS_SUCCESS
            }
        }
    }
}

// ============================================================================
// KEY_INFORMATION structures (NT 6.1 compatible)
// ============================================================================

/// KEY_BASIC_INFORMATION — basic key information.
/// Total: 0x10 bytes fixed header + name (FAM).
#[repr(C)]
pub struct KeyBasicInformation {
    pub last_write_time: i64,
    pub title_index: u32,
    pub name_length: u32,
    pub name: [u16; 1], // Flexible array member
}

/// KEY_NODE_INFORMATION — detailed key information.
/// Total: 0x18 bytes fixed header + name (FAM).
#[repr(C)]
pub struct KeyNodeInformation {
    pub last_write_time: i64,
    pub title_index: u32,
    pub class_name_offset: u32,
    pub class_name_length: u32,
    pub name_length: u32,
    pub name: [u16; 1], // Flexible array member
}

/// KEY_VALUE_BASIC_INFORMATION — basic value information.
/// Total: 0x0C bytes fixed header + name (FAM).
#[repr(C)]
pub struct KeyValueBasicInformation {
    pub title_index: u32,
    pub value_type: u32,
    pub name_length: u32,
    pub name: [u16; 1], // Flexible array member
}

/// KEY_VALUE_FULL_INFORMATION — full value information.
/// Total: 0x14 bytes fixed header + name + data (FAM).
#[repr(C)]
pub struct KeyValueFullInformation {
    pub title_index: u32,
    pub value_type: u32,
    pub data_offset: u32,
    pub data_size: u32,
    pub name_length: u32,
    pub name: [u16; 1], // Flexible array member
}

/// Fixed header sizes
const KEY_BASIC_INFORMATION_SIZE: usize = 16;
const KEY_NODE_INFORMATION_SIZE: usize = 24;
const KEY_VALUE_BASIC_INFORMATION_SIZE: usize = 12;
const KEY_VALUE_FULL_INFORMATION_SIZE: usize = 20;

fn write_result_length(result_length: *mut u32, required: usize) -> bool {
    if result_length.is_null() {
        return true;
    }
    write_user_object(result_length as u64, &(required as u32)).is_ok()
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// nt_query_key — query information about a registry key.
pub unsafe extern "C" fn nt_query_key(
    key_handle: HANDLE,
    key_information_class: u32,
    key_information: PVOID,
    length: u32,
    result_length: *mut u32,
) -> NTSTATUS {
    let _operation = REGISTRY_OPERATIONS.lock();
    use crate::libs::ntdll::status::STATUS_INVALID_HANDLE;

    let Some(handle_idx) = (key_handle as usize).checked_sub(1) else {
        return STATUS_INVALID_HANDLE;
    };
    if handle_idx >= MAX_KEY_HANDLES {
        return STATUS_INVALID_HANDLE;
    }

    let handle_table = KEY_HANDLE_TABLE.lock();
    let entry = match &handle_table[handle_idx] {
        Some(e) => e,
        None => return STATUS_INVALID_HANDLE,
    };

    if key_information_class != 3 && entry.desired_access & KEY_QUERY_VALUE == 0 {
        return STATUS_ACCESS_DENIED;
    }
    let key = entry.key_ptr;
    drop(handle_table);

    // SAFETY: the operation lock excludes tree mutation and deletion.
    if key_information_class == 3 {
        return registry_information::key_name_information(
            &*key,
            key_information,
            length,
            result_length,
        );
    }
    registry_information::key_information(
        &*key,
        key_information_class,
        key_information,
        length,
        result_length,
        true,
    )
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// nt_enumerate_key — enumerate subkeys of a registry key.
pub unsafe extern "C" fn nt_enumerate_key(
    key_handle: HANDLE,
    index: u32,
    key_information_class: u32,
    key_information: PVOID,
    length: u32,
    result_length: *mut u32,
) -> NTSTATUS {
    let _operation = REGISTRY_OPERATIONS.lock();
    use crate::libs::ntdll::status::STATUS_INVALID_HANDLE;
    use crate::libs::ntdll::status::STATUS_NO_MORE_ENTRIES;

    let Some(handle_idx) = (key_handle as usize).checked_sub(1) else {
        return STATUS_INVALID_HANDLE;
    };
    if handle_idx >= MAX_KEY_HANDLES {
        return STATUS_INVALID_HANDLE;
    }

    let handle_table = KEY_HANDLE_TABLE.lock();
    let entry = match &handle_table[handle_idx] {
        Some(e) => e,
        None => return STATUS_INVALID_HANDLE,
    };

    if entry.desired_access & KEY_ENUMERATE_SUB_KEYS == 0 {
        return STATUS_ACCESS_DENIED;
    }
    let key = entry.key_ptr;
    drop(handle_table);

    let child_count = unsafe { (*key).child_count };

    if index >= child_count as u32 {
        return STATUS_NO_MORE_ENTRIES;
    }

    // Get the child key
    let child = unsafe {
        if let Some(child_ptr) = (*key).children[index as usize] {
            child_ptr
        } else {
            return STATUS_NO_MORE_ENTRIES;
        }
    };

    // SAFETY: the operation lock pins the enumerated child and its subtree.
    registry_information::key_information(
        &*child,
        key_information_class,
        key_information,
        length,
        result_length,
        false,
    )
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// nt_enumerate_value_key — enumerate values of a registry key.
pub unsafe extern "C" fn nt_enumerate_value_key(
    key_handle: HANDLE,
    index: u32,
    key_value_information_class: u32,
    key_value_information: PVOID,
    length: u32,
    result_length: *mut u32,
) -> NTSTATUS {
    let _operation = REGISTRY_OPERATIONS.lock();
    use crate::libs::ntdll::status::STATUS_INVALID_HANDLE;
    use crate::libs::ntdll::status::STATUS_NO_MORE_ENTRIES;

    let Some(handle_idx) = (key_handle as usize).checked_sub(1) else {
        return STATUS_INVALID_HANDLE;
    };
    if handle_idx >= MAX_KEY_HANDLES {
        return STATUS_INVALID_HANDLE;
    }

    let handle_table = KEY_HANDLE_TABLE.lock();
    let entry = match &handle_table[handle_idx] {
        Some(e) => e,
        None => return STATUS_INVALID_HANDLE,
    };

    if entry.desired_access & KEY_QUERY_VALUE == 0 {
        return STATUS_ACCESS_DENIED;
    }
    let key = entry.key_ptr;
    drop(handle_table);

    let value_count = unsafe { (*key).value_count };

    if index >= value_count as u32 {
        return STATUS_NO_MORE_ENTRIES;
    }

    // Get the value entry
    let value = unsafe {
        match &(*key).values[index as usize] {
            Some(v) => v,
            None => return STATUS_NO_MORE_ENTRIES,
        }
    };

    registry_information::value_information(
        &value.name[..value.name_len as usize],
        value.value_type,
        &value.data[..value.data_size as usize],
        key_value_information_class,
        key_value_information,
        length,
        result_length,
    )
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// nt_reg_close_key — close a registry key handle.
pub unsafe extern "C" fn nt_reg_close_key(key_handle: HANDLE) -> NTSTATUS {
    let _operation = REGISTRY_OPERATIONS.lock();
    use crate::libs::ntdll::status::STATUS_INVALID_HANDLE;

    let Some(handle_idx) = (key_handle as usize).checked_sub(1) else {
        return STATUS_INVALID_HANDLE;
    };
    if handle_idx >= MAX_KEY_HANDLES {
        return STATUS_INVALID_HANDLE;
    }

    let mut handle_table = KEY_HANDLE_TABLE.lock();
    if handle_table[handle_idx].is_some() {
        handle_table[handle_idx] = None;
        kprintln!(
            subsystem: "REG",
            "    [REG] Closed key handle 0x{:x} (slot={})",
            key_handle as u32,
            handle_idx
        );
        STATUS_SUCCESS
    } else {
        STATUS_INVALID_HANDLE
    }
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// nt_flush_key — flush pending changes for a key.
///
/// The kernel registry cache in this build is written through on every
/// `nt_set_value_key`/`nt_delete_value_key` call, so there is nothing pending
/// to flush. The handle is still validated to match the real API's
/// error behaviour.
pub unsafe extern "C" fn nt_flush_key(key_handle: HANDLE) -> NTSTATUS {
    let _operation = REGISTRY_OPERATIONS.lock();
    let Some(handle_idx) = (key_handle as usize).checked_sub(1) else {
        return STATUS_INVALID_HANDLE;
    };
    if handle_idx >= MAX_KEY_HANDLES || KEY_HANDLE_TABLE.lock()[handle_idx].is_none() {
        return STATUS_INVALID_HANDLE;
    }
    STATUS_SUCCESS
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// nt_notify_change_key — register (or wait for) key change notifications.
///
/// Synchronous requests report success. Asynchronous requests register the
/// supplied event; value mutations signal it and remove the notification.
pub unsafe extern "C" fn nt_notify_change_key(
    key_handle: HANDLE,
    event_handle: HANDLE,
    _apc_routine: PVOID,
    _apc_context: PVOID,
    _completion_list: PVOID,
    _completion_filter: u32,
    watch_tree: u32,
    _buffer: PVOID,
    _buffer_size: u32,
    asynchronous: u32,
) -> NTSTATUS {
    let _operation = REGISTRY_OPERATIONS.lock();
    let Some(handle_idx) = (key_handle as usize).checked_sub(1) else {
        return STATUS_INVALID_HANDLE;
    };
    if handle_idx >= MAX_KEY_HANDLES || KEY_HANDLE_TABLE.lock()[handle_idx].is_none() {
        return STATUS_INVALID_HANDLE;
    }

    if asynchronous == 0 {
        return STATUS_SUCCESS;
    }

    let mut notifications = KEY_NOTIFICATIONS.lock();
    if let Some(free_slot) = notifications.iter().position(|slot| slot.is_none()) {
        let key_ptr = KEY_HANDLE_TABLE.lock()[handle_idx]
            .as_ref()
            .map(|entry| entry.key_ptr)
            .unwrap_or(core::ptr::null_mut());
        notifications[free_slot] = Some(RegistryNotification {
            key_ptr,
            event_handle,
            watch_tree: watch_tree != 0,
        });
        return 0x40000003_u32 as i32;
    }
    STATUS_SUCCESS
}

/// Initialize default registry keys (SYSTEM, SOFTWARE, HARDWARE, SAM, SECURITY).
pub fn init_default_keys() {
    unsafe {
        let root = REGISTRY_ROOT.lock();
        if let Some(root_key) = *root {
            // create SYSTEM hive key.
            let sys_path = [
                b'S' as u16,
                b'Y' as u16,
                b'S' as u16,
                b'T' as u16,
                b'E' as u16,
                b'M' as u16,
            ];
            let _ = create_subkey(root_key, &sys_path);

            // create SOFTWARE hive key with a version value.
            let soft_path = [
                b'S' as u16,
                b'O' as u16,
                b'F' as u16,
                b'T' as u16,
                b'W' as u16,
                b'A' as u16,
                b'R' as u16,
                b'E' as u16,
            ];
            let _ = create_subkey(root_key, &soft_path);

            // create HARDWARE hive key.
            let hw_path = [
                b'H' as u16,
                b'A' as u16,
                b'R' as u16,
                b'D' as u16,
                b'W' as u16,
                b'A' as u16,
                b'R' as u16,
                b'E' as u16,
            ];
            let _ = create_subkey(root_key, &hw_path);

            // create SAM hive key.
            let sam_path = [b'S' as u16, b'A' as u16, b'M' as u16];
            let _ = create_subkey(root_key, &sam_path);

            // create SECURITY hive key.
            let sec_path = [
                b'S' as u16,
                b'E' as u16,
                b'C' as u16,
                b'U' as u16,
                b'R' as u16,
                b'I' as u16,
                b'T' as u16,
                b'Y' as u16,
            ];
            let _ = create_subkey(root_key, &sec_path);
        }
    }
}

#[cfg(test)]
pub(crate) static REGISTRY_TEST_LOCK: spin::Spinlock<()> = spin::Spinlock::new(());

#[cfg(test)]
mod tests {
    use super::*;

    // The exact code NtDeleteKey uses; kept local because the production
    // constant lives inside the function body.
    const STATUS_CANNOT_DELETE: i32 = -1073741791;

    // All tests in this module mutate the same process-global registry tree
    // (REGISTRY_ROOT, root child arrays, KEY_HANDLE_TABLE) through unsynchronized
    // read-modify-write sequences. Running them in parallel loses updates and
    // corrupts key structures, so serialize them explicitly.

    // Under libtest the kernel heap is never initialised; configure a test
    // region so pool allocations for registry keys work.
    fn ensure_test_heap() {
        if crate::mm::heap::KERNEL_HEAP.configured_region().is_some() {
            return;
        }
        const TEST_HEAP_LEN: usize = 4 * 1024 * 1024;
        static mut TEST_HEAP: [u8; TEST_HEAP_LEN] = [0u8; TEST_HEAP_LEN];
        let base = core::ptr::addr_of_mut!(TEST_HEAP) as *mut u8;
        crate::mm::heap::KERNEL_HEAP.configure(base, TEST_HEAP_LEN);
    }

    fn name_of(s: &str) -> alloc::vec::Vec<u16> {
        s.chars().map(|c| c as u16).collect()
    }

    fn root_key() -> *mut RegistryKey {
        init();
        match *REGISTRY_ROOT.lock() {
            Some(root) => root,
            None => core::ptr::null_mut(),
        }
    }

    #[test]
    fn delete_key_removes_leaf_and_invalidates_handle() {
        let _guard = REGISTRY_TEST_LOCK.lock();
        ensure_test_heap();
        unsafe {
            let root = root_key();
            assert!(!root.is_null());

            let child = create_subkey(root, &name_of("DeleteLeafTest")).unwrap();
            let slot = allocate_handle_slot(child, DELETE_RIGHT).unwrap();
            let handle = (slot + 1) as HANDLE;

            let status = nt_delete_key(handle);
            assert_eq!(status, STATUS_SUCCESS);

            // Parent no longer lists the child.
            let still_listed = (0..(*root).child_count).any(|i| (*root).children[i] == Some(child));
            assert!(!still_listed);

            // The handle slot was invalidated.
            let handle_table = KEY_HANDLE_TABLE.lock();
            assert!(handle_table[slot].is_none());
            drop(handle_table);

            // Retrying through the now-dead handle must not succeed.
            assert_ne!(nt_delete_key(handle), STATUS_SUCCESS);
        }
    }

    #[test]
    fn delete_key_requires_delete_access() {
        let _guard = REGISTRY_TEST_LOCK.lock();
        ensure_test_heap();
        unsafe {
            let root = root_key();
            let child = create_subkey(root, &name_of("DeleteNoAccess")).unwrap();
            let slot = allocate_handle_slot(child, 0).unwrap();
            let handle = (slot + 1) as HANDLE;

            assert_eq!(nt_delete_key(handle), STATUS_ACCESS_DENIED);

            // Clean up the slot so other tests can reuse the table.
            deallocate_handle_slot(slot);
        }
    }

    #[test]
    fn delete_key_refuses_parent_with_children() {
        let _guard = REGISTRY_TEST_LOCK.lock();
        ensure_test_heap();
        unsafe {
            let root = root_key();
            let parent = create_subkey(root, &name_of("DeleteParentTest")).unwrap();
            let _child = create_subkey(parent, &name_of("DeleteParentTestChild")).unwrap();

            let slot = allocate_handle_slot(parent, DELETE_RIGHT).unwrap();
            let handle = (slot + 1) as HANDLE;

            assert_eq!(nt_delete_key(handle), STATUS_CANNOT_DELETE);
            assert_eq!((*parent).child_count, 1);
            deallocate_handle_slot(slot);
        }
    }

    #[test]
    fn delete_key_refuses_root() {
        let _guard = REGISTRY_TEST_LOCK.lock();
        ensure_test_heap();
        unsafe {
            let root = root_key();
            let slot = allocate_handle_slot(root, DELETE_RIGHT).unwrap();
            let handle = (slot + 1) as HANDLE;

            assert_eq!(nt_delete_key(handle), STATUS_CANNOT_DELETE);
            deallocate_handle_slot(slot);
        }
    }
}

#[path = "registry_queries.rs"]
pub mod queries;

pub use queries::{nt_open_key_ex, nt_query_multiple_value_key, nt_query_open_sub_keys};

#[path = "registry_mutation.rs"]
pub mod mutation;
pub use mutation::{nt_rename_key, nt_set_information_key};

#[path = "registry_information.rs"]
mod registry_information;

// Registry values use a counted name; zero length identifies the default value.
fn capture_value_name(address: u64) -> Result<alloc::vec::Vec<u16>, NTSTATUS> {
    let header = read_user_object::<UnicodeString>(address)?;
    if header.length % 2 != 0
        || header.length > header.maximum_length
        || header.length as usize > MAX_VALUE_NAME_LEN * 2
    {
        return Err(STATUS_INVALID_PARAMETER);
    }
    if header.length == 0 {
        return Ok(alloc::vec::Vec::new());
    }
    read_user_unicode_string(address, MAX_VALUE_NAME_LEN * 2)
}
fn same_value_unit(left: u16, right: u16) -> bool {
    // Full Win7 NLS upcase data remains a separate compatibility requirement.
    let lower = |unit| {
        if (65..=90).contains(&unit) {
            unit + 32
        } else {
            unit
        }
    };
    lower(left) == lower(right)
}

fn key_access_mask(mut access: u32) -> Result<u32, NTSTATUS> {
    // WOW64 view selectors choose a namespace; this single view has no
    // redirection yet, so reject them explicitly.
    if access & 0x300 != 0 {
        return Err(crate::libs::ntdll::status::STATUS_NOT_SUPPORTED);
    }
    crate::se::seaccess::REGISTRY_GENERIC_MAPPING.map_generic(&mut access);
    if access & !0x000f_003f != 0 {
        return Err(STATUS_ACCESS_DENIED);
    }
    Ok(access)
}
fn registry_start(
    attributes: &ObjectAttributes,
    root: Option<*mut RegistryKey>,
) -> Result<*mut RegistryKey, NTSTATUS> {
    if attributes.length as usize != core::mem::size_of::<ObjectAttributes>() {
        return Err(STATUS_INVALID_PARAMETER);
    }
    if attributes.root_directory.is_null() {
        return root.ok_or(STATUS_OBJECT_NAME_NOT_FOUND);
    }
    let table = KEY_HANDLE_TABLE.lock();
    (attributes.root_directory as usize)
        .checked_sub(1)
        .and_then(|slot| table.get(slot))
        .and_then(Option::as_ref)
        .map(|entry| entry.key_ptr)
        .ok_or(STATUS_INVALID_HANDLE)
}
