use super::{irp_stack, IoStackLocation, Irp, IRP_CANCEL_INDICATED};

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// Invoke the completion routines registered on the IRP's stack locations,
/// walking the driver stack from the lowest slot upward the way NT does as
/// an IRP unwinds. Each routine is taken out of the stack location before
/// it is called so a routine that completes the IRP itself cannot be run
/// again. Returns false only when a routine returned
/// STATUS_MORE_PROCESSING_REQUIRED and now owns the IRP (it must complete
/// it later); the IRP must not be freed in that case.
pub(super) unsafe fn invoke_completion_routines(irp: *mut Irp) -> bool {
    let count = (*irp).stack_count as usize;
    let status = (*irp).io_status.status as i32;
    let cancelled = ((*irp).flags & IRP_CANCEL_INDICATED) != 0;
    let base = (irp as *mut u8).add(core::mem::size_of::<Irp>()) as *mut IoStackLocation;

    for i in 0..count {
        let stack = base.add(i);
        // Take the routine out first: re-entrant completion from inside
        // the routine must not invoke it a second time.
        let routine = (*stack).completion_routine.take();
        let Some(routine) = routine else {
            continue;
        };
        let control = (*stack).control;
        let invoke = if cancelled {
            (control & irp_stack::SL_INVOKE_ON_CANCEL) != 0
        } else if status >= 0 {
            (control & irp_stack::SL_INVOKE_ON_SUCCESS) != 0
        } else {
            (control & irp_stack::SL_INVOKE_ON_ERROR) != 0
        };
        if !invoke {
            continue;
        }
        let device = (*stack).device_object;
        let context = (*stack).completion_context;
        let result = routine(device, irp, context);
        if result == crate::libs::ntdll::status::STATUS_MORE_PROCESSING_REQUIRED {
            return false;
        }
    }
    true
}
