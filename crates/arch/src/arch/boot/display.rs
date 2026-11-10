//! Boot-time display-mode selection, IRQ masking and boot animation.

use crate::boot_types::BootMode;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

static BOOT_ANIMATION_RUNNING: AtomicBool = AtomicBool::new(false);
static BOOT_ANIMATION_TARGET_READY: AtomicBool = AtomicBool::new(false);
static BOOT_ANIMATION_SEQUENCE_DONE: AtomicBool = AtomicBool::new(false);
static BOOT_ANIMATION_FINISHED: AtomicBool = AtomicBool::new(false);
static BOOT_ANIMATION_STARTED_MS: AtomicU64 = AtomicU64::new(0);
static BOOT_ANIMATION_LAST_FRAME_MS: AtomicU64 = AtomicU64::new(0);
static BOOT_ANIMATION_ANIMATION_MS: AtomicU64 = AtomicU64::new(0);
static BOOT_ANIMATION_PAINTING: AtomicBool = AtomicBool::new(false);
static DEBUG_POLL: AtomicU64 = AtomicU64::new(0);

const WIN7_ANIMATION_MS: u64 = 7_200;
const WIN7_FRAME_INTERVAL_MS: u64 = 30;

/// Bring up the platform's text console (VGA mirror on x86_64,
/// log-ring on the other architectures). Idempotent across
/// re-entry.
pub fn init_text_console() {
    #[cfg(target_arch = "x86_64")]
    {
        // gui_log is the VGA mirror; it's only defined on x86_64.
        crate::hal::x86_64::text_console::init();
    }
    // The unified log-ring lives in hal::common; harmless to call
    // on every arch.
    crate::hal::common::text_console::init();
}
/// Initialise the platform serial port. Idempotent.
pub fn init_serial() {
    crate::hal::serial::init();
}

/// Mask every legacy 8259 IRQ line. On x86_64 this writes the
/// OCW1 mask to the master/slave pair; on every other
/// architecture the legacy PIC is absent and the per-arch
/// interrupt controller is handled inside `arch::init_hardware()`,
/// so this is a no-op.
pub fn mask_legacy_pic_irqs() {
    #[cfg(target_arch = "x86_64")]
    crate::hal::x86_64::pic::init_and_mask_all();
}

/// Belt-and-braces mask used by the Safe-Mode polling shell:
/// every maskable IRQ is dropped at the controller so the
/// PIT/IRQ keyboard cannot fire while the shell owns the CPU.
///
/// On x86_64 we mask the legacy 8259 with the OCW1 mask
/// `0xFF/0xFF` (all 16 lines). The other architectures mask at
/// the platform controller inside `arch::init_hardware()` already;
/// we still call into the per-arch helper to leave room for
/// future per-arch masks.
pub fn mask_all_irqs_for_polled_io() {
    #[cfg(target_arch = "x86_64")]
    {
        crate::hal::x86_64::pic::write_mask(0xFFFF);
    }
    // The other architectures have already masked their
    // respective controllers in `arch::init_hardware()`, so no
    // additional action is needed here.
}

/// Configure the boot-time display mode (logo, SOS, ...).
///
/// `Desktop` / `Cmd`  -> Windows 7 logo + animation, no log output.
/// `DebugCmd`         -> SOS mode (verbose log) + ntbtlog.
/// `SafeMode`         -> SOS mode, distinct Safe Mode banner.
pub fn configure_boot_display(mode: BootMode) {
    use crate::drivers::bootvid::{set_boot_mode, BootDisplayMode};

    // Gate the boot log on the selected mode. Desktop and Cmd modes
    // show only the animation; DebugCmd and SafeMode show the full
    // verbose boot trace.
    crate::rtl::klog::set_boot_log_visible(!mode.suppress_boot_log());

    match mode {
        BootMode::Desktop | BootMode::Cmd => {
            set_boot_mode(BootDisplayMode::Normal);
            crate::drivers::bootvid::paint_boot_screen(0);
        }
        BootMode::DebugCmd => {
            set_boot_mode(BootDisplayMode::Sos);
            crate::rtl::ntbtlog::enable_boot_log();
            crate::rtl::ntbtlog::begin_boot_sequence();
            crate::rtl::ntbtlog::log_ntoskrnl();
        }
        BootMode::SafeMode => {
            set_boot_mode(BootDisplayMode::Sos);
            crate::rtl::ntbtlog::enable_boot_log();
            crate::rtl::ntbtlog::begin_boot_sequence();
            crate::rtl::ntbtlog::log_ntoskrnl();
            crate::boot_println!("Starting in Safe Mode");
        }
    }
}

