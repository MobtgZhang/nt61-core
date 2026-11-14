//! Dedicated Windows 7 boot-animation system thread.
//!
//! The animation used to be advanced from the clock ISR, which meant a
//! full frame of per-pixel blending ran at `CLOCK_LEVEL` and starved the
//! rest of the kernel for several milliseconds per frame.  The sequence
//! is now owned by a real scheduler thread: `poll_boot_animation()` runs
//! at `PASSIVE_LEVEL`, the timer ISR only records the reschedule, and the
//! post-animation mirror loop keeps the virtio-gpu scanout in sync with
//! whatever the desktop shell paints into the LFB.

use crate::ke::sync::{wait_single, DispatcherHeader, Event, EventType, Spinlock};
use core::sync::atomic::{AtomicBool, Ordering};

static ANIMATION_TICK_STORAGE: Spinlock<Option<Event>> = Spinlock::new(None);
static MIRROR_TICK_STORAGE: Spinlock<Option<Event>> = Spinlock::new(None);
static THREAD_STARTED: AtomicBool = AtomicBool::new(false);

fn animation_tick() -> &'static DispatcherHeader {
    let mut guard = ANIMATION_TICK_STORAGE.lock();
    if guard.is_none() {
        *guard = Some(Event::new(EventType::Synchronization));
    }
    let header_pointer = guard
        .as_ref()
        .map(|event| &event.header as *const DispatcherHeader);
    drop(guard);
    // SAFETY: the event lives in a static Spinlock storage slot that is
    // never freed, so the pointer stays valid for the process lifetime.
    unsafe { &*header_pointer.expect("animation tick event initialised") }
}

fn mirror_tick() -> &'static DispatcherHeader {
    let mut guard = MIRROR_TICK_STORAGE.lock();
    if guard.is_none() {
        *guard = Some(Event::new(EventType::Synchronization));
    }
    let header_pointer = guard
        .as_ref()
        .map(|event| &event.header as *const DispatcherHeader);
    drop(guard);
    // SAFETY: same static storage rule as `animation_tick`.
    unsafe { &*header_pointer.expect("mirror tick event initialised") }
}

extern "C" fn boot_animation_thread_entry(_context: u64) {
    // Animation phase: paint one frame per tick until the desktop hands
    // the display over (or the sequence completes in CMD mode).
    loop {
        let _ = wait_single(animation_tick(), 15);
        crate::arch::boot::poll_boot_animation();
        if crate::arch::boot::is_boot_animation_finished() {
            break;
        }
    }

    display_thread_entry(0);
}

extern "C" fn display_thread_entry(_context: u64) {
    // Mirror phase: the Ring-3 shell now paints straight into the LFB.
    // When the virtio-gpu scanout is alive it can no longer see those
    // writes directly, so copy damaged rectangles into the persistent GPU
    // resource every 16 ms. An unchanged desktop performs no transfer.
    loop {
        let _ = wait_single(mirror_tick(), 16);
        crate::drivers::bootvid::present_desktop_frame();
    }
}

/// Basic framebuffer presentation for Safe Mode and Debug CMD. No logo or
/// animation is started, but the software framebuffer must reach the GPU.
pub fn start_basic_display_thread() {
    if THREAD_STARTED.swap(true, Ordering::AcqRel) {
        return;
    }
    crate::arch::boot::skip_boot_animation();
    crate::drivers::bootvid::initialize_basic_display();
    mirror_tick();
    let result =
        crate::ps::thread::ps_create_system_thread(display_thread_entry as *const () as u64, 0);
    if !result.success {
        THREAD_STARTED.store(false, Ordering::Release);
    }
}

/// Start the boot-animation system thread.  Idempotent: the timer-ISR
/// fallback in `poll_services()` still works if the scheduler is not
/// ready yet during early boot.
pub fn start_animation_thread() {
    if THREAD_STARTED.swap(true, Ordering::AcqRel) {
        return;
    }
    animation_tick();
    mirror_tick();
    crate::arch::boot::run_boot_animation();
    let entry = boot_animation_thread_entry as *const () as u64;
    let result = crate::ps::thread::ps_create_system_thread(entry, 0);
    if !result.success {
        THREAD_STARTED.store(false, Ordering::Release);
        crate::hal::serial::write_string_always("[ANIM] system thread create failed\r\n");
    }
    crate::hal::serial::write_string_always("[ANIM] system thread attach\r\n");
}
