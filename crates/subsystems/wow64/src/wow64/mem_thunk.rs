//! mem_thunk — Wow64 Memory Management Thunks
//
//! This module implements the memory management thunks that translate
//! 32-bit virtual memory operations to their 64-bit equivalents.
//
//! The key functions are:
//!   * wow64_allocate_virtual_memory32
//!   * wow64_free_virtual_memory32
//!   * wow64_protect_virtual_memory32
//!   * wow64_query_virtual_memory32
//!   * wow64_read_virtual_memory32
//!   * wow64_write_virtual_memory32
//
//!     References:
//!   * geoffchappell.com — wow64 memory management

use crate::libs::wow64::types::*;
use crate::libs::wow64::wow64vas;
use crate::ntoskrnl::ntdll::status::STATUS_ACCESS_VIOLATION;

// =============================================================================
// wow64_allocate_virtual_memory32
// =============================================================================

/// Allocate virtual memory in a 32-bit process.
///
/// This is the Wow64 equivalent of NT_ALLOCATE_VIRTUAL_MEMORY for 32-bit
/// processes. It allocates memory in the lower 4GB address space.
///
/// # Arguments
/// * `process_handle` - handle to the target process (can be nt_current_process)
/// * `base_address` - Pointer to base address (input/output)
/// * `zero_bits` - Zero bits for address hint
/// * `region_size` - Pointer to region size (input/output)
/// * `allocation_type` - MEM_COMMIT, MEM_RESERVE, etc.
/// * `protect` - Page protection (PAGE_READWRITE, etc.)
///
/// # Returns
/// * NTSTATUS
///
/// # Safety
/// This function manipulates process address spaces.
pub unsafe extern "C" fn wow64_allocate_virtual_memory32(
    process_handle: HANDLE32,
    base_address: Ulong32Ptr,
    zero_bits: ULONG32,
    region_size: ULONG32,
    allocation_type: ULONG32,
    protect: ULONG32,
) -> ULONG32 {
    crate::wow64_klog!(
        "Wow64AllocateVirtualMemory32 handle=0x{:08x} base_ptr=0x{:08x} \
         size=0x{:08x} type=0x{:08x} prot=0x{:02x} zb={:#x}",
        process_handle,
        base_address,
        region_size,
        allocation_type,
        protect,
        zero_bits
    );
    let _ = zero_bits;

    let mut base = if base_address != 0 {
        match crate::mm::user_copy::read_user_object::<u32>(base_address as u64) {
            Ok(value) => value as usize,
            Err(status) => return status as u32,
        }
    } else {
        0
    };
    let mut size = region_size as usize;

    // Validate allocation type
    if allocation_type == 0 {
        return STATUS_INVALID_PARAMETER;
    }

    // Validate protection
    if !is_valid_protection(protect) {
        crate::wow64_klog!("Invalid protection 0x{:02x}", protect);
        return STATUS_INVALID_PARAMETER;
    }

    // Allocate through the Wow64 VAS
    let status = unsafe {
        crate::libs::ntdll::virtual_mem::nt_allocate_virtual_memory(
            process_handle as usize as *mut core::ffi::c_void,
            &mut base as *mut usize as *mut crate::libs::ntdll::types::PVOID,
            zero_bits as usize,
            &mut size,
            allocation_type,
            protect,
        )
    };
    if status >= 0 {
        let _ = crate::mm::user_copy::write_user_object(base_address as u64, &(base as u32));
        let _ = crate::mm::user_copy::write_user_object(region_size as u64, &(size as u32));
    }
    status as u32
}

// =============================================================================
// wow64_free_virtual_memory32
// =============================================================================

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// Free virtual memory in a 32-bit process.
///
/// # Arguments
/// * `process_handle` - handle to the target process
/// * `base_address` - Base address to free
/// * `region_size` - Pointer to region size (output)
/// * `free_type` - MEM_DECOMMIT or MEM_RELEASE
///
/// # Returns
/// * NTSTATUS
pub unsafe extern "C" fn wow64_free_virtual_memory32(
    process_handle: HANDLE32,
    base_address: ULONG32,
    region_size: Ulong32Ptr,
    free_type: ULONG32,
) -> ULONG32 {
    crate::wow64_klog!(
        "Wow64FreeVirtualMemory32 handle=0x{:08x} addr=0x{:08x} \
         size_ptr=0x{:08x} type=0x{:08x}",
        process_handle,
        base_address,
        region_size,
        free_type
    );

    // Validate free type
    match free_type {
        memory_allocation_type::MEM_RELEASE | memory_allocation_type::MEM_DECOMMIT => {}
        _ => {
            return STATUS_INVALID_PARAMETER;
        }
    }

    // Validate address
    if !wow64vas::is_valid_user_address(base_address) {
        return STATUS_INVALID_PARAMETER;
    }

    let mut base = base_address as usize;
    let mut size = region_size as usize;
    let status = unsafe {
        crate::libs::ntdll::virtual_mem::nt_free_virtual_memory(
            process_handle as usize as *mut core::ffi::c_void,
            &mut base as *mut usize as *mut crate::libs::ntdll::types::PVOID,
            &mut size,
            free_type,
        )
    };
    if status >= 0 && region_size != 0 {
        let _ = crate::mm::user_copy::write_user_object(region_size as u64, &(size as u32));
    }
    status as u32
}