/// Run the Windows 7 boot animation for Desktop / Cmd modes.
///
/// Animates the Windows 7 "Starting Windows" sequence from 0% to 100%
/// over ~7.2 seconds, then releases the boot display.
pub fn run_boot_animation() {
    if BOOT_ANIMATION_FINISHED.load(Ordering::Acquire) {
        crate::hal::serial::write_string_always("[ANIM] run skip finished\r\n");
        return;
    }
    if BOOT_ANIMATION_RUNNING.swap(true, Ordering::AcqRel) {
        crate::hal::serial::write_string_always("[ANIM] run skip already running\r\n");
        return;
    }
    crate::hal::serial::write_string_always("[ANIM] run start\r\n");

    BOOT_ANIMATION_TARGET_READY.store(false, Ordering::Release);
    BOOT_ANIMATION_SEQUENCE_DONE.store(false, Ordering::Release);
    BOOT_ANIMATION_FINISHED.store(false, Ordering::Release);
    BOOT_ANIMATION_STARTED_MS.store(boot_now_ms(), Ordering::Release);
    BOOT_ANIMATION_LAST_FRAME_MS.store(0, Ordering::Release);
    BOOT_ANIMATION_ANIMATION_MS.store(0, Ordering::Release);
    crate::drivers::bootvid::set_gui_console_active(true);
    crate::hal::serial::write_string_always("[ANIM] gui active\r\n");
    crate::drivers::bootvid::vid_clear_black();
    crate::hal::serial::write_string_always("[ANIM] clear done\r\n");
    crate::drivers::bootvid::paint_boot_screen_time(0);
    crate::hal::serial::write_string_always("[ANIM] first paint done\r\n");
    BOOT_ANIMATION_LAST_FRAME_MS.store(
        BOOT_ANIMATION_STARTED_MS.load(Ordering::Acquire),
        Ordering::Release,
    );

    // The dedicated animation system thread advances the sequence while
    // the boot thread keeps loading the desktop, so the first frame
    // becomes visible as soon as the kernel owns the framebuffer.
}

/// Whether the early boot animation still owns the boot display.
pub fn is_boot_animation_running() -> bool {
    BOOT_ANIMATION_RUNNING.load(Ordering::Acquire)
}

/// Whether the early boot animation has fully completed or handed off.
pub fn is_boot_animation_finished() -> bool {
    BOOT_ANIMATION_FINISHED.load(Ordering::Acquire)
}

/// Signal that the Ring-3 shell resources have been loaded. The
/// animation thread finishes its current frame, clears the screen, and
/// exits so the shell can take ownership of the framebuffer.
pub fn mark_boot_animation_complete() {
    BOOT_ANIMATION_TARGET_READY.store(true, Ordering::Release);
}

/// Block until the full Windows 7 sequence has played. CMD mode uses
/// this because it has no desktop-ready handoff and must not enter the
/// text console while the animation thread still owns the screen.
pub fn wait_boot_animation_finished() {
    while !BOOT_ANIMATION_SEQUENCE_DONE.load(Ordering::Acquire) {
        poll_boot_animation();
    }
}

