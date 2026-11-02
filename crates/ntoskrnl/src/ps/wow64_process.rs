//
//! This module implements the 32-bit process creation functionality for Wow64.
//! It extends the standard 64-bit process creation with Wow64-specific fields
//! and initialization.
//
//! Key functions:
//!   * Wow64 process extension creation
//!   * PEB32 initialization
//!   * Wow64 VAS initialization
//!   * 32-bit system DLL loading
//
//!     References:
//!   * geoffchappell.com — WoW64 process creation

use crate::libs::wow64::types::*;
use crate::ps::process::{create_process_full, Eprocess};
use alloc::boxed::Box;

// =============================================================================
// Wow64 Process Extension
// =============================================================================

/// EPROCESS extension for Wow64 processes.
/// This structure is stored alongside the standard EPROCESS
/// and contains Wow64-specific information.
#[repr(C)]
#[derive(Default)]
pub struct EprocessWow64Extension {
    /// Whether this is a Wow64 process.
    pub is_wow64_process: bool,
    /// 32-bit PEB address (user-mode virtual address).
    pub peb32_address: ULONG32,
    /// Pointer to PEB32 structure (kernel-mode).
    pub peb32: *mut Peb32,
    /// Wow64 VAS manager pointer.
    pub wow64vas: *mut Wow64VasState,
    /// Base address of 32-bit ntdll.dll.
    pub ntdll32_base: ULONG32,
    /// Base address of 32-bit kernel32.dll.
    pub kernel32_base: ULONG32,
    /// Base address of 32-bit user32.dll.
    pub user32_base: ULONG32,
    /// Last 32-bit error code (nt_current_teb()->last_error_value).
    pub last_error32: ULONG32,
}

impl EprocessWow64Extension {
    /// create a new Wow64 extension.
    pub fn new() -> Self {
        Self {
            is_wow64_process: true,
            peb32_address: PEB32_VIRTUAL_ADDRESS,
            peb32: core::ptr::null_mut(),
            wow64vas: core::ptr::null_mut(),
            ntdll32_base: 0,
            kernel32_base: 0,
            user32_base: 0,
            last_error32: 0,
        }
    }

    /// Check if this process is a Wow64 process.
    pub fn is_wow64(&self) -> bool {
        self.is_wow64_process
    }

    /// Get the PEB32 address.
    pub fn get_peb32_address(&self) -> ULONG32 {
        self.peb32_address
    }

    /// Set the last error for the 32-bit process.
    pub fn set_last_error(&mut self, error: ULONG32) {
        self.last_error32 = error;
    }

    /// Get the last error for the 32-bit process.
    pub fn get_last_error(&self) -> ULONG32 {
        self.last_error32
    }
}

/// Wow64 VAS state for a single process.
pub struct Wow64VasState {
    /// Next allocation base (bump pointer).
    pub next_alloc: u32,
    /// End of user space (exclusive).
    pub end_of_space: u32,
    /// Number of allocations made.
    pub allocation_count: u32,
    /// Peak usage.
    pub peak_usage: u32,
}

impl Default for Wow64VasState {
    fn default() -> Self {
        Self {
            next_alloc: WOW64_USER_SPACE_START,
            end_of_space: WOW64_USER_SPACE_END,
            allocation_count: 0,
            peak_usage: 0,
        }
    }
}

impl Wow64VasState {
    /// create a new VAS state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Reset to initial state.
    pub fn reset(&mut self) {
        self.next_alloc = WOW64_USER_SPACE_START;
        self.allocation_count = 0;
    }

    /// Get current usage.
    pub fn current_usage(&self) -> u32 {
        self.next_alloc - WOW64_USER_SPACE_START
    }
}

// =============================================================================
// Wow64 Process Creation
// =============================================================================

/// create a new Wow64 (32-bit) process.
///
/// This function creates a new 32-bit process that will run under
/// the Wow64 compatibility layer.
///
/// # Arguments
/// * `image_path` - Path to the 32-bit executable
/// * `parent_process` - Parent process (optional)
///
/// # Returns
/// * `Some(&'static mut Eprocess)` on success
/// * `None` on failure
pub fn create_wow64_process(
    image_path: &[u8],
    parent_process: Option<&'static mut Eprocess>,
) -> Option<&'static mut Eprocess> {
    let parent = parent_process.map(|process| process as *mut Eprocess);
    let process = create_process_full(image_path, parent)?;
    let pid = process.unique_process_id;
    if initialize_wow64_process(process, image_path).is_none() {
        crate::ps::process::terminate_process(pid, 0xC0000005u32 as i32);
        return None;
    }
    crate::ps::process::get_by_pid(pid)
}

