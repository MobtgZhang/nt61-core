//! NT 6.1 Device Driver Subsystem
//
//! The driver tree mirrors Windows' own PnP / bus layout. Each
//! bus driver is a singleton that walks the firmware topology
//! (PCI / ACPI / USB hubs) and exposes each populated device to
//! the I/O manager as a `DeviceObject`. Functional drivers
//! (storage, network, audio, video, input) bind to the device
//! objects through the standard PnP Start Device IRP.
//
//! Clean-room implementation. Spec source: Microsoft Docs / OSR /
//! "Windows Internals, 6th ed." (Russinovich). No code is copied
//! from any Microsoft binary, the WDK, or ReactOS.
//
//! # Module layout
//
//! ```text
//! drivers/mod.rs (this file) - top-level init / smoke_test aggregator
//! drivers/bus/               - PCI, ACPI, USB bus drivers + PnP manager
//! drivers/storage/           - ATA, ATAPI, AHCI, NVMe, SCSI
//! drivers/usb/               - UHCI, EHCI, xHCI, hub, HID
//! drivers/net/               - e1000, rtl8139, virtio-net
//! drivers/audio/             - Intel HDA, AC'97
//! drivers/input/             - i8042, USB HID keyboard / mouse
//! drivers/video/             - VGA, EFI framebuffer, Bochs VBE
//! drivers/timer/             - HPET, ACPI PM timer
//! drivers/ndis/              - NDIS 6.0 wrapper
//! drivers/wdf/               - KMDF host
//! ```
//
//! # Init order
//
//! The driver subsystem is brought up between Phase 4 (I/O
//! system) and Phase 5 (file systems) in `kernel_main`:
//
//! 1. `bus::init()`        - enumerate PCI, ACPI, USB root hubs
//! 2. `storage::init()`    - start ATA / AHCI / NVMe controllers
//! 3. `usb::init()`        - start host controllers + hub driver
//! 4. `net::init()`        - start NDIS-wrapped NICs
//! 5. `audio::init()`      - start HDA / AC'97 codecs
//! 6. `input::init()`      - start i8042 / USB HID
//! 7. `video::init()`      - start VGA / EfiFb / Bochs VBE
//! 8. `timer::init()`      - start HPET / ACPI PM timer
//
//!    `smoke_test()` runs every driver's post-init self-check.

// The drivers below carry the register/IOCTL surface of their
// real-world NT 6.1 counterparts; the unused_imports / dead_code
// lints fire on every register/IOCTL constant that is not yet
// exercised at runtime. Suppressing them here keeps the source
// readable: a faithful driver description naturally carries
// symbols whose wiring arrives with the hardware path.

extern crate alloc;

pub mod arch;

pub mod audio;
pub mod bus;
pub mod input;
pub mod ndis;
pub mod net;
pub mod storage;
pub mod timer;
pub mod usb;
pub mod video;
pub mod virtio;
pub mod wdf;

pub mod bootvid;
pub mod ci;
pub mod clfs;
pub mod fileinfo;
pub mod fltmgr;
pub mod ksec;
pub mod partmgr;
pub mod pcw;
pub mod platform;
pub mod spldr;
pub mod volmgr;
pub mod volsnap;