// =============================================================================
// wow64_protect_virtual_memory32
// =============================================================================

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// Change the protection on a region of memory.
///
/// # Arguments
/// * `process_handle` - handle to the target process
/// * `base_address` - Pointer to base address (input/output)
/// * `region_size` - Pointer to region size (input/output)
/// * `new_protect` - New protection value
/// * `old_protect` - Pointer to old protection (output)
///
/// # Returns
/// * NTSTATUS
pub unsafe extern "C" fn wow64_protect_virtual_memory32(
    process_handle: HANDLE32,
    base_address: Ulong32Ptr,
    region_size: Ulong32Ptr,
    new_protect: ULONG32,
    old_protect: Ulong32Ptr,
) -> ULONG32 {
    crate::wow64_klog!(
        "Wow64ProtectVirtualMemory32 handle=0x{:08x} base_ptr=0x{:08x} \
         size_ptr=0x{:08x} prot=0x{:02x} old_ptr=0x{:08x}",
        process_handle,
        base_address,
        region_size,
        new_protect,
        old_protect
    );

    let mut base = match crate::mm::user_copy::read_user_object::<u32>(base_address as u64) {
        Ok(value) => value as usize,
        Err(status) => return status as u32,
    };
    let mut size = match crate::mm::user_copy::read_user_object::<u32>(region_size as u64) {
        Ok(value) => value as usize,
        Err(status) => return status as u32,
    };
    let mut old_protect_value: u32 = 0;

    // Validate protection
    if !is_valid_protection(new_protect) {
        return STATUS_INVALID_PARAMETER;
    }

    // Validate range
    let status = unsafe {
        crate::libs::ntdll::virtual_mem::nt_protect_virtual_memory(
            process_handle as usize as *mut core::ffi::c_void,
            &mut base as *mut usize as *mut crate::libs::ntdll::types::PVOID,
            &mut size,
            new_protect,
            &mut old_protect_value,
        )
    };
    if status >= 0 {
        let _ = crate::mm::user_copy::write_user_object(base_address as u64, &(base as u32));
        let _ = crate::mm::user_copy::write_user_object(region_size as u64, &(size as u32));
        if old_protect != 0 {
            let _ = crate::mm::user_copy::write_user_object(old_protect as u64, &old_protect_value);
        }
    }
    status as u32
}