/// Initialize WOW64 process state after the native process object exists.
///
/// The image bytes are parsed and relocated through the PE32 loader so the
/// executable's preferred base and entry point drive the WOW64 user state.
pub fn initialize_wow64_process(
    process: &'static mut Eprocess,
    image: &[u8],
) -> Option<&'static mut Eprocess> {
    let loaded_image = crate::loader::pe32::load_pe32(image, 0)?;
    let image_base = loaded_image.base_address;
    let entry_point = loaded_image.entry_point();
    let image_size = loaded_image.size_of_image;

    map_wow64_image(process, &loaded_image)?;

    process.set_wow64_flag(true);
    process.set_wow64_peb32_va(PEB32_VIRTUAL_ADDRESS);
    process.user_image_base = u64::from(image_base);
    process.user_image_size = u64::from(image_size);
    process.user_rip = u64::from(entry_point);

    let mut peb32_box = Box::new(Peb32::default());
    let peb32 = &mut *peb32_box;
    let heap_base = crate::libs::wow64::wow64vas::allocate_heap(0x1000)?;
    unsafe {
        init_peb32(peb32, process);
        peb32.process_heap = heap_base;
    }
    process.wow64_peb32_ptr = peb32 as *mut Peb32 as u64;

    let mut wow64vas_box = Box::new(Wow64VasState::new());
    let wow64vas = &mut *wow64vas_box;
    process.wow64_vas_ptr = wow64vas as *mut Wow64VasState as u64;

    if !process.main_thread.is_null() {
        let stack_top = crate::libs::wow64::wow64vas::allocate_stack(
            crate::libs::wow64::wow64vas::WOW64_DEFAULT_STACK_SIZE,
        )?;
        let stack_base =
            stack_top.checked_sub(crate::libs::wow64::wow64vas::WOW64_DEFAULT_STACK_SIZE)?;

        unsafe {
            let thread = &mut *process.main_thread;
            let teb32 = crate::ps::wow64_thread::initialize_wow64_thread(
                thread,
                process,
                stack_top,
            );
            let Some(teb32) = teb32 else {
                drop(peb32_box);
                drop(wow64vas_box);
                cleanup_wow64_process_resources(
                    process,
                    heap_base,
                    stack_top,
                    None,
                );
                return None;
            };
            let teb32_va = crate::ps::wow64_thread::teb32_user_va(teb32);
            (*peb32).teb = teb32_va;
            (*peb32).teb32 = teb32_va;
            (*process.main_thread).kthread.context.user_rip = u64::from(entry_point);
            (*process.main_thread).kthread.context.user_rsp = u64::from(stack_top);
        }

        process.user_stack_base = u64::from(stack_base);
        process.user_stack_limit = u64::from(stack_top);
        process.user_rsp = u64::from(stack_top);
    }

    Box::leak(peb32_box);
    Box::leak(wow64vas_box);
    Some(process)
}

/// Return WOW64 allocations made before a failed initialization.
fn cleanup_wow64_process_resources(
    process: &mut Eprocess,
    heap_base: u32,
    stack_top: u32,
    teb32: Option<*mut crate::libs::wow64::types::Teb32>,
) {
    crate::libs::wow64::wow64vas::free_heap(heap_base);
    if stack_top != 0 {
        crate::libs::wow64::wow64vas::free_stack(stack_top);
    }
    if let Some(teb32) = teb32 {
        unsafe {
            crate::mm::pool::free(teb32.cast::<u8>());
        }
    }
    process.wow64_peb32_ptr = 0;
    process.wow64_vas_ptr = 0;
}

