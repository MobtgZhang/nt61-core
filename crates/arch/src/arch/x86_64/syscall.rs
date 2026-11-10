//! x86_64 SYSCALL entry / dispatch — full NT 6.1 implementation.
//
//! This module wires up the AMD64 SYSCALL/SYSRET mechanism for
//! Ring 3 → Ring 0 transitions and dispatches every NT 6.1 native
//! system call to the matching kernel handler.
//
//! Architecture
//! ------------
//! On entry to Ring 0 (via the `syscall` instruction), the CPU
//! has already saved the user-mode RIP into RCX and the user-mode
//! RFLAGS into R11. The kernel trap frame is built by the
//! assembly entry point in `idt_stubs.rs` (`syscall_entry`); see
//! that file for the exact stack layout.
//
//! The `TrapFrame` Rust struct mirrors the on-stack layout. The
//! fields are read by the dispatch code to extract the user-mode
//! arguments (in the System V AMD64 ABI used by the Windows x64
//! calling convention, the first 4 arguments are passed in
//! RCX, RDX, R8, R9 and additional arguments are pushed onto
//! the stack).
//
//! Syscall numbers
//! ---------------
//! Each NT 6.1 native API has a stable numeric identifier. The
//! identifiers are public ABI and never change within a release.
//! The full list lives in `syscall_numbers.rs` and is referenced
//! from the dispatch table below; the table key is the syscall
//! number and the value is the kernel-side handler.
//
//! Implementation policy
//! ---------------------
//! For calls that already have a working kernel-side
//! implementation in `libs::ntdll::*`, the dispatch invokes that
//! function directly. For calls we have not yet implemented, the
//! dispatch returns `STATUS_NOT_IMPLEMENTED` so user-mode callers
//! see a deterministic NTSTATUS code instead of a #GP fault.

use core::sync::atomic::{AtomicU32, Ordering};

use alloc::boxed::Box;

/// User-controlled `SYS_FILE_READ` requests are capped so a Ring 3
/// caller cannot force an unbounded kernel allocation.
const SYS_FILE_READ_MAX_BYTES: u64 = 1 << 20;

// use crate::kprintln;

#[cfg(target_arch = "x86_64")]
#[cfg(target_arch = "x86_64")]
use self::syscall_numbers as nums;

// AMD64 MSRs for the SYSCALL mechanism.
const IA32_LSTAR: u32 = 0xC0000082; // long-mode SYSCALL target RIP
const IA32_STAR: u32 = 0xC0000081; // CS/SS selectors for SYSRET
const IA32_FMASK: u32 = 0xC0000084; // RFLAGS mask on SYSCALL
const IA32_EFER: u32 = 0xC0000080; // EFER (bit 0 = SCE = SYSCALL enable)
const IA32_KERNEL_GS_BASE: u32 = 0xC0000102; // Kernel GS base (used by SWAPGS)
const IA32_GS_BASE: u32 = 0xC0000101; // User GS base (set by kernel on swapgs)

/// Segment selectors used for SYSRET. All are re-exported from `gdt.rs`
/// which is the authoritative source for the OVMF-augmented GDT layout.
///
/// OVMF GDT (slots 0-3 are OVMF-preserved):
///   slot 2 (selector 0x10): kernel CS  (OVMF)
///   slot 3 (selector 0x18): kernel SS  (OVMF)
///
/// Kernel-augmented (slots 4-6):
///   slot 4 (selector 0x20): user SS    (DPL=3, written by gdt::init())
///   slot 5 (selector 0x28): user CS    (DPL=3, written by gdt::init())
///   slot 6 (selector 0x30): TSS        (written by gdt::init())
#[cfg(target_arch = "x86_64")]
#[cfg(target_arch = "x86_64")]
pub use crate::arch::x86_64::gdt::KERNEL_CS;
#[cfg(target_arch = "x86_64")]
pub use crate::arch::x86_64::gdt::KERNEL_DS;
#[cfg(target_arch = "x86_64")]
pub use crate::arch::x86_64::gdt::USER_CS;
#[cfg(target_arch = "x86_64")]
pub use crate::arch::x86_64::gdt::USER_SS;

/// offset of the user-RSP slot inside the per-CPU area. The
/// `syscall_entry` asm writes `gs:[0x0] = user_rsp` on entry
/// and reads it back on `sysretq`.
pub const PER_CPU_USER_RSP_OFFSET: usize = 0x0;
/// offset of the kernel-RSP slot. The asm loads `gs:[0x8]` on
/// entry to switch to the kernel stack.
pub const PER_CPU_KERNEL_RSP_OFFSET: usize = 0x8;
/// offset of the `current_thread` slot. The kernel writes the
/// ETHREAD pointer of the running thread here in `setup_bsp`
/// (and again on every context switch). All `gs:[N]` readers
/// (`KeGetCurrentEthread`, `get_current_ethread`, ...) MUST
/// use this offset.
pub const PER_CPU_CURRENT_THREAD_OFFSET: usize =
    crate::arch::x86_64::percpu_impl::OFFSET_CURRENT_THREAD;
/// offset of the `syscall_snap_addr` slot. The `syscall_entry`
/// asm reads `gs:[0x58]` to load `&SYSCALL_ENTRY_SNAP` into RAX
/// without needing a 64-bit LEA or a memory LOAD. The slot is
/// populated by `init_syscall_msrs()` in `syscall.rs`.
pub const PER_CPU_SYSCALL_SNAP_ADDR_OFFSET: usize = 0x58;
/// offset of the `current_process` slot.
pub const PER_CPU_CURRENT_PROCESS_OFFSET: usize =
    crate::arch::x86_64::percpu_impl::OFFSET_CURRENT_PROCESS;

// =====================================================================
// Canonical per-CPU re-exports
// =====================================================================
//
// All per-CPU infrastructure is defined in the canonical locations:
//   - Struct definition + main API:   crate::arch::common::percpu
#[cfg(target_arch = "x86_64")]
#[cfg(target_arch = "x86_64")]
//   - x86_64 GS-base implementation:  crate::arch::x86_64::percpu_impl
//
// This module re-exports everything under the syscall::* namespace so
// that callers (scheduler.rs, smp.rs, irql.rs, ...) can use a stable
// API regardless of where the underlying implementation lives.
// =====================================================================
pub use crate::arch::common::percpu::{
    get_current, get_current_cpu_id, get_current_process, get_current_thread, init as percpu_init,
    set_current_process, set_current_thread, set_kernel_stack, set_user_rsp, PerCpuArea,
    MAX_PER_CPU, PER_CPU_AREA_SIZE,
};

#[cfg(target_arch = "x86_64")]
#[cfg(target_arch = "x86_64")]
pub use crate::arch::x86_64::percpu_impl::{
    __percpu_get_current as per_cpu_ptr_mut,
    __percpu_get_current_cpu_id as get_current_cpu_id_from_gs, allocate as allocate_per_cpu_area,
    init_storage,
};

/// Safe wrapper for percpu init (unsafe call).
pub fn init(cpu_id: u32) -> u64 {
    unsafe { percpu_init(cpu_id) }
}

/// Legacy alias for init.
pub fn init_per_cpu(cpu_id: u32) -> u64 {
    init(cpu_id)
}

/// Return a raw pointer to the BSP per-CPU area. Used by the
/// MM code to publish the system / user PML4 pair so the
/// syscall / interrupt stubs can switch CR3 to the system PML4
/// before running kernel handlers.
///
/// Returns `core::ptr::null_mut()` if the per-CPU area has not
/// been initialised yet.
pub fn get_per_cpu() -> *mut PerCpuArea {
    per_cpu_ptr_mut() as *mut PerCpuArea
}

/// The on-stack register save area built by `syscall_entry`.
/// Field order MUST match the assembly push order in
/// `idt_stubs.rs`. See the comment at the top of that block.
pub use crate::arch::x86_64::dispatch::TrapFrame;

/// Snapshot of RAX (= user-mode syscall number) taken at the very
/// first instruction of `syscall_entry` (in `idt_stubs.rs`). This
/// captures the value BEFORE the Rust compiler can clobber RAX
/// in the dispatcher's prologue. Useful for verifying that the
/// user-mode stub actually set RAX to the expected syscall
/// number before executing `syscall`.
///
/// Layout (24 bytes):
///   [0..8]   = RAX at syscall_entry (user-mode syscall number)
///   [8..16]  = RCX at syscall_entry (user-mode RIP)
///   [16..24] = 8 bytes from user RIP (bytes the CPU would
///              execute next)
#[repr(C)]
pub struct SyscallEntrySnap {
    pub rax: u64,
    pub rip: u64,
    pub rip_bytes: [u8; 8],
    pub user_r13: u64,
    pub user_r12: u64,
}

#[no_mangle]
pub static mut SYSCALL_ENTRY_SNAP: SyscallEntrySnap = SyscallEntrySnap {
    rax: 0xDEAD_BEEF_DEAD_BEEF,
    rip: 0xDEAD_BEEF_DEAD_BEEF,
    rip_bytes: [0xCC; 8],
    user_r13: 0xDEAD_BEEF_DEAD_BEEF,
    user_r12: 0xDEAD_BEEF_DEAD_BEEF,
};

// (The previous per-field address-symbol helpers
//  SYSCALL_ENTRY_SNAP_ADDR_* and init_snap_addrs() have been
//  removed: the assembly now uses `sym SYSCALL_ENTRY_SNAP` once
//  for the base and offsets from the assembly side, which works
//  without any runtime address patching.)

// Back-compat aliases for the existing print sites.
/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
#[inline(always)]
pub unsafe fn syscall_entry_rax_snap() -> u64 {
    core::ptr::read_volatile(&raw const SYSCALL_ENTRY_SNAP.rax)
}
/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
#[inline(always)]
pub unsafe fn syscall_entry_rip_snap() -> u64 {
    core::ptr::read_volatile(&raw const SYSCALL_ENTRY_SNAP.rip)
}
/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
#[inline(always)]
pub unsafe fn syscall_entry_rip_byte_snap() -> [u8; 8] {
    core::ptr::read_volatile(&raw const SYSCALL_ENTRY_SNAP.rip_bytes)
}

extern "C" {
    fn syscall_entry();
}

// =====================================================================
// MSR helpers
// =====================================================================

#[inline(always)]
fn rdmsr(msr: u32) -> u64 {
    let lo: u32;
    let hi: u32;
    unsafe {
        core::arch::asm!(
            "rdmsr",
            in("ecx") msr,
            out("eax") lo,
            out("edx") hi,
            options(nostack, preserves_flags),
        );
    }
    ((hi as u64) << 32) | (lo as u64)
}

#[inline(always)]
fn wrmsr(msr: u32, val: u64) {
    let lo = val as u32;
    let hi = (val >> 32) as u32;
    unsafe {
        core::arch::asm!(
            "wrmsr",
            in("ecx") msr,
            in("eax") lo,
            in("edx") hi,
            options(nostack, preserves_flags),
        );
    }
}

pub fn set_kernel_gs_base(base: u64) {
    wrmsr(IA32_KERNEL_GS_BASE, base);
}
pub fn set_user_gs_base(base: u64) {
    wrmsr(IA32_GS_BASE, base);
}
pub fn get_user_gs_base() -> u64 {
    rdmsr(IA32_GS_BASE)
}
pub fn get_kernel_gs_base() -> u64 {
    rdmsr(IA32_KERNEL_GS_BASE)
}

// =====================================================================
// Argument extraction helpers
// =====================================================================
//
// The Microsoft x64 calling convention (used by Windows 7 / NT 6.1
// on x86-64) is the standard AMD64 calling convention *except*
// for the SYSCALL mechanism: the AMD64 `syscall` instruction
// clobbers RCX (it becomes the user RIP), so the standard "arg0
// in rcx" rule cannot be honoured. The Microsoft convention
// instead is:
//
//     arg0 = R10
//     arg1 = RDX
//     arg2 = R8
//     arg3 = R9
//     arg4 = stack[0]
//     arg5 = stack[1]
//     ...
//
// Stack arguments live at offsets from the user RSP at the time
// of the `syscall` instruction. We save user RSP into the trap
// frame's `rsp` field on entry, and stack args are read relative
// to that pointer.
//
// The ntdll thunk is a normal function, so the caller's `call`
// pushed a return address at [rsp]. The first stack argument is
// therefore at [rsp + 8], not [rsp + 0]. (Shadow space lives
// below the return address, at [rsp - 32 .. rsp - 8], so it is
// not part of the argument area.)

/// offset of the first stack argument from the user RSP recorded
/// in the trap frame: 8 bytes for the return address.
const STACK_ARG_BASE: u64 = 8;
//
// This is the convention that every `ntdll.dll` thunk on the
// user-mode side uses; the kernel reads arguments back from
// exactly these registers.
// =====================================================================

#[inline(always)]
pub fn arg0(tf: &TrapFrame) -> u64 {
    tf.r10
}
#[inline(always)]
pub fn arg1(tf: &TrapFrame) -> u64 {
    tf.rdx
}
#[inline(always)]
pub fn arg2(tf: &TrapFrame) -> u64 {
    tf.r8
}
#[inline(always)]
pub fn arg3(tf: &TrapFrame) -> u64 {
    tf.r9
}
#[inline(always)]
fn arg4(tf: &TrapFrame) -> u64 {
    stack_arg(tf, 0)
}
#[inline(always)]
fn arg5(tf: &TrapFrame) -> u64 {
    stack_arg(tf, 1)
}

/// read a stack-passed argument. `n` is the index of the
/// argument (0 = first stack arg, 1 = second stack arg, ...).
/// On entry to the kernel, the user RSP points at the return
/// address of the ntdll thunk; the first stack argument follows
/// it at `rsp + 8`.
//
// The user RSP is attacker-controlled, so it is never dereferenced
// directly. Every read goes through `read_stack_arg`, which probes
// the covering page first (see `syscall_validation`): an unmapped
// or kernel-half RSP is rejected instead of faulting inside the
// kernel. A rejected read yields 0 and marks the dispatch as
// invalid so the caller gets `STATUS_ACCESS_VIOLATION`.
#[inline(always)]
pub fn stack_arg(tf: &TrapFrame, n: usize) -> u64 {
    match read_stack_arg(tf, n) {
        Some(v) => v,
        None => {
            mark_bad_stack_arg();
            0
        }
    }
}

/// Probe then read one stack argument. Returns `None` when the
/// slot is not validated user memory.
#[inline]
fn read_stack_arg(tf: &TrapFrame, n: usize) -> Option<u64> {
    let addr = tf.rsp.checked_add(STACK_ARG_BASE + n as u64 * 8)?;
    unsafe {
        if crate::arch::x86_64::syscall_validation::probe_for_read(addr as *const u8, 8).is_err() {
            return None;
        }
        let mut value = 0u64;
        if crate::mm::user_copy::read_user(addr, &mut value).is_err() {
            return None;
        }
        Some(value)
    }
}

/// Set when a stack argument failed validation. `dispatch` checks
/// this after the handler runs so a handler that dereferences a
/// bad argument still reports an access violation.
/// Per-CPU bad-stack-argument bitmap. A global scalar is insufficient
/// because concurrent syscalls on different CPUs would clear each
/// other's validation state.
static BAD_STACK_ARG_BITMAP: AtomicU32 = AtomicU32::new(0);

#[inline]
fn mark_bad_stack_arg() {
    let cpu = crate::arch::common::percpu::get_current_cpu_id() as usize;
    if cpu < 32 {
        BAD_STACK_ARG_BITMAP.fetch_or(1 << cpu, Ordering::AcqRel);
    }
}

#[inline]
fn clear_bad_stack_arg() {
    let cpu = crate::arch::common::percpu::get_current_cpu_id() as usize;
    if cpu < 32 {
        BAD_STACK_ARG_BITMAP.fetch_and(!(1u32 << cpu), Ordering::AcqRel);
    }
}

/// Validate the argument window the handler will read.
///
/// The service number encodes the argument size in bits 16-31
/// (bytes, i.e. parameter count * 8). Only the parameters beyond
/// the four register arguments live on the user stack, so the
/// window to probe is `(arg_size - 32)` bytes starting at the
/// return-address slot. This mirrors what `KiSystemCall64` does
/// on Windows 7 x64: the parameter area is probed before the
/// handler is allowed to touch it.
fn validate_stack_args(tf: &TrapFrame, syscall_num: u32) -> i32 {
    let table_index = (syscall_num >> 12) & 0xF;
    let arg_size = if table_index == 1 {
        // Shadow (win32k) services encode their stack argument count in
        // the per-service table rather than bits 16-31 of the number.
        crate::ke::ssdt::get_shadow_argument_size(syscall_num) as usize
    } else {
        crate::ke::ssdt::get_argument_size(syscall_num) as usize
    };
    if arg_size == 0 {
        return crate::arch::x86_64::syscall_validation::STATUS_SUCCESS;
    }
    if arg_size > crate::ke::ssdt::SSDT_MAX_ARGUMENT_SIZE {
        return crate::arch::x86_64::syscall_validation::STATUS_INVALID_PARAMETER;
    }
    let stack_bytes = arg_size.saturating_sub(4 * 8);
    if stack_bytes == 0 {
        return crate::arch::x86_64::syscall_validation::STATUS_SUCCESS;
    }
    let base = match tf.rsp.checked_add(STACK_ARG_BASE) {
        Some(v) => v,
        None => return crate::arch::x86_64::syscall_validation::STATUS_ACCESS_VIOLATION,
    };
    unsafe {
        crate::arch::x86_64::syscall_validation::probe_for_read(base as *const u8, stack_bytes)
            .err()
            .unwrap_or(crate::arch::x86_64::syscall_validation::STATUS_SUCCESS)
    }
}