// =============================================================================
// wow64_query_virtual_memory32
// =============================================================================

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// Query information about a region of virtual memory.
///
/// # Arguments
/// * `process_handle` - handle to the target process
/// * `base_address` - Base address to query
/// * `memory_info_class` - r#type of information to query
/// * `buffer` - Output buffer
/// * `buffer_size` - Size of output buffer
/// * `return_length` - Actual size needed (output)
///
/// # Returns
/// * NTSTATUS
///
/// # Info Classes
/// * 0 - MemoryBasicInformation
/// * 1 - MemoryWorkingSetList
/// * 2 - MemorySectionName
/// * etc.
pub unsafe extern "C" fn wow64_query_virtual_memory32(
    process_handle: HANDLE32,
    base_address: ULONG32,
    memory_info_class: ULONG32,
    buffer: ULONG32,
    buffer_size: ULONG32,
    return_length: Ulong32Ptr,
) -> ULONG32 {
    crate::wow64_klog!(
        "Wow64QueryVirtualMemory32 handle=0x{:08x} addr=0x{:08x} \
         class={} buf=0x{:08x} sz={} retlen_ptr=0x{:08x}",
        process_handle,
        base_address,
        memory_info_class,
        buffer,
        buffer_size,
        return_length
    );

    // Validate buffer
    let required_size = core::mem::size_of::<Wow64MemoryBasicInformation32>() as u32;
    if buffer == 0 || buffer_size < required_size {
        if return_length != 0 {
            let _ = crate::mm::user_copy::write_user_object(return_length as u64, &required_size);
        }
        return STATUS_BUFFER_TOO_SMALL;
    }

    // Validate address
    if !wow64vas::is_valid_user_address(base_address) {
        return STATUS_INVALID_PARAMETER;
    }

    match memory_info_class {
        0 => {
            // MemoryBasicInformation
            if let Some(info) = wow64vas::query(base_address) {
                crate::wow64_klog!(
                    "MemoryBasicInformation base=0x{:08x} \
                     size=0x{:08x} state=0x{:08x}",
                    info.base_address,
                    info.region_size,
                    info.state
                );
                if crate::mm::user_copy::write_user_object(buffer as u64, &info).is_err() {
                    return STATUS_ACCESS_VIOLATION as u32;
                }
                if return_length != 0 {
                    let _ = crate::mm::user_copy::write_user_object(
                        return_length as u64,
                        &required_size,
                    );
                }
                STATUS_SUCCESS
            } else {
                STATUS_UNSUCCESSFUL
            }
        }
        _ => {
            crate::wow64_klog!("Unsupported memory info class {}", memory_info_class);
            STATUS_INVALID_PARAMETER
        }
    }
}

// =============================================================================
// wow64_read_virtual_memory32
// =============================================================================

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// read memory from another process.
///
/// # Arguments
/// * `process_handle` - handle to the source process
/// * `base_address` - Address to read from
/// * `buffer` - Local buffer to read into (32-bit address)
/// * `size` - Number of bytes to read
/// * `number_of_bytes_read` - Number of bytes actually read (output)
///
/// # Returns
/// * NTSTATUS
pub unsafe extern "C" fn wow64_read_virtual_memory32(
    process_handle: HANDLE32,
    base_address: ULONG32,
    buffer: ULONG32,
    size: ULONG32,
    number_of_bytes_read: Ulong32Ptr,
) -> ULONG32 {
    crate::wow64_klog!(
        "Wow64ReadVirtualMemory32 handle=0x{:08x} from=0x{:08x} \
         to=0x{:08x} size=0x{:08x} bytes_read_ptr=0x{:08x}",
        process_handle,
        base_address,
        buffer,
        size,
        number_of_bytes_read
    );

    // Validate ranges
    if !wow64vas::is_valid_user_range(base_address, size) {
        return STATUS_ACCESS_DENIED;
    }
    if !wow64vas::is_valid_user_range(buffer, size) {
        return STATUS_ACCESS_DENIED;
    }

    let mut bytes_read: usize = 0;
    let status = unsafe {
        crate::libs::ntdll::virtual_mem::nt_read_virtual_memory(
            process_handle as usize as *mut core::ffi::c_void,
            base_address as usize as *mut core::ffi::c_void,
            buffer as usize as *mut core::ffi::c_void,
            size as usize,
            &mut bytes_read,
        )
    };
    if status >= 0 && number_of_bytes_read != 0 {
        let _ = crate::mm::user_copy::write_user_object(
            number_of_bytes_read as u64,
            &(bytes_read as u32),
        );
    }
    status as u32
}

// =============================================================================
// wow64_write_virtual_memory32
// =============================================================================

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// write memory to another process.
///
/// # Arguments
/// * `process_handle` - handle to the target process
/// * `base_address` - Address to write to
/// * `buffer` - Local buffer to write from (32-bit address)
/// * `size` - Number of bytes to write
/// * `number_of_bytes_written` - Number of bytes actually written (output)
///
/// # Returns
/// * NTSTATUS
pub unsafe extern "C" fn wow64_write_virtual_memory32(
    process_handle: HANDLE32,
    base_address: ULONG32,
    buffer: ULONG32,
    size: ULONG32,
    number_of_bytes_written: Ulong32Ptr,
) -> ULONG32 {
    crate::wow64_klog!(
        "Wow64WriteVirtualMemory32 handle=0x{:08x} to=0x{:08x} \
         from=0x{:08x} size=0x{:08x} bytes_written_ptr=0x{:08x}",
        process_handle,
        base_address,
        buffer,
        size,
        number_of_bytes_written
    );

    // Validate ranges
    if !wow64vas::is_valid_user_range(base_address, size) {
        return STATUS_ACCESS_DENIED;
    }
    if !wow64vas::is_valid_user_range(buffer, size) {
        return STATUS_ACCESS_DENIED;
    }

    let mut bytes_written: usize = 0;
    let status = unsafe {
        crate::libs::ntdll::virtual_mem::nt_write_virtual_memory(
            process_handle as usize as *mut core::ffi::c_void,
            base_address as usize as *mut core::ffi::c_void,
            buffer as usize as *mut core::ffi::c_void,
            size as usize,
            &mut bytes_written,
        )
    };
    if status >= 0 && number_of_bytes_written != 0 {
        let _ = crate::mm::user_copy::write_user_object(
            number_of_bytes_written as u64,
            &(bytes_written as u32),
        );
    }
    status as u32
}

