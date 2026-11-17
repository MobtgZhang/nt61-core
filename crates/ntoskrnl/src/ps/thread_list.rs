//! Process membership uses ETHREAD.thread_list_entry; the KTHREAD node belongs
//! exclusively to scheduler ready queues. This lock serializes membership
//! publication, traversal and removal, independently of ready-queue locks.
//!
//! Lock order is process registry -> membership. No operation below acquires
//! the registry, schedules, allocates or accesses caller memory while locked.
//! Returned raw pointers do not retain an object reference: callers must keep
//! the process registry locked or otherwise own the process/thread lifetime.

use crate::ps::process::Eprocess;
use crate::ps::thread::Ethread;

static MEMBERSHIP: crate::ke::sync::Spinlock<()> = crate::ke::sync::Spinlock::new(());

/// # Safety
/// Both allocations must be initialized and stable through the call. The
/// thread must belong to this process and must not be concurrently destroyed.
pub unsafe fn attach(process: *mut Eprocess, thread: *mut Ethread) -> bool {
    if process.is_null() || thread.is_null() {
        return false;
    }
    let _guard = MEMBERSHIP.lock();
    // SAFETY: the caller retains both allocations; all membership writers
    // hold MEMBERSHIP. The node is initialized before publication.
    unsafe {
        if (*thread).threads_process != process || (*thread).kthread.process != process {
            return false;
        }
        let entry = core::ptr::addr_of_mut!((*thread).thread_list_entry);
        let empty = ((*entry).flink.is_null() && (*entry).blink.is_null())
            || ((*entry).flink == entry && (*entry).blink == entry);
        if !empty {
            return false;
        }
        let head = core::ptr::addr_of_mut!((*process).kprocess_thread_list_head);
        if (*head).flink.is_null() && (*head).blink.is_null() {
            (*head).init();
        }
        (*head).insert_tail(entry);
    }
    true
}

/// # Safety
/// The initialized thread and its owning process must remain allocated through
/// the call. A linked node must have been published through `attach`.
pub unsafe fn detach(thread: *mut Ethread) {
    if thread.is_null() {
        return;
    }
    let _guard = MEMBERSHIP.lock();
    // SAFETY: membership links are exclusively protected by MEMBERSHIP and
    // their owner allocations are retained by the caller.
    unsafe {
        let entry = core::ptr::addr_of_mut!((*thread).thread_list_entry);
        if !(*entry).flink.is_null() && !(*entry).blink.is_null() {
            (*(*entry).flink).blink = (*entry).blink;
            (*(*entry).blink).flink = (*entry).flink;
        }
        (*entry).init();
    }
}

/// # Safety
/// The caller must retain the process and all its listed thread allocations,
/// normally by holding the process registry lock. A returned pointer has no
/// lifetime beyond that ownership/lock and must not escape it without a pin.
pub unsafe fn find(process: *mut Eprocess, tid: u64) -> Option<*mut Ethread> {
    if process.is_null() {
        return None;
    }
    let _guard = MEMBERSHIP.lock();
    // SAFETY: membership traversal is serialized with attach/detach; callers
    // retain allocations. container-of uses the actual independent ETHREAD node.
    unsafe {
        let head = core::ptr::addr_of_mut!((*process).kprocess_thread_list_head);
        let mut entry = (*head).flink;
        while !entry.is_null() && entry != head {
            let thread = entry.cast::<u8>()
                .sub(core::mem::offset_of!(Ethread, thread_list_entry))
                .cast::<Ethread>();
            if (*thread).client_id.unique_thread == tid {
                return Some(thread);
            }
            entry = (*entry).flink;
        }
    }
    None
}

/// # Safety
/// The process and all linked thread allocations must remain live during the
/// call. This counts membership, including threads still awaiting readying.
pub unsafe fn count(process: *mut Eprocess) -> u32 {
    if process.is_null() {
        return 0;
    }
    let _guard = MEMBERSHIP.lock();
    // SAFETY: the caller retains the allocations; the lock stabilizes links.
    unsafe {
        let head = core::ptr::addr_of_mut!((*process).kprocess_thread_list_head);
        let mut entry = (*head).flink;
        let mut count = 0u32;
        while !entry.is_null() && entry != head {
            count += 1;
            entry = (*entry).flink;
        }
        count
    }
}

/// # Safety
/// Caller retains the process registry and every member allocation. Dispatcher
/// observes the request and removes members without freeing active stacks.
pub unsafe fn request_termination(process:*mut Eprocess) {
    let _guard=MEMBERSHIP.lock();
    unsafe {
        let head=core::ptr::addr_of_mut!((*process).kprocess_thread_list_head);
        let mut entry=(*head).flink;
        while !entry.is_null() && entry!=head {
            let thread=entry.cast::<u8>().sub(core::mem::offset_of!(Ethread,thread_list_entry)).cast::<Ethread>();
            (*thread).execution_request.terminate();
            entry=(*entry).flink;
        }
    }
}

/// # Safety
/// Registry ownership must retain the process and all members. Configuration is
/// applied through dispatcher APIs so ready queue priority indices stay valid.
pub unsafe fn configure(process:*mut Eprocess,priority:Option<i8>,affinity:Option<u64>) {
    let _guard=MEMBERSHIP.lock();
    unsafe{
        let head=core::ptr::addr_of_mut!((*process).kprocess_thread_list_head);
        let mut entry=(*head).flink;
        while !entry.is_null()&&entry!=head {
            let thread=entry.cast::<u8>().sub(core::mem::offset_of!(Ethread,thread_list_entry)).cast::<Ethread>();
            if let Some(p)=priority{crate::ke::scheduler::set_priority(thread,p);}
            if let Some(mask)=affinity{crate::ke::scheduler::set_thread_affinity(thread,mask);}
            entry=(*entry).flink;
        }
    }
}