// =====================================================================
// Dispatch
// =====================================================================

pub type SyscallResult = i64;

#[inline(always)]
fn not_implemented() -> SyscallResult {
    crate::libs::ntdll::status::STATUS_NOT_IMPLEMENTED as i64
}

#[inline(always)]
fn success() -> SyscallResult {
    0
}

/// Entry point for a native service call.
///
/// The user stack window is validated before any handler runs, and
/// re-checked afterwards in case a handler read beyond the declared
/// argument window. A failed validation is reported as
/// `STATUS_ACCESS_VIOLATION` rather than faulting in Ring 0.
fn dispatch(syscall_num: u32, tf: &TrapFrame) -> SyscallResult {
    clear_bad_stack_arg();
    let status = validate_stack_args(tf, syscall_num);
    if status != 0 {
        return status as SyscallResult;
    }

    // Route through the common SSDT when a handler is registered for
    // this service number. The direct match in `dispatch_service`
    // handles services that only exist in the x86_64-specific table.
    if crate::ke::ssdt_common::has_entry(syscall_num as usize) {
        let args = build_ssdt_arguments(tf);
        if let Some(status) =
            unsafe { crate::ke::ssdt_common::try_dispatch(syscall_num as usize, &args) }
        {
            return status as i32 as SyscallResult;
        }
    }

    let result = dispatch_service(syscall_num, tf);
    if BAD_STACK_ARG_BITMAP.load(Ordering::Acquire) != 0 {
        return crate::arch::x86_64::syscall_validation::STATUS_ACCESS_VIOLATION as SyscallResult;
    }
    result
}

/// Build the architecture-independent `SyscallArguments` from the
/// x86_64 trap frame. The first four arguments live in R10, RDX, R8,
/// and R9; arguments 4 through 7 are read from the user stack.
fn build_ssdt_arguments(tf: &TrapFrame) -> crate::ke::ssdt_common::SyscallArguments {
    crate::ke::ssdt_common::SyscallArguments {
        register_args: [
            arg0(tf) as usize,
            arg1(tf) as usize,
            arg2(tf) as usize,
            arg3(tf) as usize,
            stack_arg(tf, 0) as usize,
            stack_arg(tf, 1) as usize,
            stack_arg(tf, 2) as usize,
            stack_arg(tf, 3) as usize,
        ],
        integer_register_count: crate::ke::ssdt_common::INTEGER_REGISTER_ARGUMENTS,
        stack_pointer: (tf.rsp + STACK_ARG_BASE + 4 * core::mem::size_of::<u64>() as u64) as usize,
    }
}