/// Advance the boot animation from the clock-driven service poll.
///
/// The startup path is not always a scheduler thread, and the AP
/// dispatch request can be missed under TCG. Driving the renderer from
/// the timer poll keeps the sequence visible while SMSS loads the
/// desktop shell without depending on IPI delivery.
pub fn poll_boot_animation() {
    let cpu = crate::ke::scheduler::get_current_cpu();
    crate::hal::serial::write_string_always("[ANIM] poll enter cpu=");
    crate::hal::serial::write_hex_u64_always(cpu as u64);
    crate::hal::serial::write_string_always("\r\n");
    if cpu != 0 {
        crate::drivers::bootvid::poll_animation_strip(cpu as u32);
        return;
    }
    if !BOOT_ANIMATION_RUNNING.load(Ordering::Acquire)
        || BOOT_ANIMATION_PAINTING
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
    {
        return;
    }

    let started_ms = BOOT_ANIMATION_STARTED_MS.load(Ordering::Acquire);
    let now_ms = boot_now_ms();
    let elapsed_ms = now_ms.saturating_sub(started_ms);
    let animation_ms = elapsed_ms.min(WIN7_ANIMATION_MS);
    crate::hal::serial::write_string_always("[ANIM] poll state started=");
    crate::hal::serial::write_hex_u64_always(started_ms);
    crate::hal::serial::write_string_always(" now=");
    crate::hal::serial::write_hex_u64_always(now_ms);
    crate::hal::serial::write_string_always(" animation=");
    crate::hal::serial::write_hex_u64_always(animation_ms);
    crate::hal::serial::write_string_always(" done=");
    crate::hal::serial::write_hex_u64_always(
        BOOT_ANIMATION_SEQUENCE_DONE.load(Ordering::Acquire) as u64
    );
    crate::hal::serial::write_string_always("\r\n");
    BOOT_ANIMATION_ANIMATION_MS.store(animation_ms, Ordering::Release);
    let last_frame_ms = BOOT_ANIMATION_LAST_FRAME_MS.load(Ordering::Acquire);
    let _ = (DEBUG_POLL.fetch_add(1, Ordering::AcqRel), last_frame_ms);

    if animation_ms >= WIN7_ANIMATION_MS {
        if !BOOT_ANIMATION_SEQUENCE_DONE.load(Ordering::Acquire) {
            BOOT_ANIMATION_SEQUENCE_DONE.store(true, Ordering::Release);
            BOOT_ANIMATION_FINISHED.store(true, Ordering::Release);
            BOOT_ANIMATION_RUNNING.store(false, Ordering::Release);
            crate::ke::scheduler::set_early_ap_polling(false);
        } else if now_ms.saturating_sub(last_frame_ms) >= WIN7_FRAME_INTERVAL_MS {
            let next_animation_ms = animation_ms.saturating_add(WIN7_FRAME_INTERVAL_MS);
            BOOT_ANIMATION_LAST_FRAME_MS.store(now_ms, Ordering::Release);
            BOOT_ANIMATION_ANIMATION_MS.store(next_animation_ms, Ordering::Release);
            crate::drivers::bootvid::paint_boot_screen_time(
                next_animation_ms.min(u32::MAX as u64) as u32
            );
        }
    } else if !BOOT_ANIMATION_SEQUENCE_DONE.load(Ordering::Acquire)
        && now_ms.saturating_sub(last_frame_ms) >= WIN7_FRAME_INTERVAL_MS
    {
        BOOT_ANIMATION_LAST_FRAME_MS.store(now_ms, Ordering::Release);
        let next_animation_ms = animation_ms.saturating_add(WIN7_FRAME_INTERVAL_MS);
        BOOT_ANIMATION_ANIMATION_MS.store(next_animation_ms, Ordering::Release);
        crate::drivers::bootvid::paint_boot_screen_time(next_animation_ms as u32);
    }

    if BOOT_ANIMATION_TARGET_READY.load(Ordering::Acquire) {
        BOOT_ANIMATION_FINISHED.store(true, Ordering::Release);
        BOOT_ANIMATION_RUNNING.store(false, Ordering::Release);
        crate::ke::scheduler::set_early_ap_polling(false);
    }

    BOOT_ANIMATION_PAINTING.store(false, Ordering::Release);
}

fn boot_now_ms() -> u64 {
    #[cfg(target_arch = "x86_64")]
    {
        crate::hal::x86_64::pit::get_system_time_ms()
    }

    #[cfg(target_arch = "aarch64")]
    {
        crate::hal::aarch64::timer::time_ms()
    }

    #[cfg(target_arch = "riscv64")]
    {
        use crate::arch::riscv64::pit::{time, TIMER_TICKS_PER_MS};
        time() / TIMER_TICKS_PER_MS
    }

    #[cfg(target_arch = "loongarch64")]
    {
        let counter: u64;
        unsafe { core::arch::asm!("csrrd {}, 0x20", out(reg) counter) };
        counter / 100_000
    }
}