// =============================================================================
// Helper Functions
// =============================================================================

/// Validate a memory protection value.
fn is_valid_protection(protect: ULONG32) -> bool {
    matches!(
        protect,
        memory_protect::PAGE_NOACCESS
            | memory_protect::PAGE_READONLY
            | memory_protect::PAGE_READWRITE
            | memory_protect::PAGE_WRITECOPY
            | memory_protect::PAGE_EXECUTE
            | memory_protect::PAGE_EXECUTE_READ
            | memory_protect::PAGE_EXECUTE_READWRITE
            | memory_protect::PAGE_EXECUTE_WRITECOPY
            | memory_protect::PAGE_GUARD
            | memory_protect::PAGE_NOCACHE
            | memory_protect::PAGE_WRITECOMBINE
    )
}

// =============================================================================
// Initialization
// =============================================================================

/// Initialize the memory thunk layer.
pub fn init() {
    crate::wow64_klog!("Initializing memory thunk layer");
    crate::wow64_klog!("Memory thunk layer initialized");
}

// =============================================================================
// Public Entry Points (for thunk.rs forwarding)
// =============================================================================

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `wow64_allocate_virtual_memory` — wrapper for wow64_allocate_virtual_memory32.
pub unsafe extern "C" fn wow64_allocate_virtual_memory(
    process_handle: u32,
    base_address: u32,
    zero_bits: u32,
    region_size: u32,
    allocation_type: u32,
    protect: u32,
) -> u32 {
    crate::wow64_klog!(
        "wow64_allocate_virtual_memory proc={:#x} base={:#x} zb={:#x} size={:#x} type={:#x} prot={:#x}",
        process_handle, base_address, zero_bits, region_size,
        allocation_type, protect
    );
    // Forward to the actual implementation
    wow64_allocate_virtual_memory32(
        process_handle,
        base_address,
        zero_bits,
        region_size,
        allocation_type,
        protect,
    )
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `wow64_free_virtual_memory` — wrapper for wow64_free_virtual_memory32.
pub unsafe extern "C" fn wow64_free_virtual_memory(
    process_handle: u32,
    base_address: u32,
    region_size: u32,
    free_type: u32,
) -> u32 {
    crate::wow64_klog!(
        "wow64_free_virtual_memory proc={:#x} base={:#x} size={:#x} type={:#x}",
        process_handle,
        base_address,
        region_size,
        free_type
    );
    // Forward to the actual implementation
    wow64_free_virtual_memory32(process_handle, base_address, region_size, free_type)
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `wow64_read_virtual_memory` — wrapper for wow64_read_virtual_memory32.
pub unsafe extern "C" fn wow64_read_virtual_memory(
    process_handle: u32,
    base_address: u32,
    buffer: u32,
    size: u32,
) -> u32 {
    crate::wow64_klog!(
        "wow64_read_virtual_memory proc={:#x} src={:#x} dst={:#x} size={}",
        process_handle,
        base_address,
        buffer,
        size
    );
    // Forward to the actual implementation
    wow64_read_virtual_memory32(
        process_handle,
        base_address,
        buffer,
        size,
        0, // number_of_bytes_read_ptr
    )
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `wow64_write_virtual_memory` — wrapper for wow64_write_virtual_memory32.
pub unsafe extern "C" fn wow64_write_virtual_memory(
    process_handle: u32,
    base_address: u32,
    buffer: u32,
    size: u32,
) -> u32 {
    crate::wow64_klog!(
        "wow64_write_virtual_memory proc={:#x} dst={:#x} src={:#x} size={}",
        process_handle,
        base_address,
        buffer,
        size
    );
    // Forward to the actual implementation
    wow64_write_virtual_memory32(
        process_handle,
        base_address,
        buffer,
        size,
        0, // number_of_bytes_written_ptr
    )
}