fn dispatch_service(syscall_num: u32, tf: &TrapFrame) -> SyscallResult {
    use crate::libs::ntdll::status::STATUS_ACCESS_VIOLATION;
    use crate::libs::ntdll::status::STATUS_INVALID_PARAMETER;
    use crate::libs::ntdll::status::STATUS_IO_DEVICE_ERROR;
    use crate::libs::ntdll::status::STATUS_SUCCESS;
    use crate::libs::ntdll::types::{
        client_id, IoStatusBlock, ObjectAttributes, SizeT, UnicodeString, HANDLE, PVOID,
    };

    // Check for Shadow SSDT (win32k) services
    // Shadow services are in range 0x1000-0x1FFF
    // Table index is bits 12-15 (>> 12)
    let table_index = (syscall_num >> 12) & 0xF;
    if table_index == 0x1 {
        // Shadow SSDT (win32k.sys) service
        let syscall_frame = ntoskrnl::arch::common::syscall_frame::SyscallFrame::from_args([
            arg0(tf),
            arg1(tf),
            arg2(tf),
            arg3(tf),
            stack_arg(tf, 0),
            stack_arg(tf, 1),
            stack_arg(tf, 2),
            stack_arg(tf, 3),
            stack_arg(tf, 4),
            stack_arg(tf, 5),
            stack_arg(tf, 6),
            stack_arg(tf, 7),
        ]);
        let result =
            ntoskrnl::ke::shadow_ssdt::dispatch_shadow_service(syscall_num, &syscall_frame);
        return result as SyscallResult;
    }

    match syscall_num {
        // ---- Process / Thread ----
        nums::NT_CREATE_PROCESS => {
            if !validate_output(arg0(tf), 4)
                || !validate_write(arg2(tf), size_of::<ObjectAttributes>())
            {
                return STATUS_ACCESS_VIOLATION as SyscallResult;
            }
            unsafe {
                crate::libs::ntdll::process::nt_create_process(
                    arg0(tf) as *mut HANDLE,
                    arg1(tf) as u32,
                    arg2(tf) as *mut ObjectAttributes,
                    arg3(tf) as HANDLE,
                    arg4(tf) as u8,
                    arg5(tf) as HANDLE,
                    stack_arg(tf, 0) as PVOID,
                    stack_arg(tf, 1) as PVOID,
                ) as i64
            }
        }
        nums::NT_CREATE_PROCESS_EX => {
            if !validate_output(arg0(tf), 4)
                || !validate_write(arg2(tf), size_of::<ObjectAttributes>())
            {
                return STATUS_ACCESS_VIOLATION as SyscallResult;
            }
            unsafe {
                crate::libs::ntdll::process::nt_create_process_ex(
                    arg0(tf) as *mut HANDLE,
                    arg1(tf) as u32,
                    arg2(tf) as *mut ObjectAttributes,
                    arg3(tf) as HANDLE,
                    arg4(tf) as u32,
                    arg5(tf) as HANDLE,
                    stack_arg(tf, 0) as PVOID,
                    stack_arg(tf, 1) as PVOID,
                    stack_arg(tf, 2) as u32,
                ) as i64
            }
        }
        nums::NT_OPEN_PROCESS => {
            if !validate_output(arg0(tf), 4)
                || !validate_write(arg2(tf), size_of::<ObjectAttributes>())
                || !validate_write(arg3(tf), size_of::<client_id>())
            {
                return STATUS_ACCESS_VIOLATION as SyscallResult;
            }
            unsafe {
                crate::libs::ntdll::process::nt_open_process(
                    arg0(tf) as *mut HANDLE,
                    arg1(tf) as u32,
                    arg2(tf) as *mut ObjectAttributes,
                    arg3(tf) as *mut client_id,
                ) as i64
            }
        }
        nums::NT_TERMINATE_PROCESS => unsafe {
            // Terminate takes only a kernel HANDLE and an exit status:
            // there is no user pointer to probe here, and the helper
            // validates the handle against the process table.
            crate::libs::ntdll::process::nt_terminate_process(arg0(tf) as HANDLE, arg1(tf) as i32)
                as i64
        },
        nums::NT_QUERY_INFORMATION_PROCESS => {
            let length = arg3(tf) as usize;
            if !validate_write(arg2(tf), length) || !validate_output(arg4(tf), 4) {
                return STATUS_ACCESS_VIOLATION as SyscallResult;
            }
            unsafe {
                crate::libs::ntdll::process::nt_query_information_process(
                    arg0(tf) as HANDLE,
                    arg1(tf) as u32,
                    arg2(tf) as PVOID,
                    arg3(tf) as u32,
                    arg4(tf) as *mut u32,
                ) as i64
            }
        }
        nums::NT_SET_INFORMATION_PROCESS => unsafe {
            // arg2 is a user input buffer of arg3 bytes that the
            // helper dereferences; probe it before handing it over.
            if !validate_read(arg2(tf), arg3(tf) as usize) {
                return STATUS_ACCESS_VIOLATION as SyscallResult;
            }
            crate::libs::ntdll::process::nt_set_information_process(
                arg0(tf) as HANDLE,
                arg1(tf) as u32,
                arg2(tf) as PVOID,
                arg3(tf) as u32,
            ) as i64
        },
        nums::NT_TRIM_PROCESS_WORKING_SET => {
            // NT_TRIM_PROCESS_WORKING_SET(handle, min_ws, max_ws)
            crate::mm::working_set::mm_trim_process_working_set(arg0(tf), arg1(tf) as i64) as i64
        }

        // ---- Virtual Memory ----
        nums::NT_ALLOCATE_VIRTUAL_MEMORY => unsafe {
            crate::libs::ntdll::virtual_mem::nt_allocate_virtual_memory(
                arg0(tf) as HANDLE,
                arg1(tf) as *mut PVOID,
                arg2(tf) as usize,
                arg3(tf) as *mut SizeT,
                arg4(tf) as u32,
                arg5(tf) as u32,
            ) as i64
        },
        nums::NT_FREE_VIRTUAL_MEMORY => unsafe {
            crate::libs::ntdll::virtual_mem::nt_free_virtual_memory(
                arg0(tf) as HANDLE,
                arg1(tf) as *mut PVOID,
                arg2(tf) as *mut SizeT,
                arg3(tf) as u32,
            ) as i64
        },
        nums::NT_PROTECT_VIRTUAL_MEMORY => unsafe {
            crate::libs::ntdll::virtual_mem::nt_protect_virtual_memory(
                arg0(tf) as HANDLE,
                arg1(tf) as *mut PVOID,
                arg2(tf) as *mut SizeT,
                arg3(tf) as u32,
                arg4(tf) as *mut u32,
            ) as i64
        },
        nums::NT_QUERY_VIRTUAL_MEMORY => unsafe {
            crate::libs::ntdll::virtual_mem::nt_query_virtual_memory(
                arg0(tf) as HANDLE,
                arg1(tf) as PVOID,
                arg2(tf) as u32,
                arg3(tf) as PVOID,
                arg4(tf) as usize,
                arg5(tf) as *mut usize,
            ) as i64
        },
        nums::NT_READ_VIRTUAL_MEMORY => unsafe {
            crate::libs::ntdll::virtual_mem::nt_read_virtual_memory(
                arg0(tf) as HANDLE,
                arg1(tf) as PVOID,
                arg2(tf) as PVOID,
                arg3(tf) as usize,
                arg4(tf) as *mut usize,
            ) as i64
        },
        nums::NT_WRITE_VIRTUAL_MEMORY => unsafe {
            crate::libs::ntdll::virtual_mem::nt_write_virtual_memory(
                arg0(tf) as HANDLE,
                arg1(tf) as PVOID,
                arg2(tf) as PVOID,
                arg3(tf) as usize,
                arg4(tf) as *mut usize,
            ) as i64
        },

        // ---- File I/O ----
        nums::NT_CLOSE => unsafe {
            // Close takes a single kernel HANDLE; there is no user
            // pointer to probe, and the helper validates the handle.
            crate::libs::ntdll::file::nt_close(arg0(tf) as HANDLE) as i64
        },
        nums::NT_CREATE_FILE => unsafe {
            crate::libs::ntdll::file::nt_create_file(
                arg0(tf) as *mut HANDLE,
                arg1(tf) as u32,
                arg2(tf) as *mut ObjectAttributes,
                arg3(tf) as *mut IoStatusBlock,
                stack_arg(tf, 0) as *mut i64,
                stack_arg(tf, 1) as u32,
                stack_arg(tf, 2) as u32,
                stack_arg(tf, 3) as u32,
                stack_arg(tf, 4) as u32,
                stack_arg(tf, 5) as PVOID,
                stack_arg(tf, 6) as u32,
            ) as i64
        },
        nums::NT_OPEN_FILE => unsafe {
            crate::libs::ntdll::file::nt_open_file(
                arg0(tf) as *mut HANDLE,
                arg1(tf) as u32,
                arg2(tf) as *mut ObjectAttributes,
                arg3(tf) as *mut IoStatusBlock,
                arg4(tf) as u32,
                arg5(tf) as u32,
            ) as i64
        },
        nums::NT_READ_FILE => unsafe {
            crate::libs::ntdll::file::nt_read_file(
                arg0(tf) as HANDLE,
                arg1(tf) as HANDLE,
                arg2(tf) as PVOID,
                arg3(tf) as PVOID,
                stack_arg(tf, 0) as *mut IoStatusBlock,
                stack_arg(tf, 1) as PVOID,
                stack_arg(tf, 2) as u32,
                stack_arg(tf, 3) as *mut i64,
                stack_arg(tf, 4) as *mut u32,
            ) as i64
        },
        nums::NT_WRITE_FILE => unsafe {
            crate::libs::ntdll::file::nt_write_file(
                arg0(tf) as HANDLE,
                arg1(tf) as HANDLE,
                arg2(tf) as PVOID,
                arg3(tf) as PVOID,
                stack_arg(tf, 0) as *mut IoStatusBlock,
                stack_arg(tf, 1) as PVOID,
                stack_arg(tf, 2) as u32,
                stack_arg(tf, 3) as *mut i64,
                stack_arg(tf, 4) as *mut u32,
            ) as i64
        },
        nums::NT_QUERY_INFORMATION_FILE => unsafe {
            crate::libs::ntdll::file::nt_query_information_file(
                arg0(tf) as HANDLE,
                arg1(tf) as *mut IoStatusBlock,
                arg2(tf) as PVOID,
                arg3(tf) as u32,
                stack_arg(tf, 0) as u32,
            ) as i64
        },
        nums::NT_SET_INFORMATION_FILE => unsafe {
            crate::libs::ntdll::file::nt_set_information_file(
                arg0(tf) as HANDLE,
                arg1(tf) as *mut IoStatusBlock,
                arg2(tf) as PVOID,
                arg3(tf) as u32,
                stack_arg(tf, 0) as u32,
            ) as i64
        },
        nums::NT_FLUSH_BUFFERS_FILE => unsafe {
            crate::libs::ntdll::file::nt_flush_buffers_file(
                arg0(tf) as HANDLE,
                arg1(tf) as *mut IoStatusBlock,
            ) as i64
        },
        nums::NT_DELETE_FILE => unsafe {
            crate::libs::ntdll::file::nt_delete_file(arg0(tf) as *mut ObjectAttributes) as i64
        },
        nums::NT_DEVICE_IO_CONTROL_FILE => {
            // (HANDLE FileHandle, HANDLE Event, PVOID ApcRoutine,
            //  PVOID ApcContext, PIO_STATUS_BLOCK IoStatusBlock,
            //  ULONG IoControlCode, PVOID InputBuffer,
            //  ULONG InputBufferLength, PVOID OutputBuffer,
            //  ULONG OutputBufferLength)
            unsafe {
                crate::libs::ntdll::ioctl::nt_device_io_control_file(
                    arg0(tf) as HANDLE,
                    arg1(tf) as HANDLE,
                    arg2(tf) as PVOID,
                    arg3(tf) as PVOID,
                    arg4(tf) as *mut IoStatusBlock,
                    arg5(tf) as u32,
                    stack_arg(tf, 0) as PVOID,
                    stack_arg(tf, 1) as u32,
                    stack_arg(tf, 2) as PVOID,
                    stack_arg(tf, 3) as u32,
                ) as i64
            }
        }

        // ---- Sections ----
        nums::NT_CREATE_SECTION => unsafe {
            crate::libs::ntdll::section::nt_create_section(
                arg0(tf) as *mut HANDLE,
                arg1(tf) as u32,
                arg2(tf) as *mut ObjectAttributes,
                arg3(tf) as *mut i64,
                arg4(tf) as u32,
                arg5(tf) as u32,
                stack_arg(tf, 0) as HANDLE,
            ) as i64
        },
        nums::NT_UNMAP_VIEW_OF_SECTION => unsafe {
            crate::libs::ntdll::section::nt_unmap_view_of_section(
                arg0(tf) as HANDLE,
                arg1(tf) as PVOID,
            ) as i64
        },

        // ---- Synchronization primitives ----
        nums::NT_CREATE_EVENT => unsafe {
            if !validate_output(arg0(tf), size_of::<HANDLE>())
                || !validate_write(arg2(tf), size_of::<ObjectAttributes>())
            {
                return STATUS_ACCESS_VIOLATION as SyscallResult;
            }
            crate::libs::ntdll::sync::nt_create_event(
                arg0(tf) as *mut HANDLE,
                arg1(tf) as u32,
                arg2(tf) as *mut ObjectAttributes,
                arg3(tf) as u32,
                arg4(tf) as u8,
            ) as i64
        },
        nums::NT_SET_EVENT => unsafe {
            if !validate_output(arg1(tf), 4) {
                return STATUS_ACCESS_VIOLATION as SyscallResult;
            }
            crate::libs::ntdll::sync::nt_set_event(arg0(tf) as HANDLE, arg1(tf) as *mut u32) as i64
        },
        nums::NT_RESET_EVENT => unsafe {
            if !validate_output(arg1(tf), 4) {
                return STATUS_ACCESS_VIOLATION as SyscallResult;
            }
            crate::libs::ntdll::sync::nt_reset_event(arg0(tf) as HANDLE, arg1(tf) as *mut u32)
                as i64
        },
        nums::NT_PULSE_EVENT => unsafe {
            if !validate_output(arg1(tf), 4) {
                return STATUS_ACCESS_VIOLATION as SyscallResult;
            }
            crate::libs::ntdll::sync::nt_pulse_event(arg0(tf) as HANDLE, arg1(tf) as *mut u32)
                as i64
        },
        nums::NT_CREATE_MUTANT => unsafe {
            crate::libs::ntdll::sync::nt_create_mutant(
                arg0(tf) as *mut HANDLE,
                arg1(tf) as u32,
                arg2(tf) as *mut ObjectAttributes,
                arg3(tf) as u8,
            ) as i64
        },
        nums::NT_CREATE_SEMAPHORE => unsafe {
            crate::libs::ntdll::sync::nt_create_semaphore(
                arg0(tf) as *mut HANDLE,
                arg1(tf) as u32,
                arg2(tf) as *mut ObjectAttributes,
                arg3(tf) as u32,
                arg4(tf) as u32,
            ) as i64
        },
        nums::NT_RELEASE_SEMAPHORE => unsafe {
            crate::libs::ntdll::sync::nt_release_semaphore(
                arg0(tf) as HANDLE,
                arg1(tf) as u32,
                arg2(tf) as *mut u32,
            ) as i64
        },
        nums::NT_CREATE_TIMER => unsafe {
            crate::libs::ntdll::sync::nt_create_timer(
                arg0(tf) as *mut HANDLE,
                arg1(tf) as u32,
                arg2(tf) as *mut ObjectAttributes,
                arg3(tf) as u32,
            ) as i64
        },
        nums::NT_SET_TIMER => unsafe {
            crate::libs::ntdll::sync::nt_set_timer(
                arg0(tf) as crate::libs::ntdll::types::HANDLE,
                arg1(tf) as *const i64,
                arg2(tf) as u32,
                arg3(tf) as *const (),
                arg4(tf) as *mut (),
                arg5(tf) as u8,
                stack_arg(tf, 0) as *mut crate::libs::ntdll::types::HANDLE,
            ) as i64
        },
        nums::NT_CANCEL_TIMER => unsafe {
            crate::libs::ntdll::sync::nt_cancel_timer(
                arg0(tf) as crate::libs::ntdll::types::HANDLE,
                arg1(tf) as *mut crate::libs::ntdll::types::HANDLE,
            ) as i64
        },
        nums::NT_WAIT_FOR_SINGLE_OBJECT => unsafe {
            crate::libs::ntdll::sync::nt_wait_for_single_object(
                arg0(tf) as HANDLE,
                arg1(tf) as u8,
                arg2(tf) as *mut i64,
            ) as i64
        },
        nums::NT_WAIT_FOR_MULTIPLE_OBJECTS => unsafe {
            crate::libs::ntdll::sync::nt_wait_for_multiple_objects(
                arg0(tf) as u32,
                arg1(tf) as *mut HANDLE,
                arg2(tf) as u32,
                arg3(tf) as u8,
                arg4(tf) as *mut i64,
            ) as i64
        },
        nums::NT_DELAY_EXECUTION => {
            crate::libs::ntdll::sync::nt_delay_execution(arg0(tf) as u8, arg1(tf) as *mut i64)
                as i64
        }

        // ---- Registry ----
        nums::NT_CREATE_KEY => unsafe {
            crate::libs::ntdll::registry::nt_create_key(
                arg0(tf) as *mut HANDLE,
                arg1(tf) as u32,
                arg2(tf) as *mut ObjectAttributes,
                arg3(tf) as u32,
                arg4(tf) as *mut UnicodeString,
                arg5(tf) as u32,
                stack_arg(tf, 0) as *mut u32,
            ) as i64
        },
        nums::NT_OPEN_KEY => unsafe {
            crate::libs::ntdll::registry::nt_open_key(
                arg0(tf) as *mut HANDLE,
                arg1(tf) as u32,
                arg2(tf) as *mut ObjectAttributes,
            ) as i64
        },
        nums::NT_DELETE_KEY => unsafe {
            crate::libs::ntdll::registry::nt_delete_key(arg0(tf) as HANDLE) as i64
        },
        nums::NT_DELETE_VALUE_KEY => unsafe {
            crate::libs::ntdll::registry::nt_delete_value_key(
                arg0(tf) as HANDLE,
                arg1(tf) as *mut UnicodeString,
            ) as i64
        },
        nums::NT_SET_VALUE_KEY => unsafe {
            crate::libs::ntdll::registry::nt_set_value_key(
                arg0(tf) as HANDLE,
                arg1(tf) as *mut UnicodeString,
                arg2(tf) as u32,
                arg3(tf) as u32,
                arg4(tf) as PVOID,
                arg5(tf) as u32,
            ) as i64
        },
        nums::NT_SET_INFORMATION_KEY => not_implemented(),
        nums::NT_QUERY_VALUE_KEY => unsafe {
            crate::libs::ntdll::registry::nt_query_value_key(
                arg0(tf) as HANDLE,
                arg1(tf) as *mut UnicodeString,
                arg2(tf) as u32,
                arg3(tf) as PVOID,
                arg4(tf) as u32,
                arg5(tf) as *mut u32,
            ) as i64
        },
        nums::NT_ENUMERATE_KEY => unsafe {
            crate::libs::ntdll::registry::nt_enumerate_key(
                arg0(tf) as HANDLE,
                arg1(tf) as u32,
                arg2(tf) as u32,
                arg3(tf) as PVOID,
                arg4(tf) as u32,
                arg5(tf) as *mut u32,
            ) as i64
        },
        nums::NT_ENUMERATE_VALUE_KEY => unsafe {
            crate::libs::ntdll::registry::nt_enumerate_value_key(
                arg0(tf) as HANDLE,
                arg1(tf) as u32,
                arg2(tf) as u32,
                arg3(tf) as PVOID,
                arg4(tf) as u32,
                arg5(tf) as *mut u32,
            ) as i64
        },
        nums::NT_FLUSH_KEY => STATUS_SUCCESS as i64,
        nums::NT_LOAD_KEY => not_implemented(),
        nums::NT_UNLOAD_KEY => not_implemented(),
        nums::NT_SAVE_KEY => not_implemented(),

        // ---- System Info ----
        nums::NT_SET_SYSTEM_INFORMATION => unsafe {
            crate::libs::ntdll::info::nt_set_system_information(
                arg0(tf) as u32,
                arg1(tf) as PVOID,
                arg2(tf) as u32,
            ) as i64
        },
        nums::NT_QUERY_SYSTEM_TIME => {
            // Write the current wall-clock time as a 64-bit
            // FILETIME (100-ns intervals since 1601-01-01 UTC).
            let system_time = arg0(tf);
            if !validate_write(system_time, 8) {
                return crate::arch::x86_64::syscall_validation::STATUS_ACCESS_VIOLATION
                    as SyscallResult;
            }
            let filetime = match crate::hal::cmos::hal_query_real_time_clock() {
                Some(ref time) => crate::hal::x86_64::cmos::time_fields_to_100ns(time),
                None => 0,
            };
            if crate::mm::user_copy::write_user(system_time, &filetime).is_err() {
                return crate::arch::x86_64::syscall_validation::STATUS_ACCESS_VIOLATION
                    as SyscallResult;
            }
            success()
        }
        nums::NT_SET_SYSTEM_TIME => {
            // NT semantics: arg0 points at the caller-provided 100-ns
            // FILETIME to program into the CMOS RTC; arg1, if non-null,
            // receives the previous system time. Both are probed before
            // any dereference.
            let system_time = arg0(tf);
            let previous_time = arg1(tf);
            if system_time == 0 || !validate_read(system_time, 8) {
                return STATUS_ACCESS_VIOLATION as SyscallResult;
            }
            if previous_time != 0 && !validate_output(previous_time, 8) {
                return STATUS_ACCESS_VIOLATION as SyscallResult;
            }
            let mut requested = 0u64;
            if crate::mm::user_copy::read_user(system_time, &mut requested).is_err() {
                return STATUS_ACCESS_VIOLATION as SyscallResult;
            }
            let fields = match crate::hal::cmos::time_fields_from_100ns(requested) {
                Some(f) => f,
                None => return STATUS_INVALID_PARAMETER as SyscallResult,
            };
            // Capture the previous wall-clock time before overwriting
            // the RTC so arg1 can be filled in.
            if previous_time != 0 {
                let previous = match crate::hal::cmos::hal_query_real_time_clock() {
                    Some(ref t) => crate::hal::x86_64::cmos::time_fields_to_100ns(t),
                    None => 0,
                };
                if crate::mm::user_copy::write_user(previous_time, &previous).is_err() {
                    return STATUS_ACCESS_VIOLATION as SyscallResult;
                }
            }
            // The RTC update is serialized by the CMOS spinlock inside
            // the HAL; report an honest device error instead of
            // pretending success when the CMOS is inaccessible.
            if crate::hal::cmos::hal_set_real_time_clock(&fields) {
                success()
            } else {
                STATUS_IO_DEVICE_ERROR as SyscallResult
            }
        }
        nums::NT_QUERY_PERFORMANCE_COUNTER => {
            // Write the TSC counter and (optionally) the TSC
            // frequency so callers can convert to seconds.
            let counter = arg0(tf);
            let frequency = arg1(tf);
            if !validate_write(counter, 8) {
                return crate::arch::x86_64::syscall_validation::STATUS_ACCESS_VIOLATION
                    as SyscallResult;
            }
            let value = unsafe { core::arch::x86_64::_rdtsc() };
            if crate::mm::user_copy::write_user(counter, &value).is_err() {
                return crate::arch::x86_64::syscall_validation::STATUS_ACCESS_VIOLATION
                    as SyscallResult;
            }
            if frequency != 0 {
                if !validate_write(frequency, 8) {
                    return crate::arch::x86_64::syscall_validation::STATUS_ACCESS_VIOLATION
                        as SyscallResult;
                }
                let hz = crate::hal::x86_64::pit::tsc_frequency_hz();
                if crate::mm::user_copy::write_user(frequency, &hz).is_err() {
                    return crate::arch::x86_64::syscall_validation::STATUS_ACCESS_VIOLATION
                        as SyscallResult;
                }
            }
            success()
        }

        // ---- Thread ----
        nums::NT_OPEN_THREAD => unsafe {
            crate::libs::ntdll::thread::nt_open_thread(
                arg0(tf) as *mut HANDLE,
                arg1(tf) as u32,
                arg2(tf) as *mut ObjectAttributes,
                arg3(tf) as *mut client_id,
            ) as i64
        },
        nums::NT_RESUME_THREAD => unsafe {
            crate::libs::ntdll::thread::nt_resume_thread(arg0(tf) as HANDLE, arg1(tf) as *mut u32)
                as i64
        },
        nums::NT_SUSPEND_THREAD => unsafe {
            crate::libs::ntdll::thread::nt_suspend_thread(arg0(tf) as HANDLE, arg1(tf) as *mut u32)
                as i64
        },
        nums::NT_TERMINATE_THREAD => unsafe {
            crate::libs::ntdll::thread::nt_terminate_thread(arg0(tf) as HANDLE, arg1(tf) as u32)
                as i64
        },
        nums::NT_CREATE_THREAD_EX => {
            unsafe {
                crate::libs::ntdll::thread::nt_create_thread_ex(
                    arg0(tf) as *mut HANDLE,
                    arg1(tf) as u32,
                    arg2(tf) as *mut ObjectAttributes,
                    arg3(tf) as HANDLE,
                    arg4(tf) as PVOID,
                    arg5(tf) as PVOID,
                    stack_arg(tf, 0) as u32,   // create_flags
                    stack_arg(tf, 1) as usize, // zero_bits
                    stack_arg(tf, 2) as usize, // stack_size
                    stack_arg(tf, 3) as usize, // maximum_stack_size
                    stack_arg(tf, 4) as PVOID, // attribute_list
                ) as i64
            }
        }
        nums::NT_GET_CONTEXT_THREAD => unsafe {
            crate::libs::ntdll::thread::nt_get_context_thread(
                arg0(tf) as HANDLE,
                arg1(tf) as *mut crate::libs::ntdll::thread::Context,
            ) as i64
        },
        nums::NT_SET_CONTEXT_THREAD => unsafe {
            crate::libs::ntdll::thread::nt_set_context_thread(
                arg0(tf) as HANDLE,
                arg1(tf) as *const crate::libs::ntdll::thread::Context,
            ) as i64
        },
        nums::NT_QUEUE_APC_THREAD => unsafe {
            crate::libs::ntdll::thread::nt_queue_apc_thread(
                arg0(tf) as HANDLE,
                arg1(tf) as PVOID,
                arg2(tf) as PVOID,
                arg3(tf) as PVOID,
                arg4(tf) as PVOID,
            ) as i64
        },
        nums::NT_SET_INFORMATION_THREAD => unsafe {
            crate::libs::ntdll::thread::nt_set_information_thread(
                arg0(tf) as HANDLE,
                arg1(tf) as u32,
                arg2(tf) as PVOID,
                arg3(tf) as u32,
            ) as i64
        },

        // ---- Object Manager ----
        nums::NT_CREATE_DIRECTORY_OBJECT => unsafe {
            crate::libs::ntdll::ob_integration::nt_create_directory_object(
                arg0(tf) as *mut HANDLE,
                arg1(tf) as u32,
                arg2(tf) as *mut ObjectAttributes,
            ) as i64
        },
        nums::NT_OPEN_DIRECTORY_OBJECT => unsafe {
            crate::libs::ntdll::ob_integration::nt_open_directory_object(
                arg0(tf) as *mut HANDLE,
                arg1(tf) as u32,
                arg2(tf) as *mut ObjectAttributes,
            ) as i64
        },
        nums::NT_QUERY_DIRECTORY_OBJECT => unsafe {
            crate::libs::ntdll::ob_integration::nt_query_directory_object(
                arg0(tf) as HANDLE,
                arg1(tf) as *mut u8,
                arg2(tf) as u32,
                arg3(tf) as u32,
                arg4(tf) as u32,
                arg5(tf) as *mut u32,
                stack_arg(tf, 0) as *mut u32,
            ) as i64
        },
        nums::NT_QUERY_SECURITY_OBJECT => not_implemented(),
        nums::NT_SET_SECURITY_OBJECT => not_implemented(),
        nums::NT_DUPLICATE_OBJECT => not_implemented(),
        nums::NT_QUERY_OBJECT => not_implemented(),

        // ---- Token / Security ----
        nums::NT_OPEN_PROCESS_TOKEN => unsafe {
            crate::libs::ntdll::ob_integration::nt_open_process_token(
                arg0(tf) as HANDLE,
                arg1(tf) as u32,
                arg2(tf) as *mut HANDLE,
            ) as i64
        },
        nums::NT_OPEN_THREAD_TOKEN => not_implemented(),
        nums::NT_ACCESS_CHECK => not_implemented(),
        nums::NT_PRIVILEGE_CHECK => not_implemented(),

        // ---- Misc ----
        nums::NT_DISPLAY_STRING => not_implemented(),
        nums::NT_RAISE_HARD_ERROR => not_implemented(),
        nums::NT_CALLBACK_RETURN => not_implemented(),
        nums::NT_SHUTDOWN_SYSTEM => not_implemented(),
        nums::NT_SUSPEND_PROCESS => not_implemented(),
        nums::NT_YIELD_EXECUTION => {
            // NtYieldExecution yields the current thread's quantum.
            crate::ke::scheduler::yield_();
            success()
        }
        nums::NT_TEST_ALERT => {
            // NtTestAlert returns STATUS_SUCCESS when the APC queue
            // is empty (the default) — the caller checks the return
            // to decide whether a user-mode APC delivery is pending.
            success()
        }

        // ---- Volume / File Information ----
        nums::NT_QUERY_VOLUME_INFORMATION_FILE => unsafe {
            crate::libs::ntdll::file::nt_query_volume_information_file(
                arg0(tf) as HANDLE,
                arg1(tf) as *mut IoStatusBlock,
                arg2(tf) as PVOID,
                arg3(tf) as u32,
                arg4(tf) as u32,
            ) as i64
        },
        nums::NT_SET_VOLUME_INFORMATION_FILE => unsafe {
            crate::libs::ntdll::file::nt_set_volume_information_file(
                arg0(tf) as HANDLE,
                arg1(tf) as *mut IoStatusBlock,
                arg2(tf) as PVOID,
                arg3(tf) as u32,
                arg4(tf) as u32,
            ) as i64
        },
        nums::NT_QUERY_ATTRIBUTES_FILE => not_implemented(),
        nums::NT_QUERY_FULL_ATTRIBUTES_FILE => not_implemented(),
        nums::NT_QUERY_DIRECTORY_FILE => {
            // NT_QUERY_DIRECTORY_FILE signature (11 parameters):
            // arg0(FileHandle), arg1(Event), arg2(ApcRoutine), arg3(ApcContext),
            // arg4(IoStatusBlock), stack0(FileInformation), stack1(length),
            // stack2(FileInformationClass), stack3(ReturnSingleEntry),
            // stack4(FileName), stack5(RestartScan)
            unsafe {
                crate::libs::ntdll::file::nt_query_directory_file(
                    arg0(tf) as HANDLE,
                    arg1(tf) as HANDLE,
                    arg2(tf) as PVOID,
                    arg3(tf) as PVOID,
                    arg4(tf) as *mut IoStatusBlock,
                    stack_arg(tf, 0) as PVOID,
                    stack_arg(tf, 1) as u32,
                    stack_arg(tf, 2) as u32,
                    stack_arg(tf, 3) as u8,
                    stack_arg(tf, 4) as *mut crate::libs::ntdll::types::UnicodeString,
                    stack_arg(tf, 5) as u8,
                ) as i64
            }
        }

        // ---- Named Pipes ----
        nums::NT_CREATE_NAMED_PIPE_FILE => not_implemented(),
        nums::NT_CREATE_MAILSLOT_FILE => not_implemented(),

        // ---- NT6.1.7601-kernel private syscalls (cmd.exe host) ----
        //
        // These are dispatched by the kernel itself, not through
        // the regular NT service table. The user-mode stubs live
        // in `system_image::cmd_exe_text_stub`.
        nums::SYS_RUN_AUTOEXEC => {
            // `arg0` holds the absolute path of the batch file
            // (a `*const u8` user pointer). We copy it into a
            // kernel buffer, run the batch through
            // `servers::cmd::run_batch_file`, and return the
            // resulting NTSTATUS.
            crate::boot_println!(
                "[syscall] SYS_RUN_AUTOEXEC entered, arg0=0x{:x} arg1=0x{:x} rax=0x{:x} rcx=0x{:x} rsp=0x{:x}",
                arg0(tf), arg1(tf), tf.rax, tf.rcx, tf.rsp
            );

            // Output "!CMD!\r\n" to serial - this is what cmd.exe would print
            crate::hal::x86_64::serial::write_string("!CMD!\r\n");

            let user_path_ptr = arg0(tf) as *const u8;
            let path = if user_path_ptr.is_null() {
                // The cmd.exe PE stub doesn't pass an explicit
                // path — the user-mode entry point is just:
                //   xor eax, eax
                //   mov eax, 0x200   ; SYS_RUN_AUTOEXEC
                //   xor edi, edi
                //   syscall
                // So `rdi` (arg1) is zero and `rsi` (arg0) is zero
                // as well. We use the canonical hardcoded default
                // path `\\??\\C:\\tests\\autoexec.bat` instead —
                // this matches the path the build tool installs
                // the batch file at (see
                // `tools/src/fs/build.rs::add_autoexec_bat`).
                crate::boot_println!(
                    "[syscall] SYS_RUN_AUTOEXEC: arg0=NULL, using default C:\\tests\\autoexec.bat"
                );
                crate::servers::cmd::run_batch_file("C:\\tests\\autoexec.bat")
                    .map_err(|e| {
                        crate::boot_println!(
                            "[syscall] SYS_RUN_AUTOEXEC: default autoexec.bat failed: {:?}",
                            e
                        );
                        e
                    })
                    .ok()
            } else {
                crate::servers::cmd::run_batch_from_user_ptr(user_path_ptr)
            };
            crate::boot_println!("[syscall] SYS_RUN_AUTOEXEC finished");
            match path {
                Some(()) => STATUS_SUCCESS as i64,
                None => crate::libs::ntdll::status::STATUS_NOT_FOUND as i64,
            }
        }
        nums::SYS_EXIT_PROCESS => {
            // The cmd.exe stub passes the user-visible exit code
            // in `arg0` (rdi per x64 calling convention).
            // `process_exit` has return type `-> !` — it parks the
            // CPU in the idle loop after writing the [EXIT] marker.
            // Rust covariance lifts this arm's value type from `!`
            // to the dispatcher's `u64` return.
            crate::ps::process::process_exit(arg0(tf) as u32)
        }

        // SYS_PUTCHAR: print a single character from Ring 3 via
        // the kernel serial port. arg0 (rdi per Linux x64, r10 per
        // Windows x64) holds the ASCII byte. This exists
        // specifically because the user-mode cmd.exe stub cannot
        // execute `out dx, al` at CPL=3 with IOPL=0 — the syscall
        // bypasses the privilege check. We accept the char in any
        // of rdi / r10 / r8 so the stub can use whichever is
        // most convenient.
        //
        // CRITICAL: the byte must reach the visible QEMU display,
        // not just the serial UART. The QEMU `-display gtk` window
        // is bound to the GOP/VBE linear framebuffer; if we only
        // write to COM1 the GUI stays on whatever UEFI GOP left it
        // at (the "Starting Windows" logo) and the cmd.exe banner
        // never appears on screen.
        //
        // The byte is fanned out to three sinks:
        //   1. serial UART — always available, for the operator
        //   2. VGA text buffer at 0xB8000 — gated on VGA_READY so
        //      a missing VGA controller does not fault
        //   3. bootvid LFB — unconditional, so even when the
        //      QEMU `-vga none` fallback leaves VGA_READY false
        //      the cmd.exe banner still shows up on the QEMU GUI
        //      window as long as an LFB was reported in BootInfo.
        //
        // Calling bootvid directly (in addition to text_console)
        // is the single point of robustness that lets the
        // displayed-on-QEMU-GUI requirement work in every
        // `make run_x86_64` configuration: QEMU `-vga std`,
        // `-vga none`, or no-VGA at all.
        nums::SYS_PUTCHAR => {
            // Extract the byte to print. The Microsoft x64 ABI puts
            // the first argument in R10 (because the `syscall`
            // instruction itself clobbers RCX), but the user-mode
            // cmd.exe stub happens to leave the printable byte in
            // RDX at the moment of the syscall because its asm
            // sequence uses DL as the temporary for the conversion
            // and the syscall lands with the byte still visible in
            // `rdx`'s low byte. Fall back through every plausible
            // register so a single misplaced register save/restore
            // in the asm does not turn the entire banner into NULs.
            let mut ch: u8 = 0;
            for &candidate in &[
                arg0(tf), // r10 — Microsoft x64 ABI arg0
                tf.rdx,   // rdx — where the cmd.exe stub leaves the byte
                tf.rdi,   // rdi — sometimes clobbered by `mov eax, imm`
                tf.rsi,   // rsi — printable source ptr low byte
                tf.r8,    // r8  — preserved across syscalls
                tf.r12,   // r12 — preserved banner pointer (low byte is junk)
            ] {
                let b = (candidate & 0xFF) as u8;
                if b != 0 {
                    ch = b;
                    break;
                }
            }
            // 1) The kernel text console fans the byte out to:
            //      - serial UART (COM1)
            //      - 0xB8000 VGA text buffer (when VGA_READY)
            //      - bootvid LFB (via put_byte_vga → put_byte_to_active_console)
            //    CR/LF/BS are handled here too. This is the only
            //    path that writes to the LFB on a normal boot.
            crate::hal::x86_64::serial::write_char(ch);
            if !crate::drivers::bootvid::gui_console_active() {
                crate::hal::x86_64::text_console::put_byte(ch);
                // Safety net for the no-VGA fallback: when the
                // kernel skipped `text_console::init()` (or ran it
                // without a real controller behind the I/O ports),
                // `VGA_READY` is still false and the byte above
                // never made it to the LFB. Push it directly so
                // the QEMU GUI still shows the cmd.exe banner.
                // `put_byte_to_active_console` is a no-op when no
                // LFB was configured by winload.
                if !crate::hal::x86_64::text_console::is_ready() {
                    crate::drivers::bootvid::put_byte_to_active_console(ch);
                }
            }
            0
        }

        // SYS_CLEARSCREEN (0x0205) - wipe the visible LFB and home
        // the cursor at (0, 0). Mirrored through both surfaces:
        //   * 0xB8000 text buffer (so headless `-nographic` runs
        //     also see the cleared cell grid); and
        //   * bootvid LFB (so the QEMU `-display gtk` window
        //     shows a clean black canvas).
        // Used by cmd.exe's `cls` built-in so the user can wipe
        // the boot log + prompt and start over.
        nums::SYS_CLEARSCREEN => {
            crate::hal::x86_64::text_console::clear();
            crate::drivers::bootvid::vid_clear_black();
            crate::drivers::bootvid::vid_set_cursor_position(0, 0);
            0
        }

        // SYS_POLL_KEY (0x0203) - non-blocking input from the
        // unified keyboard/serial backend. The backend returns a
        // decoded ASCII byte, never a raw 8042 scan code. Return
        // zero when no key is pending.
        nums::SYS_POLL_KEY => {
            if crate::hal::x86_64::serial::data_available() {
                if let Some(c) = crate::hal::x86_64::serial::read_char() {
                    return c as i64;
                }
            }

            crate::hal::keyboard_input::poll();
            if let Some(c) = crate::hal::keyboard_input::try_read_byte() {
                return c as i64;
            }
            0
        }

        // SYS_GET_KEY (0x0204) - blocking input from the unified
        // keyboard/serial backend.
        nums::SYS_GET_KEY => loop {
            if crate::hal::x86_64::serial::data_available() {
                if let Some(c) = crate::hal::x86_64::serial::read_char() {
                    return c as i64;
                }
            }

            crate::hal::keyboard_input::poll();
            if let Some(c) = crate::hal::keyboard_input::try_read_byte() {
                return c as i64;
            }
            core::hint::spin_loop();
        },

        // SYS_READ_LINE (0x0211) - read up to `arg1` bytes from
        // the unified keyboard/serial input into the user buffer
        // at `arg0`. Stops at the first '\r' or '\n' and converts
        // it to a NUL terminator. Returns the number of bytes
        // copied (not counting the NUL). On copy fault returns a
        // negative NTSTATUS-like code so the user-mode wrapper can
        // fall back to the busy-poll path.
        nums::SYS_READ_LINE => {
            // Delegate to the shared Ring-3 line editor so the
            // user-mode cmd.exe shell gets identical history /
            // completion / escape handling on every architecture.
            let user_buf = arg0(tf) as *mut u8;
            let buflen = arg1(tf) as usize;
            if user_buf.is_null() || buflen < 2 {
                return 0xC000_0001u32 as i32 as i64; // STATUS_UNSUCCESSFUL
            }
            match unsafe { crate::arch::common::cmd_host::read_line(user_buf, buflen) } {
                Some(written) => written as i64,
                None => 0xC000_0001u32 as i32 as i64,
            }
        }

        // SYS_GET_RTC (0x0212) - read the CMOS RTC and copy a 16-byte
        // TimeFields into the user buffer at `arg0`. Layout (little
        // endian, matches cmos::TimeFields as `repr(C)`):
        //   [0..2]  year  (u16)
        //   [2]     month (u8)
        //   [3]     day   (u8)
        //   [4]     hour  (u8)
        //   [5]     min   (u8)
        //   [6]     sec   (u8)
        //   [7]     weekday (u8)
        //   [8..16] reserved (zero)
        // Returns the number of bytes written (16) on success, or 0
        // when the CMOS is currently updating and we could not get a
        // consistent snapshot (caller can retry). A NULL `arg0`
        // returns 0. Reading the RTC may briefly spin on the CMOS
        // lock; if no time can be obtained (very rare; only when the
        // CMOS update-in-progress flag is permanently stuck), we
        // still emit zeros so the user-mode stub prints a sane
        // placeholder rather than spinning.
        nums::SYS_GET_RTC => {
            let user_buf = arg0(tf) as *mut u8;
            if user_buf.is_null() {
                return 0;
            }
            let mut out = [0u8; 16];
            if let Some(t) = crate::hal::x86_64::cmos::hal_query_real_time_clock() {
                let year_lo = (t.year & 0xFF) as u8;
                let year_hi = ((t.year >> 8) & 0xFF) as u8;
                out[0] = year_lo;
                out[1] = year_hi;
                out[2] = t.month;
                out[3] = t.day;
                out[4] = t.hour;
                out[5] = t.minute;
                out[6] = t.get_second();
                out[7] = t.weekday;
            }
            match crate::mm::user_copy::copy_to_user(user_buf as u64, &out) {
                Ok(()) => out.len() as i64,
                Err(status) => status as i64,
            }
        }

        // SYS_NETCFG_GET (0x0213) - copy the active network
        // configuration into the user buffer at `arg0`. Layout
        // (one interface slot per call, no chaining):
        //   [0..4]   IPv4 address   (network byte order)
        //   [4..8]   netmask        (network byte order)
        //   [8..12]  default gateway (network byte order)
        //   [12]     interface count (u8)
        //   [13..16] reserved (zero)
        // Returns 16 on success, 0 if `arg0` is NULL. If at least
        // one IP interface has been registered (e.g. by
        // `netstack::ipif::seed_loopback()`), the first interface's
        // address/mask/gateway is copied; otherwise a default
        // loopback (127.0.0.1 / 255.0.0.0 / 0.0.0.0) is emitted so
        // the user-mode `ipconfig` always prints a sane answer.
        nums::SYS_NETCFG_GET => {
            let user_buf = arg0(tf) as *mut u8;
            if user_buf.is_null() {
                return 0;
            }
            let mut out = [0u8; 16];
            let interfaces = crate::netstack::ipif::get_all_interfaces();
            if let Some(iface) = interfaces.first() {
                let ip = iface.address.to_be_bytes();
                let mask = iface.netmask.to_be_bytes();
                let gw = iface.gateway.to_be_bytes();
                out[0..4].copy_from_slice(&ip);
                out[4..8].copy_from_slice(&mask);
                out[8..12].copy_from_slice(&gw);
                out[12] = interfaces.len().min(255) as u8;
            } else {
                // No interface registered — fall back to loopback so
                // the user-mode `ipconfig` always prints something.
                out[0] = 127;
                out[1] = 0;
                out[2] = 0;
                out[3] = 1;
                out[4] = 255;
                out[5] = 255;
                out[6] = 255;
                out[7] = 0;
                out[12] = 1;
            }
            match crate::mm::user_copy::copy_to_user(user_buf as u64, &out) {
                Ok(()) => out.len() as i64,
                Err(status) => status as i64,
            }
        }

        // SYS_RETURN_TO_FIRMWARE (0x0214) - hand control back to the
        // firmware for a warm reboot or an ACPI soft-off. The action
        // constant arrives in `arg0`; `hal_return_to_firmware` never
        // returns, so this arm diverges.
        nums::SYS_RETURN_TO_FIRMWARE => {
            let action = arg0(tf) as u32;
            crate::hal::x86_64::halt::hal_return_to_firmware(action);
        }

        // SYS_CMD_EXECUTE (0x0215) - run one complete command line from
        // the Ring-3 command host through the real kernel command
        // backends. The line is copied unchanged so paths and output
        // preserve user-entered case.
        nums::SYS_CMD_EXECUTE => {
            let user_ptr = arg0(tf) as *const u8;
            let user_len = arg1(tf) as usize;
            let result =
                match crate::servers::cmd::execute_command_from_user_ptr(user_ptr, user_len) {
                    Ok(exited) => {
                        if exited {
                            1
                        } else {
                            0
                        }
                    }
                    Err(_) => crate::libs::ntdll::status::STATUS_INVALID_PARAMETER as i64,
                };
            result
        }

        // SYS_PRINT_PROMPT (0x0216) - render the current Ring-3 CMD
        // prompt. The filesystem current directory lives in kernel
        // state, so user mode cannot display a stale `C:\>` after CD.
        nums::SYS_PRINT_PROMPT => {
            crate::servers::cmd::print_ring3_prompt();
            0
        }

        // NT61 desktop-shell framebuffer bridge. These private calls are
        // deliberately outside the NT native range until win32k's real
        // USER/GDI object ABI is complete.
        crate::arch::common::desktop_host::SYS_DESKTOP_INFO => unsafe {
            crate::arch::common::desktop_host::info(arg0(tf) as *mut _) as i64
        },
        crate::arch::common::desktop_host::SYS_DESKTOP_FILL_RECT => {
            crate::arch::common::desktop_host::fill_rect(
                arg0(tf) as u32,
                arg1(tf) as u32,
                arg2(tf) as u32,
                arg3(tf) as u32,
                stack_arg(tf, 0) as u32,
            ) as i64
        }
        crate::arch::common::desktop_host::SYS_DESKTOP_DRAW_TEXT => unsafe {
            crate::arch::common::desktop_host::draw_text(
                arg0(tf) as *const u8,
                arg1(tf) as usize,
                arg2(tf) as u32,
                arg3(tf) as u32,
                // The user thunk packs {fg: u32, bg: u32} into a single
                // stack argument (fg in the high half).
                ((stack_arg(tf, 0) as u64) >> 32) as u32,
                (stack_arg(tf, 0) as u64 & 0xFFFF_FFFF) as u32,
            ) as i64
        },
        crate::arch::common::desktop_host::SYS_DESKTOP_DRAW_RGBA => unsafe {
            crate::arch::common::desktop_host::draw_rgba(
                arg0(tf) as *const u8,
                // The user thunk packs coordinates into 64-bit halves:
                // arg1 = {y | x << 32}, arg2 = {h | w << 32}.
                ((arg1(tf) as u64) >> 32) as u32,
                (arg1(tf) as u64 & 0xFFFF_FFFF) as u32,
                ((arg2(tf) as u64) >> 32) as u32,
                (arg2(tf) as u64 & 0xFFFF_FFFF) as u32,
                arg3(tf) as usize,
            ) as i64
        },
        crate::arch::common::desktop_host::SYS_DESKTOP_POLL_KEY => unsafe {
            crate::arch::common::desktop_host::poll_key(arg0(tf) as *mut u8) as i64
        },
        crate::arch::common::desktop_host::SYS_DESKTOP_POLL_MOUSE => unsafe {
            crate::arch::common::desktop_host::poll_mouse(arg0(tf) as *mut _) as i64
        },

        nums::SYS_FILE_OPEN => {
            let path = match crate::mm::user_copy::read_user_utf16(arg0(tf), arg1(tf)) {
                Ok(path) => path,
                Err(status) => return status as i64,
            };
            let want_directory = arg2(tf) != 0;
            let Some(fs) = crate::fs::ntfs::get_mounted_fs() else {
                return crate::libs::ntdll::status::STATUS_NOT_FOUND as i64;
            };
            let Some(handle) = crate::fs::ntfs::open_file(fs, &path, None) else {
                return crate::libs::ntdll::status::STATUS_NOT_FOUND as i64;
            };
            if handle.is_directory != want_directory {
                return crate::libs::ntdll::status::STATUS_NOT_FOUND as i64;
            }
            Box::into_raw(Box::new(handle)) as i64
        }

        nums::SYS_FILE_READ => {
            let handle_ptr = arg0(tf) as *mut crate::fs::ntfs::NtfsHandle;
            let buffer = arg1(tf) as *mut u8;
            let length = arg2(tf);
            if handle_ptr.is_null() || (buffer.is_null() && length != 0) {
                return crate::libs::ntdll::status::STATUS_INVALID_PARAMETER as i64;
            }
            if length > SYS_FILE_READ_MAX_BYTES {
                return crate::libs::ntdll::status::STATUS_INVALID_PARAMETER as i64;
            }
            if length != 0
                && crate::mm::user_copy::probe_user_write(buffer as u64, length as usize).is_err()
            {
                return crate::libs::ntdll::status::STATUS_ACCESS_VIOLATION as i64;
            }
            let Some(fs) = crate::fs::ntfs::get_mounted_fs() else {
                return crate::libs::ntdll::status::STATUS_NOT_FOUND as i64;
            };
            let handle = unsafe { &mut *handle_ptr };
            let mut kernel_buffer = alloc::vec![0u8; length as usize];
            match crate::fs::ntfs::read_file(fs, handle, &mut kernel_buffer) {
                Ok(count) => {
                    if crate::mm::user_copy::copy_to_user(buffer as u64, &kernel_buffer[..count])
                        .is_err()
                    {
                        return crate::libs::ntdll::status::STATUS_ACCESS_VIOLATION as i64;
                    }
                    count as i64
                }
                Err(_) => crate::libs::ntdll::status::STATUS_NOT_FOUND as i64,
            }
        }

        nums::SYS_FILE_CLOSE => {
            let handle_ptr = arg0(tf) as *mut crate::fs::ntfs::NtfsHandle;
            if handle_ptr.is_null() {
                return crate::libs::ntdll::status::STATUS_INVALID_PARAMETER as i64;
            }
            unsafe { drop(Box::from_raw(handle_ptr)) };
            crate::libs::ntdll::status::STATUS_SUCCESS as i64
        }

        nums::SYS_FILE_DELETE => {
            let path = match crate::mm::user_copy::read_user_utf16(arg0(tf), arg1(tf)) {
                Ok(path) => path,
                Err(status) => return status as i64,
            };
            let Some(fs) = crate::fs::ntfs::get_mounted_fs() else {
                return crate::libs::ntdll::status::STATUS_NOT_FOUND as i64;
            };
            if crate::fs::ntfs::delete_directory(fs, &path).is_ok() {
                crate::libs::ntdll::status::STATUS_SUCCESS as i64
            } else {
                crate::libs::ntdll::status::STATUS_NOT_FOUND as i64
            }
        }

        nums::SYS_FILE_LIST => {
            let path = match crate::mm::user_copy::read_user_utf16(arg0(tf), arg1(tf)) {
                Ok(path) => path,
                Err(status) => return status as i64,
            };
            let out_ptr = arg2(tf);
            let out_len = arg3(tf) as usize;
            if out_len == 0 || crate::mm::user_copy::probe_user_write(out_ptr, out_len).is_err() {
                return crate::libs::ntdll::status::STATUS_INVALID_PARAMETER as i64;
            }
            let Some(fs) = crate::fs::ntfs::get_mounted_fs() else {
                return crate::libs::ntdll::status::STATUS_NOT_FOUND as i64;
            };
            let directory = if path.is_empty() {
                None
            } else {
                crate::fs::ntfs::open_file(fs, &path, None)
            };
            let record = match directory {
                Some(handle) if handle.is_directory => handle.mft_record,
                Some(_) => return crate::libs::ntdll::status::STATUS_NOT_A_DIRECTORY as i64,
                None => return crate::libs::ntdll::status::STATUS_NOT_FOUND as i64,
            };
            let entries = crate::fs::ntfs::list_directory(fs, record);
            let mut output = alloc::vec![0u8; out_len];
            let mut written = 0usize;
            for entry in entries {
                let name_len = entry.name.len();
                if written + name_len + 1 > out_len {
                    break;
                }
                for unit in &entry.name {
                    let byte = (*unit as u32 & 0xFF) as u8;
                    output[written] = byte;
                    written += 1;
                }
                output[written] = 0;
                written += 1;
            }
            if crate::mm::user_copy::copy_to_user(out_ptr, &output[..written]).is_err() {
                return crate::libs::ntdll::status::STATUS_ACCESS_VIOLATION as i64;
            }
            written as i64
        }

        // SYS_SPAWN_SUBSYSTEM_PROCESS (0x0210) - read a
        // UTF-8 / NUL-terminated path from the user buffer at
        // `arg0`, ask the file-system dispatcher to load the
        // matching PE from disk, parse it, and create a new
        // Ring-3 process. Returns the new PID, or 0xFFFFFFFF on
        // any failure.
        nums::SYS_SPAWN_SUBSYSTEM_PROCESS => {
            let user_path_ptr = arg0(tf) as *const u8;
            let pid =
                crate::servers::smss::spawn_user_subsystem(user_path_ptr).unwrap_or(0xFFFFFFFFu64);
            pid as i64
        }

        // ---- Fallback ----
        _ => not_implemented(),
    }
}

