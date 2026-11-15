//! Hosted tools must be able to use kernel data locks without privileged IRQ
//! instructions. Ring-0 hardware mask behavior is covered by UEFI execution.
#[test]
fn hosted_spinlock_does_not_mask_host_os_interrupts() {
    assert!(!ntoskrnl::arch::interrupts_enabled());
    let lock = ntoskrnl::ke::sync::Spinlock::new(42u64);
    *lock.lock() += 1;
    assert_eq!(*lock.lock(), 43);
}
