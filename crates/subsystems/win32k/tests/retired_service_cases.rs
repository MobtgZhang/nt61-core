//! Observable retained-service contracts from original Win7 SP1 instructions.
use super::*;
use retired_services::{nt_gdi_init, nt_gdi_init_spool, nt_user_wait_for_msg_and_event};

struct CallerTeb {
    words: Box<[u32; 28]>,
    previous: *mut u8,
}

impl CallerTeb {
    fn new(error: u32, writable: bool) -> Self {
        let mut words = Box::new([0u32; 28]);
        words[26] = error;
        let pointer = user_pointer(&mut *words, writable);
        let thread = ps::thread::get_current_ethread();
        // SAFETY: the fixture has an exclusive per-host-thread ETHREAD.
        let previous = unsafe { core::mem::replace(&mut (*thread).kthread.teb, pointer as *mut u8) };
        Self { words, previous }
    }
    fn error(&self) -> u32 { self.words[26] }
}

impl Drop for CallerTeb {
    fn drop(&mut self) {
        // SAFETY: restore the current thread pointer before the allocation dies.
        unsafe { (*ps::thread::get_current_ethread()).kthread.teb = self.previous; }
    }
}

#[test]
fn retained_init_and_wait_preserve_last_error_and_ignore_unmapped_arguments() {
    let teb = CallerTeb::new(0xdeadbeef, true);
    // Addresses deliberately lack user-copy registrations: this Win7 entry
    // must not probe an event/message pointer or turn into a blocking wait.
    for argument in [0, 1, u32::MAX as u64, 1 << 32, 0x7fff_ffff_ffff, u64::MAX] {
        assert_eq!(call(nt_gdi_init, &[]), 1);
        assert_eq!(call(nt_user_wait_for_msg_and_event, &[argument]), 0);
        assert_eq!(teb.error(), 0xdeadbeef);
    }
}

#[test]
fn obsolete_spool_entry_sets_not_supported_in_only_the_callers_teb() {
    let handles: Vec<_> = [0, 5, 0xdeadbeef].into_iter().map(|before| {
        std::thread::spawn(move || {
            let teb = CallerTeb::new(before, true);
            assert_eq!(call(nt_gdi_init_spool, &[]), 0);
            assert_eq!(teb.error(), 50);
            assert_eq!(call(nt_gdi_init, &[]), 1);
            assert_eq!(call(nt_user_wait_for_msg_and_event, &[u64::MAX]), 0);
            assert_eq!(teb.error(), 50);
        })
    }).collect();
    for handle in handles { handle.join().unwrap(); }
}

#[test]
fn obsolete_spool_entry_handles_missing_or_unwritable_teb_without_kernel_fault() {
    let thread = ps::thread::get_current_ethread();
    // This host test starts without a TEB, matching a kernel caller.
    assert!(unsafe { (*thread).kthread.teb.is_null() });
    assert_eq!(call(nt_gdi_init_spool, &[]), 0);
    let teb = CallerTeb::new(0x12345678, false);
    assert_eq!(call(nt_gdi_init_spool, &[]), 0);
    assert_eq!(teb.error(), 0x12345678);
}

#[test]
fn retained_fullscreen_and_printer_entries_preserve_output_and_last_error() {
    use retired_services::{
        nt_gdi_fullscreen_control, nt_gdi_get_spool_message, nt_gdi_unload_printer_driver,
    };
    // Original whole-body results, verified by 192 unmodified x64 instruction
    // cases. Original WOW64 thunks establish argument counts 5, 4 and 2.
    let entries: [(extern "C" fn(&SyscallFrame) -> u64, usize, u64); 3] = [
        (nt_gdi_fullscreen_control, 5, 0xc0000001),
        (nt_gdi_get_spool_message, 4, 0),
        (nt_gdi_unload_printer_driver, 2, 0),
    ];
    let teb = CallerTeb::new(0xdeadbeef, true);
    let mut sentinel = [0x6a09e667u32; 32];
    let pointer = user_pointer(&mut sentinel, true);
    for (entry, count, expected) in entries {
        assert_eq!(call(entry, &[pointer; 5][..count]), expected);
        assert_eq!(sentinel, [0x6a09e667; 32]);
        assert_eq!(teb.error(), 0xdeadbeef);
        // These scalar patterns are not registered user buffers. The original
        // services never probe them, including Fullscreen's fifth argument.
        for argument in [0, 1, u32::MAX as u64, 1 << 32, 0x7fff_ffff_ffff, u64::MAX] {
            assert_eq!(call(entry, &[argument; 5][..count]), expected);
            assert_eq!(teb.error(), 0xdeadbeef);
        }
    }
}