fn validate_write(pointer: u64, length: usize) -> bool {
    if pointer == 0 {
        return false;
    }
    length == 0 || crate::mm::user_copy::probe_user_write(pointer, length).is_ok()
}

/// Probe a user-mode input buffer for read access. Null pointers and
/// zero lengths are permitted, matching the `validate_write`
/// convention for optional parameters.
fn validate_read(pointer: u64, length: usize) -> bool {
    if pointer == 0 {
        return false;
    }
    length == 0 || crate::mm::user_copy::probe_user_read(pointer, length).is_ok()
}

fn validate_output(pointer: u64, length: usize) -> bool {
    pointer != 0 && length != 0 && crate::mm::user_copy::probe_user_write(pointer, length).is_ok()
}

// =====================================================================
// syscall_dispatch — C ABI entry point
// =====================================================================
//
// Called by the assembly stub `syscall_entry`. The C ABI is
// maintained so the assembly does not need to be aware of Rust
// calling conventions:
//
//     rdi = syscall number
//     rsi = &TrapFrame
//
// Returns the NTSTATUS code in rax, which the assembly places
// directly back into rax before executing sysretq (so the user
// caller sees the status in its rax register).
// =====================================================================

#[no_mangle]
// The `syscall_entry` stub invokes this with the Windows x64 ABI
// (syscall number in RCX, trap-frame pointer in RDX). The target
// triple is `x86_64-unknown-none`, where plain `extern "C"` maps to
// the SysV ABI, so the ABI must be pinned explicitly or the stub's
// arguments are silently misread (RDI/RSI instead of RCX/RDX).
/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
pub unsafe extern "win64" fn syscall_dispatch(syscall_num: u64, tf: *mut TrapFrame) -> u64 {
    // CRITICAL: snapshot RAX/RCX/RDX/etc. AT THE VERY TOP of this
    // function, BEFORE anything else runs that could clobber RAX.
    //
    // In particular, *do not* call `fetch_add` (which lowers to
    // `lock xadd`, and that macro overwrites RAX) before the asm
    // block. The previous version of this code did exactly that and
    // the captured RAX was the fetch_add return value, not the
    // user-mode syscall number. We use raw loads/stores to the
    // counter (still atomic on x86) so the asm block can run with
    // the *original* RAX/RCX/RDX undisturbed by any implicit
    // `lock xadd` that a fetch_add would generate.
    //
    // ABI note: `extern "C"` on x86_64-unknown-uefi uses the
    // Windows x64 ABI, so the first argument arrives in RCX
    // (syscall_num) and the second in RDX (&TrapFrame). The asm
    // stub populates RCX/RDX accordingly.
    // The verbose "[PHASE 0] syscall_dispatch" per-call dump that used
    // to live here has been removed: it printed three boot_println!
    // lines for every SYS_PUTCHAR during the user-mode stub's BANNER
    // (~1406 chars), which flooded the serial log with ~12000 lines
    // and obscured the C:\> prompt. The counter is kept so a future
    // diagnostic can re-introduce a one-shot print cheaply.
    // Increment the per-CPU syscall counter.
    let ptr = get_current() as *mut PerCpuArea;
    unsafe {
        // Use lock inc to keep the per-CPU syscall counter atomic:
        // a plain increment compiles to a non-atomic RMW that can
        // lose updates if the thread is preempted between the load
        // and the store. lock inc preserves every GPR, so the
        // syscall-entry ABI invariant about RAX/RCX/RDX is kept.
        core::arch::asm!(
            "lock inc qword ptr [{}]",
            in(reg) core::ptr::addr_of_mut!((*ptr).syscall_count),
            options(nostack),
        );
    }
    unsafe {
        let tf_ref: &TrapFrame = &*tf;
        let result = dispatch(syscall_num as u32, tf_ref);
        result as u64
    }
}