/// Map a parsed PE32 image into the WOW64 process's user address space.
///
/// The PE loader keeps its image in a temporary kernel buffer. WOW64 code
/// cannot execute from that buffer, so we allocate contiguous backing pages,
/// copy the image into them, and map them at the image's preferred base.
fn map_wow64_image(
    process: &Eprocess,
    image: &crate::loader::pe32::LoadedImage32,
) -> Option<u64> {
    const PAGE_SIZE: u64 = 0x1000;
    let image_size = u64::from(image.size_of_image);
    let page_count = image_size.div_ceil(PAGE_SIZE);
    let physical_base = crate::mm::frame::allocate_pages(page_count)?;

    let kernel_va = crate::mm::constants::KERNEL_DIRECT_MAP_BASE + physical_base;
    unsafe {
        core::ptr::copy_nonoverlapping(
            image.image_data.as_ptr(),
            kernel_va as *mut u8,
            image.image_data.len(),
        );
    }

    let user_base = u64::from(image.base_address);
    let flags = crate::mm::vas::PTE_RW | crate::mm::vas::PTE_US;
    for page in 0..page_count {
        let va = user_base + page * PAGE_SIZE;
        let pa = physical_base + page * PAGE_SIZE;
        if crate::mm::vas::map_page_in_pml4(process.pml4_phys, va, pa, flags)
            != crate::mm::vas::MmStatus::Ok
        {
            if page != 0 {
                let _ = crate::mm::vas::unmap_user_pages(
                    process.pml4_phys,
                    user_base,
                    page * PAGE_SIZE,
                );
            }
            crate::mm::frame::free_pages(physical_base, page_count);
            return None;
        }
    }

    Some(physical_base)
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// Initialize PEB32 for a Wow64 process.
unsafe fn init_peb32(peb: *mut Peb32, process: &Eprocess) {
    if peb.is_null() {
        return;
    }

    // Basic PEB32 initialization
    (*peb).image_base_address = process.user_image_base as u32;
    (*peb).being_debugged = 0; // Not being debugged initially
    (*peb).bit_field = 0;
    (*peb).ldr = 0; // Will be set when ntdll32 is loaded
    (*peb).process_parameters = 0; // Will be set by RTL

    // OS version information
    (*peb).os_major_version = 6;
    (*peb).os_minor_version = 1; // Windows 7
    (*peb).os_build_number = 7601;
}

/// Load 32-bit system DLLs into the Wow64 process.
///
/// These DLLs include:
///   * ntdll.dll - Native API
///   * kernel32.dll - Win32 kernel API
///   * user32.dll - Win32 user API
///   * gdi32.dll - Win32 GDI API
// =============================================================================
// Wow64 Process Query Functions
// =============================================================================
///     Get the PEB32 address for a Wow64 process.
pub fn get_peb32_address(process: &Eprocess) -> ULONG32 {
    process.get_wow64_peb32_va()
}

/// Check if a process is a Wow64 process.
pub fn is_wow64_process(process: &Eprocess) -> bool {
    process.is_wow64_process()
}

/// Get the last error for a 32-bit process.
pub fn get_wow64_last_error(process: &Eprocess) -> ULONG32 {
    process.get_wow64_last_error()
}

/// Set the last error for a 32-bit process.
pub fn set_wow64_last_error(process: &mut Eprocess, error: ULONG32) {
    process.set_wow64_last_error(error);
    if !process.main_thread.is_null() {
        unsafe {
            if let Some(teb32) = (*process.main_thread).get_wow64_teb32().as_mut() {
                teb32.last_error_value = error;
            }
        }
    }
}

/// Query whether 32-bit registry redirection is active for a process.
pub fn registry_redirection_enabled(process: &Eprocess) -> bool {
    process.is_wow64_process()
}

// =============================================================================
// Wow64 Process Information
// =============================================================================

/// Information class for Wow64 process queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum Wow64ProcessInformationClass {
    /// Return PEB32 address.
    Wow64Information = 0,
    /// Return whether process is using XeKeys.
    XeKeys = 1,
    /// Return session ID.
    SessionInformation = 2,
}

/// Query Wow64-specific process information.
pub fn query_wow64_information(
    process: &Eprocess,
    info_class: Wow64ProcessInformationClass,
) -> ULONG32 {
    match info_class {
        Wow64ProcessInformationClass::Wow64Information => {
            // Return non-zero if Wow64 process
            if is_wow64_process(process) {
                1
            } else {
                0
            }
        }
        Wow64ProcessInformationClass::XeKeys => {
            0 // XeKeys not used
        }
        Wow64ProcessInformationClass::SessionInformation => process.session_id,
    }
}

// =============================================================================
// Initialization
// =============================================================================

/// Initialize the Wow64 process module.
pub fn init() {
    crate::wow64_klog!("Initializing Wow64 process module");
    crate::wow64_klog!("Wow64 process module initialized");
}