/// Initialise the entire driver subsystem. Calls each bus and
/// functional driver in the order Windows does. Idempotent —
/// every inner `init()` is a no-op if it has already run.
pub fn init() {
    use nt61_boot_types::boot_options::{active, BootOptions};
    let policy = active();
    // // kprintln!("  Initialising driver subsystem...");
    crate::hal::serial::write_string_always("[DRV] bus begin\r\n");
    bus::init();
    crate::hal::serial::write_string_always("[DRV] bus done\r\n");
    crate::hal::serial::write_string_always("[DRV] storage begin\r\n");
    storage::init();
    crate::hal::serial::write_string_always("[DRV] storage done\r\n");
    crate::hal::serial::write_string_always("[DRV] clfs begin\r\n");
    clfs::init();
    crate::hal::serial::write_string_always("[DRV] clfs done\r\n");
    crate::hal::serial::write_string_always("[DRV] usb begin\r\n");
    usb::init();
    crate::hal::serial::write_string_always("[DRV] usb done\r\n");
    crate::hal::serial::write_string_always("[DRV] net begin\r\n");
    if policy.network_enabled() { net::init(); }
    crate::hal::serial::write_string_always("[DRV] net done\r\n");
    crate::hal::serial::write_string_always("[DRV] audio begin\r\n");
    if !policy.has(BootOptions::SAFE) { audio::init(); }
    crate::hal::serial::write_string_always("[DRV] audio done\r\n");
    crate::hal::serial::write_string_always("[DRV] input begin\r\n");
    input::init();
    crate::hal::serial::write_string_always("[DRV] input done\r\n");
    crate::hal::serial::write_string_always("[DRV] video begin\r\n");
    if !policy.has(BootOptions::LOW_RES) && !policy.has(BootOptions::SAFE) { video::init(); }
    crate::hal::serial::write_string_always("[DRV] video done\r\n");
    // `video::init()` brings up the BOCHS VBE driver (x86_64)
    // which exposes a 1024×768×32 linear framebuffer at the
    // well-known aperture 0xE0_0000_0000. On every architecture
    // bootvid is wired to the GOP LFB by
    // `adopt_bootinfo_framebuffer()` in `arch::boot`. If GOP is
    // missing we fall back to the BOCHS aperture on x86_64 by
    // calling `bootvid::init_from_framebuffer` here so the Safe-Mode
    // shell still has a panel to paint on. `init_from_framebuffer`
    // is a no-op if the LFB is already wired, so it is harmless
    // when `adopt_bootinfo_framebuffer` already ran.
    if crate::hal::common::framebuffer::is_active() {
        let info = crate::hal::common::framebuffer::info();
        bootvid::init_from_framebuffer(info.address, info.width, info.height, info.pitch);
    }
    arch::init_machine_specific_drivers();

    // Force the bootvid LFB path on for non-x86_64 builds so the
    // bootvid console paints into the LFB instead of falling
    // through to the no-op (or worse, the legacy VGA buffer).
    bootvid::force_lfb_console();
    timer::init();
    if policy.network_enabled() { ndis::init(); }
    wdf::init();
    // bootvid now has a cross-arch LFB backend driven by the
    // shared `hal::common::framebuffer` writer; the legacy VGA
    // text buffer at 0xB8000 is still wired on x86_64 as a
    // fallback for boards that don't expose a GOP framebuffer.
    // On the other architectures `bootvid::init()` activates the
    // LFB path or stays disabled if no framebuffer was published.
    bootvid::init();
    // ci::init();  // DISABLED: code-integrity (CI) policy engine not yet wired
    // ksec::init();  // DISABLED: kernel security subsystem — depends on full seaccess init
    pcw::init();
    // partmgr::init();  // DISABLED: depends on storage I/O being live; re-enable after disk bring-up
    // volmgr::init();  // DISABLED: depends on partmgr
    // fltmgr::init();  // DISABLED: minifilter framework — large surface, leave for later
    // fileinfo::init();  // DISABLED: depends on fltmgr
    // spldr needs a DriverObject; we use the no-driver
    // variant for the bootstrap so the smoke test can still
    // verify the entry.
    spldr::init_no_driver();
}

/// Aggregator: runs every driver's smoke test and returns `true`
/// iff every one passes.
pub fn smoke_test() -> bool {
    arch::smoke_test()
}

// Staged top-level driver modules: they compile but have no callers
// outside this declaration list (no drivers::acpi / keyboard / ... /
// tcpip references exist in the tree). The live implementations live
// elsewhere (arch/x86_64, net/, input/); these files are kept until
// the staged rewrites either replace those or are deleted, so treat
// them as reference code, not as part of the driver surface.
pub mod acpi;
pub mod keyboard;
pub mod mouse;
pub mod ntfs;
pub mod pci;
pub mod serial;
pub mod tcpip;