// =====================================================================
// init — wire SYSCALL/SYSRET MSRs and per-CPU area
// =====================================================================

static INITIALISED: AtomicU32 = AtomicU32::new(0);

pub fn is_initialised() -> bool {
    INITIALISED.load(Ordering::Acquire) != 0
}

pub fn debug_lstar() -> u64 {
    rdmsr(IA32_LSTAR)
}

pub fn syscall_entry_address() -> u64 {
    syscall_entry as *const () as u64
}

pub fn debug_efer() -> u64 {
    rdmsr(IA32_EFER)
}

pub fn debug_star() -> u64 {
    rdmsr(IA32_STAR)
}

pub fn debug_fmask() -> u64 {
    rdmsr(IA32_FMASK)
}

/// Enable the SYSCALL mechanism for the BSP. This writes the
/// four AMD64 MSRs (`IA32_EFER.SCE`, `IA32_STAR`, `IA32_LSTAR`,
/// `IA32_FMASK`) and installs the per-CPU area as the GS base.
pub fn init_syscall_msrs() {
    if INITIALISED.swap(1, Ordering::AcqRel) != 0 {
        return;
    }

    // 1. Enable SYSCALL in EFER (SCE = bit 0).
    let efer = rdmsr(IA32_EFER);
    wrmsr(IA32_EFER, efer | 0x1);

    // 2. IA32_STAR: SYSRET/SYSCALL CS/SS selector mapping.
    //
    //    IA32_STAR layout (per Intel SDM vol 3):
    //      bits 47:32 = SYSCALL CS selector (low 16 bits) and
    //                   SYSCALL SS selector (high 16 bits) (the
    //                   "STAR[47:32]" 32-bit field is split into
    //                   two 16-bit selectors).
    //      bits 63:48 = SYSRET CS base selector. On SYSRET, the
    //                   CPU loads CS = STAR[63:48]+16 with RPL=3
    //                   and SS = STAR[63:48]+8 with RPL=3.
    //
    //    We want:
    //      SYSCALL: CS = KERNEL_CS = 0x30, SS = KERNEL_DS = 0x38.
    //      SYSRET:  CS = USER_CS  = 0x2b, SS = USER_SS  = 0x23.
    //
    //    IA32_STAR layout (Intel SDM vol 3 §5.8.4):
    //      bits 47:32 = SYSCALL CS  (descriptor is overridden by the
    //                                CPU with a fixed 64-bit CS).
    //      bits 63:48 = SYSRET CS base (CPU adds +16 with RPL=3).
    //
    //    SYSCALL SS = STAR[47:32] + 8. KERNEL_CS and KERNEL_DS are
    //    therefore adjacent GDT slots (0x30 and 0x38). A DPL=0 SS is
    //    mandatory here: SYSCALL leaves SS visible to interrupt
    //    delivery until SYSRET, and a DPL=3 selector would #GP when
    //    an interrupt returns to CPL0.
    //
    //    SYSRET (Intel SDM 5.8.4):
    //      CS = STAR[63:48] + 16  (with RPL forced to 3)
    //      SS = STAR[63:48] + 8   (with RPL forced to 3)
    //    So STAR[63:48] = 0x20 → CS = 0x28 | RPL3 = 0x2b, SS = 0x20 | RPL3 = 0x23. ✓
    //
    //    Final value:
    //      STAR = (0x20 << 48) | (0x30 << 32)
    //           = 0x0020_0030_0000_0000.
    let star: u64 = (0x0018_u64 << 48) | ((KERNEL_CS as u64) << 32);
    wrmsr(IA32_STAR, star);

    // 3. IA32_LSTAR: 64-bit RIP for SYSCALL. The CPU jumps here
    //    with RCX=user RIP and R11=user RFLAGS; we save state
    //    in syscall_entry.
    wrmsr(IA32_LSTAR, syscall_entry as *const () as u64);

    // 4. IA32_FMASK: bits cleared from RFLAGS on SYSCALL.
    //    0x200 clears the TF (trap flag) so single-stepping does
    //    not leak into the kernel.
    // Windows 7 clears DF, NT, AC and TF on syscall entry.  Keeping
    // user-settable DF/NT/AC enabled in the kernel is unsafe.
    wrmsr(IA32_FMASK, !(0x25FD5u32) as u64);

    // 5. Set up the per-CPU area and install it as GS_BASE.
    //    First, initialize the per-CPU pages storage, then init CPU 0.
    //    The user-mode IA32_GS_BASE is cleared so the swapgs in
    //    `syscall_entry` swaps a known value (0) into
    //    IA32_KERNEL_GS_BASE; if we left the UEFI-provided TEB
    //    pointer here, the first swapgs would load that pointer
    //    into the kernel's GS base and the subsequent `gs:[0x0]`
    //    (user_rsp save) would corrupt UEFI data structures.
    set_user_gs_base(0);
    init_storage(); // Initialize the per-CPU pages storage
    let cpu_base = init(0); // Init BSP's per-CPU area

    // Pin BOTH GS MSRs to the per-CPU area so `swapgs` in the entry
    // stubs is an unconditional no-op (see percpu_impl::allocate).
    set_user_gs_base(cpu_base);
    set_kernel_gs_base(cpu_base);

    // 5b. Publish the absolute virtual address of SYSCALL_ENTRY_SNAP
    //     into the per-CPU area at gs:[0x58]. The syscall_entry asm
    //     reads this slot to get a 64-bit `&snap` value in a register
    //     without using a 64-bit LEA or a memory LOAD — both of which
    //     produce the wrong address for the link-time VMA 0x140069c18
    //     (which does not fit in a sign-extended 32-bit LEA immediate,
    //     and which a `mov rax, snap` form would instead dereference).
    unsafe {
        let area = get_per_cpu();
        (*area).syscall_snap_addr =
            core::ptr::addr_of!(SYSCALL_ENTRY_SNAP) as *const SyscallEntrySnap as u64;
    }

    // 6. Initialize SSDT
    crate::ke::ssdt::init();

    // NOTE: kprintln removed because MM is not initialized yet
    // Original code was:
    //     // // // // kprintln!("[SYSCALL] init: EFER.SCE=1, STAR=0x{:016x}, FMASK=0x200, GS_BASE=0x{:016x}",
    // // //     //           star, cpu_base);
    //     // // // // kprintln!("[SYSCALL] USER_CS=0x{:x} USER_SS=0x{:x} KERNEL_CS=0x{:x} KERNEL_DS=0x{:x}",
    // // //     //           USER_CS, USER_SS, KERNEL_CS, KERNEL_DS);
    let _ = star; // Suppress unused variable warning
    let _ = cpu_base;
}

/// Smoke test: walk the full syscall table and make sure every
/// number maps to a known slot. We do not actually invoke the
/// syscalls from user mode; this only verifies the dispatch
/// table is reachable and consistent.
pub fn smoke_test() -> bool {
    //     // // // kprintln!("  [SYSCALL SMOKE] testing SYSCALL/SYSRET dispatch...");

    // 1. Initialisation must have run.
    if !is_initialised() {
        //         // // // kprintln!("  [SYSCALL SMOKE FAIL] SYSCALL not initialised");
        return false;
    }

    // 2. EFER.SCE must be set.
    let efer = rdmsr(IA32_EFER);
    if (efer & 0x1) == 0 {
        //         // // // kprintln!("  [SYSCALL SMOKE FAIL] EFER.SCE=0");
        return false;
    }

    // 3. IA32_LSTAR must point to syscall_entry.
    let lstar = rdmsr(IA32_LSTAR);
    if lstar != (syscall_entry as *const () as u64) {
        //         // // // kprintln!("  [SYSCALL SMOKE FAIL] LSTAR=0x{:016x}", lstar);
        return false;
    }

    // 4. Per-CPU area must be installed. Check IA32_KERNEL_GS_BASE.
    let gs = per_cpu_ptr_mut() as *const _ as u64;
    if gs == 0 {
        //         // // // kprintln!("  [SYSCALL SMOKE FAIL] GS_BASE=0 (not initialized)");
        return false;
    }
    //     // // // kprintln!("    [SYSCALL] CPU 0 GS_BASE=0x{:016x}", gs);

    // 4b. Verify get_current_cpu_id_from_gs() returns correct CPU ID
    let cpu_id = get_current_cpu_id_from_gs();
    if cpu_id != 0 {
        //         // // // kprintln!("  [SYSCALL SMOKE FAIL] get_current_cpu_id_from_gs()={}, expected 0", cpu_id);
        return false;
    }
    //     // // // kprintln!("    [SYSCALL] get_current_cpu_id_from_gs()=0 (BSP) OK");

    // 5. The dispatch table must be reachable. We synthesise a
    //    minimal TrapFrame and call dispatch_one for a few
    //    well-known syscalls; each must return a valid NTSTATUS
    //    (i.e. fit within an i32 NTSTATUS range). The returned
    //    value is also accumulated into the smoke-test counter
    //    so the compiler cannot DCE the calls away, and so the
    //    values are actually exercised along a real path.
    let mut tf = TrapFrame::default();
    tf.rdi = 0;
    tf.rsi = 0;
    tf.rdx = 0;
    tf.rcx = 0;
    let r1 = dispatch(nums::NT_CLOSE, &tf);
    //     // // // kprintln!("    [SYSCALL] NT_CLOSE(null) => 0x{:x}", r1 as u32);

    let r2 = dispatch(nums::NT_TEST_ALERT, &tf);
    //     // // // kprintln!("    [SYSCALL] NT_TEST_ALERT     => 0x{:x}", r2 as u32);

    let r3 = dispatch(nums::NT_QUERY_SYSTEM_INFORMATION, &tf);
    //     // // // kprintln!("    [SYSCALL] NT_QUERY_SYSTEM_INFORMATION(null) => 0x{:x}", r3 as u32);

    // 5a. Verify the three dispatch results are valid NTSTATUS values
    //     (high 16 bits of an i32 must fit in 0x8000_0000..=0xFFFF_FFFF
    //     for error, or 0x0000_0000..=0x7FFF_FFFF for success).
    let r = [r1 as i32, r2 as i32, r3 as i32];
    let mut all_valid = true;
    let mut success_count: u32 = 0;
    let mut error_count: u32 = 0;
    for &v in &r {
        if v >= 0 {
            success_count += 1;
        } else {
            error_count += 1;
        }
        // NTSTATUS values must have a defined severity: bits 30..31 are
        // never both zero. So e.g. 0x40000000 and 0xC0000000 are both
        // valid, but a flat 0 / uninitialised slot is suspicious. We
        // treat any value where the high 16 bits are completely zero as
        // an invalid NTSTATUS (those would only be SUCCESS, which we
        // already counted separately).
        if v != 0 && ((v as u32) >> 30) == 0 {
            all_valid = false;
        }
    }
    let _ = (success_count, error_count, r);

    // 6. Run SSDT smoke test
    crate::ke::ssdt::smoke_test();

    // 7. Run Shadow SSDT smoke test
    crate::ke::shadow_ssdt::smoke_test();

    //     // // // kprintln!("  [SYSCALL SMOKE OK] syscalls_total={} interrupts_total={}",
    // // //               total_syscalls(), total_interrupts());
    all_valid
}

// =====================================================================
// Windows 7 SP1 x64 system-call index.
//
// The tables themselves live in `nt61-abi::syscalls`, which is the
// single source of truth shared by the kernel and the Ring 3 DLLs.
// They used to be duplicated here and in a standalone
// `win7_sp1_syscalls.rs`; the duplication was removed so that a
// syscall number can never disagree between the two sides of the
// syscall boundary.
// =====================================================================
pub mod win7_sp1_syscalls {
    pub use nt61_abi::syscalls::{
        lookup_syscall, native_number, number, win32k_number, SyscallIndexEntry, NATIVE_SYSCALLS,
        WIN32K_SYSCALLS,
    };
}

// =====================================================================
// Merged from syscall_numbers.rs — NT native syscall number constants (u32).
// =====================================================================
// =====================================================================
// Merged from syscall_numbers.rs — NT native syscall number constants (u32).
// =====================================================================
pub mod syscall_numbers {
    //! Native NT 6.1 (Windows 7 SP1 x64 / Server 2008 R2) syscall numbers.
    //
    //! Every `Nt*` system call has a stable numeric identifier assigned
    //! when the call is added to the kernel. The number is the value
    //! the caller places in `rax` before executing the `syscall`
    //! instruction. The kernel dispatch table reads the number out of
    //! the saved `rax` and dispatches to the matching handler.
    //
    //! The constants in this module are the **canonical Windows 7 SP1 x64 /
    //! NT 6.1 build 7601** numbers from j00ru/windows-syscalls project.
    //
    //! ## Data Source
    //! - Primary: j00ru/windows-syscalls project (https://github.com/j00ru/windows-syscalls)
    //! - CSV file: x64/csv/nt.csv
    //! - Values: Windows 7 (SP1) column (11th column, index 10)

    // =================================================================
    // Wait & Synchronization
    // =================================================================

    pub const NT_WAIT_FOR_SINGLE_OBJECT: u32 = 0x0001;
    pub const NT_WAIT_FOR_MULTIPLE_OBJECTS: u32 = 0x0058;
    pub const NT_DELAY_EXECUTION: u32 = 0x0031;
    pub const NT_YIELD_EXECUTION: u32 = 0x0043;
    pub const NT_SIGNAL_AND_WAIT_FOR_SINGLE_OBJECT: u32 = 0x0176;
    pub const NT_WAIT_FOR_MULTIPLE_OBJECTS32: u32 = 0x0017;

    // =================================================================
    // System Information & control
    // =================================================================

    pub const NT_QUERY_SYSTEM_INFORMATION: u32 = 0x0033;
    pub const NT_SET_SYSTEM_INFORMATION: u32 = 0x016C;
    pub const NT_QUERY_SYSTEM_TIME: u32 = 0x0057;
    pub const NT_SET_SYSTEM_TIME: u32 = 0x016E;
    pub const NT_QUERY_INTERRUPT_TIME: u32 = 0x0058;
    pub const NT_SET_INTERRUPT_TIME: u32 = 0x0059;
    pub const NT_QUERY_TICK_COUNT: u32 = 0x00A8;
    pub const NT_GET_CURRENT_PROCESSOR_NUMBER: u32 = 0x00CB;
    pub const NT_SHUTDOWN_SYSTEM: u32 = 0x0174;
    pub const NT_TEST_ALERT: u32 = 0x017E;
    pub const NT_CALLBACK_RETURN: u32 = 0x0002;
    pub const NT_DISPLAY_STRING: u32 = 0x00B8;
    pub const NT_QUERY_PERFORMANCE_COUNTER: u32 = 0x002E;
    pub const NT_SET_SYSTEM_POWER_STATE: u32 = 0x016D;
    pub const NT_SHUTDOWN_WORKER_FACTORY: u32 = 0x0175;

    // =================================================================
    // Object Management
    // =================================================================

    pub const NT_CLOSE: u32 = 0x000C;
    pub const NT_QUERY_OBJECT: u32 = 0x000D;
    pub const NT_SET_INFORMATION_OBJECT: u32 = 0x0059;
    pub const NT_DUPLICATE_OBJECT: u32 = 0x0039;
    pub const NT_MAKE_PERMANENT_OBJECT: u32 = 0x00E4;
    pub const NT_MAKE_TEMPORARY_OBJECT: u32 = 0x00E5;

    // =================================================================
    // Process Management
    // =================================================================

    pub const NT_OPEN_PROCESS: u32 = 0x0023;
    pub const NT_QUERY_INFORMATION_PROCESS: u32 = 0x0016;
    pub const NT_SET_INFORMATION_PROCESS: u32 = 0x0019;
    pub const NT_CREATE_PROCESS: u32 = 0x009F;
    pub const NT_CREATE_PROCESS_EX: u32 = 0x004A;
    pub const NT_TERMINATE_PROCESS: u32 = 0x0029;
    pub const NT_OPEN_PROCESS_TOKEN: u32 = 0x00F9;
    pub const NT_OPEN_PROCESS_TOKEN_EX: u32 = 0x002D;
    pub const NT_SUSPEND_PROCESS: u32 = 0x017A;
    pub const NT_RESUME_PROCESS: u32 = 0x0144;

    // =================================================================
    // Thread Management
    // =================================================================

    pub const NT_OPEN_THREAD: u32 = 0x00FE;
    pub const NT_QUERY_INFORMATION_THREAD: u32 = 0x0022;
    pub const NT_SET_INFORMATION_THREAD: u32 = 0x000A;
    pub const NT_CREATE_THREAD: u32 = 0x004B;
    pub const NT_CREATE_THREAD_EX: u32 = 0x00A5;
    pub const NT_TERMINATE_THREAD: u32 = 0x0050;
    pub const NT_SUSPEND_THREAD: u32 = 0x017B;
    pub const NT_RESUME_THREAD: u32 = 0x004F;
    pub const NT_GET_CONTEXT_THREAD: u32 = 0x00CA;
    pub const NT_SET_CONTEXT_THREAD: u32 = 0x0150;
    pub const NT_OPEN_THREAD_TOKEN: u32 = 0x0021;
    pub const NT_OPEN_THREAD_TOKEN_EX: u32 = 0x002C;
    pub const NT_IMPERSONATE_THREAD: u32 = 0x00D5;
    pub const NT_REVERT_TO_SELF: u32 = 0x00D6;
    pub const NT_REGISTER_THREAD_TERMINATE_PORT: u32 = 0x0136;
    pub const NT_ALERT_THREAD: u32 = 0x006A;
    pub const NT_ALERT_RESUME_THREAD: u32 = 0x0069;

    // =================================================================
    // Memory Management
    // =================================================================

    pub const NT_ALLOCATE_VIRTUAL_MEMORY: u32 = 0x0015;
    pub const NT_FREE_VIRTUAL_MEMORY: u32 = 0x001B;
    pub const NT_READ_VIRTUAL_MEMORY: u32 = 0x003C;
    pub const NT_WRITE_VIRTUAL_MEMORY: u32 = 0x0037;
    pub const NT_QUERY_VIRTUAL_MEMORY: u32 = 0x0020;
    pub const NT_PROTECT_VIRTUAL_MEMORY: u32 = 0x004D;
    pub const NT_MAP_VIEW_OF_SECTION: u32 = 0x0025;
    pub const NT_UNMAP_VIEW_OF_SECTION: u32 = 0x0027;
    pub const NT_MAP_USER_PHYSICAL_PAGES: u32 = 0x00E7;
    pub const NT_ALLOCATE_USER_PHYSICAL_PAGES: u32 = 0x006D;
    pub const NT_FREE_USER_PHYSICAL_PAGES: u32 = 0x00C7;
    pub const NT_LOCK_VIRTUAL_MEMORY: u32 = 0x00E3;
    pub const NT_UNLOCK_VIRTUAL_MEMORY: u32 = 0x0189;
    pub const NT_FLUSH_VIRTUAL_MEMORY: u32 = 0x00C5;

    // =================================================================
    // File I/O
    // =================================================================

    pub const NT_CREATE_FILE: u32 = 0x0052;
    pub const NT_OPEN_FILE: u32 = 0x0030;
    pub const NT_READ_FILE: u32 = 0x0003;
    pub const NT_WRITE_FILE: u32 = 0x0005;
    pub const NT_QUERY_INFORMATION_FILE: u32 = 0x000E;
    pub const NT_SET_INFORMATION_FILE: u32 = 0x0024;
    pub const NT_DELETE_FILE: u32 = 0x00B2;
    pub const NT_FLUSH_BUFFERS_FILE: u32 = 0x0048;
    pub const NT_FLUSH_BUFFERS_FILE_EX: u32 = 0x00D2;
    pub const NT_QUERY_VOLUME_INFORMATION_FILE: u32 = 0x0046;
    pub const NT_SET_VOLUME_INFORMATION_FILE: u32 = 0x0173;
    pub const NT_QUERY_DIRECTORY_FILE: u32 = 0x0032;
    pub const NT_QUERY_ATTRIBUTES_FILE: u32 = 0x003A;
    pub const NT_QUERY_FULL_ATTRIBUTES_FILE: u32 = 0x0113;
    pub const NT_LOCK_FILE: u32 = 0x00E0;
    pub const NT_UNLOCK_FILE: u32 = 0x0188;
    pub const NT_NOTIFY_CHANGE_DIRECTORY_FILE: u32 = 0x00EA;
    pub const NT_CANCEL_IO_FILE: u32 = 0x005A;
    pub const NT_CANCEL_IO_FILE_EX: u32 = 0x0086;
    pub const NT_READ_FILE_SCATTER: u32 = 0x002B;
    pub const NT_WRITE_FILE_GATHER: u32 = 0x0018;
    pub const NT_QUERY_EA_FILE: u32 = 0x0112;
    pub const NT_SET_EA_FILE: u32 = 0x0156;

    // =================================================================
    // Section / Shared Memory
    // =================================================================

    pub const NT_CREATE_SECTION: u32 = 0x0047;
    pub const NT_OPEN_SECTION: u32 = 0x0034;
    pub const NT_EXTEND_SECTION: u32 = 0x00BF;
    pub const NT_CREATE_NAMED_PIPE_FILE: u32 = 0x009B;
    pub const NT_CREATE_MAILSLOT_FILE: u32 = 0x0099;
    pub const NT_CREATE_PROFILE: u32 = 0x00A0;

    // =================================================================
    // Synchronization Objects
    // =================================================================

    pub const NT_CREATE_EVENT: u32 = 0x0045;
    pub const NT_OPEN_EVENT: u32 = 0x003D;
    pub const NT_SET_EVENT: u32 = 0x000B;
    pub const NT_RESET_EVENT: u32 = 0x0141;
    pub const NT_CLEAR_EVENT: u32 = 0x003B;
    pub const NT_PULSE_EVENT: u32 = 0x010C;
    pub const NT_QUERY_EVENT: u32 = 0x0053;
    pub const NT_CREATE_MUTANT: u32 = 0x009A;
    pub const NT_OPEN_MUTANT: u32 = 0x00F6;
    pub const NT_RELEASE_MUTANT: u32 = 0x001D;
    pub const NT_QUERY_MUTANT: u32 = 0x0121;
    pub const NT_CREATE_SEMAPHORE: u32 = 0x00A3;
    pub const NT_OPEN_SEMAPHORE: u32 = 0x00FB;
    pub const NT_RELEASE_SEMAPHORE: u32 = 0x0007;
    pub const NT_QUERY_SEMAPHORE: u32 = 0x0128;
    pub const NT_CREATE_TIMER: u32 = 0x00A6;
    pub const NT_OPEN_TIMER: u32 = 0x00FF;
    pub const NT_SET_TIMER: u32 = 0x005F;
    pub const NT_CANCEL_TIMER: u32 = 0x005E;
    pub const NT_QUERY_TIMER: u32 = 0x0035;
    pub const NT_QUERY_TIMER_RESOLUTION: u32 = 0x012D;
    pub const NT_SET_TIMER_RESOLUTION: u32 = 0x0171;
    pub const NT_CREATE_KEYED_EVENT: u32 = 0x0098;
    pub const NT_OPEN_KEYED_EVENT: u32 = 0x00F5;
    pub const NT_RELEASE_KEYED_EVENT: u32 = 0x0137;
    pub const NT_WAIT_FOR_KEYED_EVENT: u32 = 0x018C;
    pub const NT_CREATE_EVENT_PAIR: u32 = 0x0093;
    pub const NT_OPEN_EVENT_PAIR: u32 = 0x00EF;
    pub const NT_SET_EVENT_BOOST_PRIORITY: u32 = 0x002A;

    // =================================================================
    // Registry
    // =================================================================

    pub const NT_CREATE_KEY: u32 = 0x001A;
    pub const NT_OPEN_KEY: u32 = 0x000F;
    pub const NT_DELETE_KEY: u32 = 0x00B3;
    pub const NT_DELETE_VALUE_KEY: u32 = 0x00B6;
    pub const NT_QUERY_KEY: u32 = 0x0013;
    pub const NT_SET_VALUE_KEY: u32 = 0x005D;
    pub const NT_QUERY_VALUE_KEY: u32 = 0x0014;
    pub const NT_ENUMERATE_KEY: u32 = 0x002F;
    pub const NT_ENUMERATE_VALUE_KEY: u32 = 0x0010;
    pub const NT_FLUSH_KEY: u32 = 0x00C3;
    pub const NT_LOAD_KEY: u32 = 0x00DD;
    pub const NT_LOAD_KEY2: u32 = 0x00DE;
    pub const NT_UNLOAD_KEY: u32 = 0x0185;
    pub const NT_SAVE_KEY: u32 = 0x0149;
    pub const NT_SAVE_KEY_EX: u32 = 0x014A;
    pub const NT_NOTIFY_CHANGE_KEY: u32 = 0x00EB;
    pub const NT_QUERY_OPEN_SUB_KEYS: u32 = 0x0122;
    pub const NT_COMPACT_KEYS: u32 = 0x008B;
    pub const NT_COMPRESS_KEY: u32 = 0x008E;
    pub const NT_CREATE_KEY_TRANSACTED: u32 = 0x0097;
    pub const NT_OPEN_KEY_TRANSACTED: u32 = 0x00F3;
    pub const NT_OPEN_KEY_TRANSACTED_EX: u32 = 0x00F4;
    pub const NT_DELETE_KEY_TRANSACTED: u32 = 0x00F3;
    pub const NT_SET_VALUE_KEY_TRANSACTED: u32 = 0x0160;
    pub const NT_SET_INFORMATION_KEY: u32 = 0x015C;
    pub const NT_RENAME_KEY: u32 = 0x013B;
    pub const NT_QUERY_MULTIPLE_VALUE_KEY: u32 = 0x0120;

    // =================================================================
    // Directory & Symbolic Link
    // =================================================================

    pub const NT_CREATE_DIRECTORY_OBJECT: u32 = 0x0091;
    pub const NT_OPEN_DIRECTORY_OBJECT: u32 = 0x0055;
    pub const NT_QUERY_DIRECTORY_OBJECT: u32 = 0x0110;
    pub const NT_CREATE_SYMBOLIC_LINK_OBJECT: u32 = 0x00A4;
    pub const NT_OPEN_SYMBOLIC_LINK_OBJECT: u32 = 0x00FD;
    pub const NT_QUERY_SYMBOLIC_LINK_OBJECT: u32 = 0x0129;

    // =================================================================
    // Security
    // =================================================================

    pub const NT_ACCESS_CHECK: u32 = 0x0061;
    pub const NT_ACCESS_CHECK_AND_AUDIT_ALARM: u32 = 0x0026;
    pub const NT_SET_SECURITY_OBJECT: u32 = 0x0169;
    pub const NT_QUERY_SECURITY_OBJECT: u32 = 0x0127;
    pub const NT_PRIVILEGE_CHECK: u32 = 0x0107;
    pub const NT_IMPERSONATE_ANONYMOUS_TOKEN: u32 = 0x00D4;

    // =================================================================
    // Token Management
    // =================================================================

    pub const NT_CREATE_TOKEN: u32 = 0x00A7;
    pub const NT_OPEN_OBJECT_AUDIT_ALARM: u32 = 0x00F7;
    pub const NT_FILTER_TOKEN: u32 = 0x00C0;
    pub const NT_ADJUST_PRIVILEGES_TOKEN: u32 = 0x003E;
    pub const NT_ADJUST_GROUPS_TOKEN: u32 = 0x0068;
    pub const NT_DUPLICATE_TOKEN: u32 = 0x003F;
    pub const NT_QUERY_INFORMATION_TOKEN: u32 = 0x001E;
    pub const NT_SET_INFORMATION_TOKEN: u32 = 0x015E;

    // =================================================================
    // ALPC
    // =================================================================

    pub const NT_CREATE_PORT: u32 = 0x009D;
    pub const NT_CONNECT_PORT: u32 = 0x008F;
    pub const NT_LISTEN_PORT: u32 = 0x00DB;
    pub const NT_ACCEPT_CONNECT_PORT: u32 = 0x0060;
    pub const NT_SEND_WAIT_RECEIVE_PORT: u32 = 0x00AF;
    pub const NT_IMPERSONATE_CLIENT_OF_PORT: u32 = 0x001C;
    pub const NT_REPLY_WAIT_RECEIVE_PORT: u32 = 0x0008;
    pub const NT_REPLY_PORT: u32 = 0x0009;
    pub const NT_REPLY_WAIT_RECEIVE_PORT_EX: u32 = 0x0028;
    pub const NT_CREATE_PORT_SECTION: u32 = 0x00BD;
    pub const NT_SECURE_CONNECT_PORT: u32 = 0x014C;
    pub const NT_QUERY_INFORMATION_PORT: u32 = 0x0117;
    pub const NT_REQUEST_PORT: u32 = 0x0140;
    pub const NT_REQUEST_WAIT_REPLY_PORT: u32 = 0x001F;
    pub const NT_COMPLETE_CONNECT_PORT: u32 = 0x008D;
    pub const NT_REPLY_WAIT_REPLY_PORT: u32 = 0x013F;

    // =================================================================
    // Job Objects
    // =================================================================

    pub const NT_CREATE_JOB_SET: u32 = 0x0096;
    pub const NT_CREATE_JOB_OBJECT: u32 = 0x0095;
    pub const NT_OPEN_JOB_OBJECT: u32 = 0x00F1;
    pub const NT_ASSIGN_PROCESS_TO_JOB_OBJECT: u32 = 0x0085;
    pub const NT_TERMINATE_JOB_OBJECT: u32 = 0x017D;
    pub const NT_QUERY_JOB_INFORMATION: u32 = 0x0178;
    pub const NT_SET_JOB_INFORMATION: u32 = 0x0179;
    pub const NT_QUERY_INFORMATION_JOB_OBJECT: u32 = 0x0116;
    pub const NT_SET_INFORMATION_JOB_OBJECT: u32 = 0x015B;
    pub const NT_IS_PROCESS_IN_JOB: u32 = 0x004C;

    // =================================================================
    // Locale & Culture
    // =================================================================

    pub const NT_QUERY_DEFAULT_LOCALE: u32 = 0x0012;
    pub const NT_SET_DEFAULT_LOCALE: u32 = 0x0153;
    pub const NT_QUERY_DEFAULT_UILANGUAGE: u32 = 0x0041;
    pub const NT_SET_DEFAULT_UILANGUAGE: u32 = 0x0189;
    pub const NT_QUERY_DEFAULT_HARD_ERROR_MODE: u32 = 0x0108;
    pub const NT_SET_DEFAULT_HARD_ERROR_MODE: u32 = 0x0187;
    pub const NT_READ_ONLY_LOGON_SESSION_TREE: u32 = 0x0180;
    pub const NT_VERIFY_SESSION: u32 = 0x0181;

    // =================================================================
    // Paging & Memory Management
    // =================================================================

    pub const NT_CREATE_PAGING_FILE: u32 = 0x009C;
    pub const NT_INITIALIZE_REGISTRY: u32 = 0x00D7;
    pub const NT_CREATE_PROCESS_STATE: u32 = 0x0166;
    pub const NT_CREATE_PROCESS_STATE_CHANGE: u32 = 0x00BD;

    // =================================================================
    // User Processes
    // =================================================================

    pub const NT_CREATE_USER_PROCESS: u32 = 0x00AA;

    // =================================================================
    // Exception Handling
    // =================================================================

    pub const NT_RAISE_EXCEPTION: u32 = 0x012F;
    pub const NT_RAISE_HARD_ERROR: u32 = 0x0130;
    pub const NT_CONTINUE: u32 = 0x0040;

    // =================================================================
    // LDT
    // =================================================================

    pub const NT_SET_LDT_ENTRIES: u32 = 0x0165;
    pub const NT_SET_LDT_SIZE: u32 = 0x0168;

    // =================================================================
    // DLL & Process Loading
    // =================================================================

    pub const NT_QUERY_LICENSE_VALUE: u32 = 0x011F;
    pub const NT_ALLOCATE_LOCALLY_UNIQUE_ID: u32 = 0x006B;
    pub const NT_ALLOCATE_UUIDS: u32 = 0x006E;

    // =================================================================
    // Transaction Manager
    // =================================================================

    pub const NT_CREATE_TRANSACTION: u32 = 0x00A8;
    pub const NT_OPEN_TRANSACTION: u32 = 0x0100;
    pub const NT_COMMIT_TRANSACTION: u32 = 0x008A;
    pub const NT_ROLLBACK_TRANSACTION: u32 = 0x0147;
    pub const NT_ENUMERATE_TRANSACTION: u32 = 0x0148;
    pub const NT_SET_INFORMATION_TRANSACTION: u32 = 0x015F;
    pub const NT_QUERY_INFORMATION_TRANSACTION: u32 = 0x0119;
    pub const NT_CREATE_TRANSACTION_MANAGER: u32 = 0x00A9;
    pub const NT_OPEN_TRANSACTION_MANAGER: u32 = 0x0101;
    pub const NT_COMMIT_COMPLETE: u32 = 0x0088;
    pub const NT_ROLLBACK_COMPLETE: u32 = 0x0145;
    pub const NT_ENUMERATE_TRANSACTION_OBJECT: u32 = 0x00BE;

    // =================================================================
    // System Service Table Information
    // =================================================================

    pub const NT_SET_SYSTEM_INFORMATION_EX: u32 = 0x012C;
    pub const NT_QUERY_SYSTEM_INFORMATION_EX: u32 = 0x012C;
    pub const NT_TRIM_PROCESS_WORKING_SET: u32 = 0x0157;

    // =================================================================
    // Worker Factory
    // =================================================================

    pub const NT_WAIT_FOR_WORK_VIA_WORKER_FACTORY: u32 = 0x018D;
    pub const NT_SET_INFORMATION_WORKER_FACTORY: u32 = 0x0161;
    pub const NT_QUERY_INFORMATION_WORKER_FACTORY: u32 = 0x011B;
    pub const NT_RELEASE_WORKER_FACTORY_WORKER: u32 = 0x0138;

    // =================================================================
    // Private Namespaces
    // =================================================================

    pub const NT_CREATE_PRIVATE_NAMESPACE: u32 = 0x009E;
    pub const NT_OPEN_PRIVATE_NAMESPACE: u32 = 0x00F8;
    pub const NT_DELETE_PRIVATE_NAMESPACE: u32 = 0x00B5;

    // =================================================================
    // Debug
    // =================================================================

    pub const NT_CREATE_DEBUG_OBJECT: u32 = 0x0090;
    pub const NT_DEBUG_ACTIVE_PROCESS: u32 = 0x00AD;
    pub const NT_REMOVE_PROCESS_DEBUG: u32 = 0x013A;
    pub const NT_WAIT_FOR_DEBUG_EVENT: u32 = 0x018B;
    pub const NT_DEBUG_CONTINUE: u32 = 0x00AE;
    pub const NT_SET_INFORMATION_DEBUG_OBJECT: u32 = 0x0159;
    pub const NT_QUERY_INFORMATION_DEBUG_OBJECT: u32 = 0x0125;
    pub const NT_DEBUG_PRINT: u32 = 0x003D;

    // =================================================================
    // System environment
    // =================================================================

    pub const NT_QUERY_SYSTEM_ENVIRONMENT: u32 = 0x0081;
    pub const NT_SET_SYSTEM_ENVIRONMENT_VALUE: u32 = 0x016A;
    pub const NT_SET_SYSTEM_ENVIRONMENT_VALUE_EX: u32 = 0x016B;
    pub const NT_ENUMERATE_BOOT_ENTRIES: u32 = 0x00BB;
    pub const NT_ENUMERATE_DRIVER_ENTRIES: u32 = 0x00BC;
    pub const NT_ENUMERATE_DRIVER_PACKAGE: u32 = 0x01A9;
    pub const NT_ADD_BOOT_ENTRY: u32 = 0x0066;
    pub const NT_DELETE_BOOT_ENTRY: u32 = 0x00B0;
    pub const NT_MODIFY_BOOT_ENTRY: u32 = 0x00E8;
    pub const NT_SET_BOOT_ENTRY_ORDER: u32 = 0x014E;
    pub const NT_SET_BOOT_OPTIONS: u32 = 0x014F;
    pub const NT_QUERY_BOOT_ENTRY_ORDER: u32 = 0x010D;
    pub const NT_QUERY_BOOT_OPTIONS: u32 = 0x010E;
    pub const NT_ADD_DRIVER_ENTRY: u32 = 0x0067;
    pub const NT_DELETE_DRIVER_ENTRY: u32 = 0x00B1;
    pub const NT_MODIFY_DRIVER_ENTRY: u32 = 0x00E9;
    pub const NT_SET_DRIVER_ENTRY_ORDER: u32 = 0x0155;
    pub const NT_QUERY_DRIVER_ENTRY_ORDER: u32 = 0x0111;

    // =================================================================
    // Plug and Play / power
    // =================================================================

    pub const NT_PLUG_PLAY_CONTROL: u32 = 0x0102;
    pub const NT_GET_PLUG_PLAY_EVENT: u32 = 0x00D2;
    pub const NT_POWER_INFORMATION: u32 = 0x005C;
    pub const NT_INITIATE_POWER_ACTION: u32 = 0x00D8;
    pub const NT_SET_THREAD_EXECUTION_STATE: u32 = 0x016F;
    pub const NT_GET_DEVICE_POWER_STATE: u32 = 0x00CC;
    pub const NT_IS_SYSTEM_RESUME_AUTOMATIC: u32 = 0x00D9;
    pub const NT_REQUEST_WAKEUP_LATENCY: u32 = 0x0084;
    pub const NT_SET_INTERVAL_PROFILE: u32 = 0x0162;
    pub const NT_QUERY_INTERVAL_PROFILE: u32 = 0x011D;

    // =================================================================
    // IO Completion
    // =================================================================

    pub const NT_CREATE_IO_COMPLETION: u32 = 0x0094;
    pub const NT_OPEN_IO_COMPLETION: u32 = 0x00F0;
    pub const NT_SET_IO_COMPLETION: u32 = 0x0163;
    pub const NT_SET_IO_COMPLETION_EX: u32 = 0x0164;
    pub const NT_QUERY_IO_COMPLETION: u32 = 0x011E;
    pub const NT_REMOVE_IO_COMPLETION: u32 = 0x0006;
    pub const NT_REMOVE_IO_COMPLETION_EX: u32 = 0x0139;
    pub const NT_WAIT_FOR_ALERT_BY_THREAD_ID: u32 = 0x01A6;

    // =================================================================
    // Atom Table
    // =================================================================

    pub const NT_ADD_ATOM: u32 = 0x0044;
    pub const NT_DELETE_ATOM: u32 = 0x00AF;
    pub const NT_QUERY_INFORMATION_ATOM: u32 = 0x0114;
    pub const NT_FIND_ATOM: u32 = 0x0011;

    // =================================================================
    // environment / NLS
    // =================================================================

    pub const NT_INITIALIZE_NLS_FILES: u32 = 0x00D6;
    pub const NT_GET_NLS_SECTION_PTR: u32 = 0x00D0;
    pub const NT_QUERY_INSTALL_UILANGUAGE: u32 = 0x007B;
    pub const NT_IS_UILANGUAGE_COMMITTED: u32 = 0x00AA;
    pub const NT_FLUSH_INSTALL_UILANGUAGE: u32 = 0x0053;
    pub const NT_SET_UUID_SEED: u32 = 0x0172;

    // =================================================================
    // Resource Manager
    // =================================================================

    pub const NT_GET_NOTIFICATION_RESOURCE_MANAGER: u32 = 0x00D1;
    pub const NT_QUERY_INFORMATION_TRANSACTION_MANAGER: u32 = 0x011A;
    pub const NT_SET_INFORMATION_TRANSACTION_MANAGER: u32 = 0x0160;

    // =================================================================
    // Other Important Syscalls
    // =================================================================

    pub const NT_DEVICE_IO_CONTROL_FILE: u32 = 0x0004;
    pub const NT_FS_CONTROL_FILE: u32 = 0x0036;
    pub const NT_LOAD_DRIVER: u32 = 0x00DC;
    pub const NT_UNLOAD_DRIVER: u32 = 0x0184;
    pub const NT_QUERY_SECTION: u32 = 0x004E;
    pub const NT_WAIT_HIGH_EVENT_PAIR: u32 = 0x018E;
    pub const NT_WAIT_LOW_EVENT_PAIR: u32 = 0x00AA;
    pub const NT_SET_HIGH_EVENT_PAIR: u32 = 0x0157;
    pub const NT_SET_LOW_EVENT_PAIR: u32 = 0x0166;
    pub const NT_SET_HIGH_WAIT_LOW_EVENT_PAIR: u32 = 0x0158;
    pub const NT_SET_LOW_WAIT_HIGH_EVENT_PAIR: u32 = 0x0167;
    pub const NT_TRANSLATE_FILE_PATH: u32 = 0x0182;
    pub const NT_START_SERVICE: u32 = 0x00EC;

    // =================================================================
    // APC
    // =================================================================

    pub const NT_QUEUE_APC_THREAD: u32 = 0x0042;
    pub const NT_READ_REQUEST_DATA: u32 = 0x0051;
    pub const NT_WRITE_REQUEST_DATA: u32 = 0x0054;

    // =================================================================
    // Profile
    // =================================================================

    pub const NT_START_PROFILE: u32 = 0x0178;
    pub const NT_STOP_PROFILE: u32 = 0x0179;
    pub const NT_QUERY_DEBUG_FILTER_STATE: u32 = 0x010F;
    pub const NT_SET_DEBUG_FILTER_STATE: u32 = 0x0151;

    // =================================================================
    // System Debug control
    // =================================================================

    pub const NT_SYSTEM_DEBUG_CONTROL: u32 = 0x017C;

    // =================================================================
    // Enumerate System environment Values
    // =================================================================

    pub const NT_ENUMERATE_SYSTEM_ENVIRONMENT_VALUES_EX: u32 = 0x00BD;

    // =================================================================
    // Notify Change Session
    // =================================================================

    pub const NT_NOTIFY_CHANGE_SESSION: u32 = 0x00ED;

    // =================================================================
    // Flush Process write Buffers
    // =================================================================

    pub const NT_FLUSH_PROCESS_WRITE_BUFFERS: u32 = 0x00C4;

    // =================================================================
    // Serialize Boot
    // =================================================================

    pub const NT_SERIALIZE_BOOT: u32 = 0x014D;

    // =================================================================
    // Maximum syscall number in native NT table
    // =================================================================

    pub const MAX_NT_SYSCALL: u32 = 0x01A9;

    // =================================================================
    // NT6.1.7601-kernel private syscalls (cmd.exe host)
    // =================================================================
    //
    // These IDs are reserved for the kernel's own user-mode
    // command host (`C:\Windows\System32\cmd.exe`). They live in the
    // 0x0200..=0x02FF range, which is above the NT syscall table
    // (max 0x01A9) so they cannot collide with any NT service.
    //
    // See `arch::x86_64::syscall::dispatch` for the kernel-side
    // implementation and `system_image::build_cmd_exe` for the
    // user-mode stub.

    /// Tell the kernel to run `C:\tests\autoexec.bat` and return.
    /// The kernel reads the file via FAT32, parses every line with
    /// `libs::cmd::bat_parser`, and lets the batch processor emit
    /// its output through the same console the cmd.exe stub is
    /// attached to. The stub is parked inside the syscall until the
    /// batch is finished, then the syscall returns.
    pub const SYS_RUN_AUTOEXEC: u32 = 0x0200;

    /// Terminate the current user-mode process. `arg0` is the exit
    /// code (passed in `rdi` per the NT x64 calling convention).
    pub const SYS_EXIT_PROCESS: u32 = 0x0201;

    /// Print a single byte to the kernel debug console / serial
    /// port. `arg0` (in `al` / `rdi`'s low byte) is the ASCII code
    /// to emit. This exists so the Ring-3 `cmd.exe` stub and any
    /// future user-mode diagnostic can produce log output without
    /// touching the privileged I/O port space. The Ring-3 IOPL is
    /// always 0 in this kernel, so `out dx, al` raises #GP; the
    /// syscall avoids that entirely. Returns 0 on success.
    pub const SYS_PUTCHAR: u32 = 0x0202;

    /// Poll the PS/2 controller for a pressed key. Returns 0 (no
    /// key available) or a non-zero value whose low byte holds the
    /// PS/2 scancode. The cmd.exe stub polls this in a tight loop
    /// to feed its character-by-character read routine. The kernel
    /// also drains the serial UART's RX FIFO and converts CRLF to
    /// `\n`, so the same syscall also surfaces any bytes the QEMU
    /// monitor typed at the guest.
    pub const SYS_POLL_KEY: u32 = 0x0203;

    /// Block-wait for a single keypress, returning the PS/2
    /// scancode (or a serial RX byte) in `arg0`. Used by the cmd
    /// read-line routine when the user has disabled the busy-poll
    /// loop.
    pub const SYS_GET_KEY: u32 = 0x0204;

    /// read a line of input from the serial UART into the user buffer.
    /// `arg0` is a pointer to a user-mode buffer, `arg1` is the buffer
    /// length in bytes. The kernel drains the COM1 RX FIFO (same
    /// source the QEMU `-serial mon:stdio` write feeds) until it sees
    /// `\r`, `\n`, or the buffer fills. Returns the number of bytes
    /// copied (not counting the terminator) or a negative NTSTATUS on
    /// copy fault.
    pub const SYS_READ_LINE: u32 = 0x0211;

    /// Clear the visible console on the bootvid linear framebuffer
    /// AND home the cursor at (0, 0). The `cls` cmd.exe built-in
    /// invokes this so the user sees a fresh blank screen instead of
    /// the boot-time log being half-painted over the Win7 banner.
    /// Returns 0 on success. `arg0`/`arg1`/`arg2` are unused.
    pub const SYS_CLEARSCREEN: u32 = 0x0205;

    /// Spawn the named subsystem EXE from disk. `arg0` is a pointer
    /// to a UTF-8 (or UTF-16, depending on the path convention) NUL-
    /// terminated path string in the caller's address space (Ring-3
    /// user pointer). The kernel copies the bytes safely into a
    /// 256-byte scratch buffer, asks the mounted file-system driver
    /// (NTFS, FAT32, or EXT4) for the file bytes, parses the PE, and
    /// creates a new Ring-3 process. The new process's PID is
    /// returned in `rax`. Used by `winlogon.exe` to spawn the
    /// Session-1 csrss.exe and `userinit.exe`, and by `userinit.exe`
    /// to spawn `cmd.exe`.
    pub const SYS_SPAWN_SUBSYSTEM_PROCESS: u32 = 0x0210;

    /// read the CMOS real-time clock and copy a 16-byte
    /// `cmos::TimeFields`-shaped struct into the user buffer at
    /// `arg0`. The layout (little endian) is:
    ///
    /// ```text
    ///   [0..2]  year      (u16, full year e.g. 2026)
    ///   [2]     month     (u8, 1..12)
    ///   [3]     day       (u8, 1..31)
    ///   [4]     hour      (u8, 0..23)
    ///   [5]     minute    (u8, 0..59)
    ///   [6]     second    (u8, 0..59)
    ///   [7]     weekday   (u8, 0..6, Sunday = 0)
    ///   [8..16] reserved  (zero)
    /// ```
    ///
    /// Returns the number of bytes written (16) on success, or 0 if
    /// the CMOS update flag was stuck or `arg0` is NULL. The data is
    /// read directly from CMOS registers via `hal::x86_64::cmos`,
    /// which already handles BCD decode and 12/24-hour mode. Used by
    /// the user-mode `cmd.exe` stub's `time` and `date` builtins so
    /// the value printed matches the host's wall clock instead of a
    /// hard-coded literal in `autoexec.bat`.
    pub const SYS_GET_RTC: u32 = 0x0212;

    /// Copy the active IPv4 configuration into the user buffer at
    /// `arg0`. The 16-byte layout (network byte order) is:
    ///
    /// ```text
    ///   [0..4]   IPv4 address    (4 bytes, big endian)
    ///   [4..8]   subnet mask     (4 bytes, big endian)
    ///   [8..12]  default gateway (4 bytes, big endian)
    ///   [12]     interface count (u8)
    ///   [13..16] reserved        (zero)
    /// ```
    ///
    /// Returns the number of bytes written (16) on success, or 0 if
    /// `arg0` is NULL. If no IP interface has been registered yet,
    /// the kernel falls back to a sane default (127.0.0.1 / 255.0.0.0
    /// / 0.0.0.0, count = 1) so the user-mode `ipconfig` always
    /// prints a valid answer even on a network-less QEMU harness.
    /// Used by the user-mode `cmd.exe` stub's `ipconfig` builtin.
    pub const SYS_NETCFG_GET: u32 = 0x0213;

    /// Execute one complete command line from the Ring-3 command host.
    /// `arg0` points to UTF-8 bytes and `arg1` is the byte count. The
    /// command text is not case-normalized by user mode; kernel command
    /// matching remains case-insensitive while preserving input display.
    pub const SYS_CMD_EXECUTE: u32 = 0x0215;
    pub const SYS_PRINT_PROMPT: u32 = 0x0216;

    /// Return control to firmware. `arg0` is one of the
    /// `FIRMWARE_REENTRY` values understood by
    /// `hal::x86_64::halt::hal_return_to_firmware`:
    ///
    ///   0 = HAL_HALT (park the CPU)
    ///   1 = HAL_REBOOT (warm reset)
    ///   2 = HAL_SHUTDOWN (ACPI S5 soft-off)
    ///
    /// The syscall never returns.
    pub const SYS_RETURN_TO_FIRMWARE: u32 = 0x0214;
    pub const SYS_FILE_OPEN: u32 = 0x0220;
    pub const SYS_FILE_READ: u32 = 0x0221;
    pub const SYS_FILE_CLOSE: u32 = 0x0223;
    pub const SYS_FILE_DELETE: u32 = 0x0225;
    pub const SYS_FILE_LIST: u32 = 0x0226;
}

#[cfg(test)]
mod syscall_number_validation {
    use super::syscall_numbers;
    use super::win7_sp1_syscalls;

    fn authoritative() -> alloc::collections::BTreeMap<&'static str, u16> {
        win7_sp1_syscalls::NATIVE_SYSCALLS
            .iter()
            .map(|e| (e.name, e.number))
            .collect()
    }

    #[test]
    fn syscall_numbers_match_win7_table() {
        let table = authoritative();
        let checks: &[(&str, u32)] = &[
            ("NtReadFile", syscall_numbers::NT_READ_FILE),
            ("NtWriteFile", syscall_numbers::NT_WRITE_FILE),
            ("NtOpenFile", syscall_numbers::NT_OPEN_FILE),
            (
                "NtDeviceIoControlFile",
                syscall_numbers::NT_DEVICE_IO_CONTROL_FILE,
            ),
            ("NtCreateEvent", syscall_numbers::NT_CREATE_EVENT),
            ("NtSetEvent", syscall_numbers::NT_SET_EVENT),
            ("NtQueueApcThread", syscall_numbers::NT_QUEUE_APC_THREAD),
            (
                "NtQueryPerformanceCounter",
                syscall_numbers::NT_QUERY_PERFORMANCE_COUNTER,
            ),
            ("NtCreateKey", syscall_numbers::NT_CREATE_KEY),
            ("NtOpenKey", syscall_numbers::NT_OPEN_KEY),
            ("NtSetValueKey", syscall_numbers::NT_SET_VALUE_KEY),
            ("NtQueryValueKey", syscall_numbers::NT_QUERY_VALUE_KEY),
            ("NtEnumerateKey", syscall_numbers::NT_ENUMERATE_KEY),
            (
                "NtEnumerateValueKey",
                syscall_numbers::NT_ENUMERATE_VALUE_KEY,
            ),
            ("NtCreateSemaphore", syscall_numbers::NT_CREATE_SEMAPHORE),
            ("NtReleaseSemaphore", syscall_numbers::NT_RELEASE_SEMAPHORE),
            ("NtCreateTimer", syscall_numbers::NT_CREATE_TIMER),
            ("NtSetTimer", syscall_numbers::NT_SET_TIMER),
            ("NtCancelTimer", syscall_numbers::NT_CANCEL_TIMER),
            (
                "NtQueryInformationFile",
                syscall_numbers::NT_QUERY_INFORMATION_FILE,
            ),
            (
                "NtSetInformationFile",
                syscall_numbers::NT_SET_INFORMATION_FILE,
            ),
            ("NtFlushBuffersFile", syscall_numbers::NT_FLUSH_BUFFERS_FILE),
            (
                "NtQueryDirectoryFile",
                syscall_numbers::NT_QUERY_DIRECTORY_FILE,
            ),
            (
                "NtQueryVolumeInformationFile",
                syscall_numbers::NT_QUERY_VOLUME_INFORMATION_FILE,
            ),
            (
                "NtSetInformationThread",
                syscall_numbers::NT_SET_INFORMATION_THREAD,
            ),
            ("NtYieldExecution", syscall_numbers::NT_YIELD_EXECUTION),
            ("NtContinue", syscall_numbers::NT_CONTINUE),
            ("NtOpenProcess", syscall_numbers::NT_OPEN_PROCESS),
            ("NtCreateProcessEx", syscall_numbers::NT_CREATE_PROCESS_EX),
            ("NtTerminateProcess", syscall_numbers::NT_TERMINATE_PROCESS),
            (
                "NtAllocateVirtualMemory",
                syscall_numbers::NT_ALLOCATE_VIRTUAL_MEMORY,
            ),
            (
                "NtFreeVirtualMemory",
                syscall_numbers::NT_FREE_VIRTUAL_MEMORY,
            ),
            (
                "NtReadVirtualMemory",
                syscall_numbers::NT_READ_VIRTUAL_MEMORY,
            ),
            (
                "NtWriteVirtualMemory",
                syscall_numbers::NT_WRITE_VIRTUAL_MEMORY,
            ),
            (
                "NtProtectVirtualMemory",
                syscall_numbers::NT_PROTECT_VIRTUAL_MEMORY,
            ),
            (
                "NtQueryVirtualMemory",
                syscall_numbers::NT_QUERY_VIRTUAL_MEMORY,
            ),
            ("NtCreateFile", syscall_numbers::NT_CREATE_FILE),
            ("NtClose", syscall_numbers::NT_CLOSE),
            ("NtCreateSection", syscall_numbers::NT_CREATE_SECTION),
            (
                "NtMapViewOfSection",
                syscall_numbers::NT_MAP_VIEW_OF_SECTION,
            ),
            (
                "NtUnmapViewOfSection",
                syscall_numbers::NT_UNMAP_VIEW_OF_SECTION,
            ),
            (
                "NtQuerySystemInformation",
                syscall_numbers::NT_QUERY_SYSTEM_INFORMATION,
            ),
            (
                "NtWaitForSingleObject",
                syscall_numbers::NT_WAIT_FOR_SINGLE_OBJECT,
            ),
            (
                "NtWaitForMultipleObjects",
                syscall_numbers::NT_WAIT_FOR_MULTIPLE_OBJECTS,
            ),
            ("NtCreateMutant", syscall_numbers::NT_CREATE_MUTANT),
            ("NtOpenMutant", syscall_numbers::NT_OPEN_MUTANT),
            ("NtReleaseMutant", syscall_numbers::NT_RELEASE_MUTANT),
            (
                "NtCreateUserProcess",
                syscall_numbers::NT_CREATE_USER_PROCESS,
            ),
            ("NtRaiseException", syscall_numbers::NT_RAISE_EXCEPTION),
            ("NtCreateThreadEx", syscall_numbers::NT_CREATE_THREAD_EX),
            ("NtResumeThread", syscall_numbers::NT_RESUME_THREAD),
        ];
        for (name, value) in checks {
            let expected = table
                .get(name)
                .unwrap_or_else(|| panic!("missing {}", name));
            assert_eq!(*expected as u32, *value, "{} mismatch", name);
        }
    }
}
