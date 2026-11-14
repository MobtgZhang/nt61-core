//! Firmware framebuffer adoption (UEFI GOP / `-device ramfb` / BOCHS).

use crate::boot_types::BootInfo;

/// Adopt the GOP / firmware-provided framebuffer that `winload`
/// published in `BootInfo`. The pixel writer is the cross-arch
/// `hal::common::framebuffer` module which is the same code on
/// every architecture; the bootvid subsystem reads through the
/// same atomics regardless of target. Returns `true` if a
/// framebuffer was brought up successfully so the caller can log
/// a status line without reaching into `FramebufferInfo` itself.
pub fn adopt_bootinfo_framebuffer(boot: &BootInfo) -> bool {
    if boot.framebuffer_base == 0 || boot.framebuffer_width == 0 {
        return false;
    }
    // Some GOP backends report a size larger than the visible linear
    // framebuffer. Derive the real scan-out size so early page-table
    // resources are not consumed by mapping irrelevant address space.
    let stride_bytes = boot.framebuffer_stride as u64;
    let derived_size = stride_bytes.saturating_mul(boot.framebuffer_height as u64);
    let derived_size = derived_size.min(64 * 1024 * 1024);
    let mapped_size = if derived_size != 0 {
        derived_size.min(boot.framebuffer_size.max(4096))
    } else {
        boot.framebuffer_size.min(64 * 1024 * 1024)
    }
    .max(4096);
    // Map the exact pixel address, including the GOP's offset within
    // its first physical page, into the kernel's address space.
    let framebuffer_va = map_framebuffer_region(boot.framebuffer_base, mapped_size);

    // Try the cross-arch LFB writer first. This is the source of
    // truth on every non-x86_64 target; on x86_64 it is mirrored
    // by `hal::x86_64::framebuffer::init_from_bootinfo` (called by
    // the legacy fallback path) so both writers stay in sync.
    let common_init = crate::hal::common::framebuffer::init_from_bootinfo(
        framebuffer_va,
        boot.framebuffer_width,
        boot.framebuffer_height,
        boot.framebuffer_stride,
        boot.framebuffer_format,
    );
    crate::boot_debug_println!("[FB3] framebuffer initialized");
    if common_init.is_some() {
        crate::hal::common::framebuffer::set_attr(0x07);
    }

    #[cfg(target_arch = "x86_64")]
    {
        // The x86_64 path additionally needs to mirror the same
        // info into the per-arch atomics so the legacy VGA-text
        // fallback stays consistent. We call the per-arch helper
        // last so the cross-arch writer wins for the LFB path.
        let arch_init = crate::hal::x86_64::framebuffer::init_from_bootinfo(
            framebuffer_va,
            boot.framebuffer_width,
            boot.framebuffer_height,
            boot.framebuffer_stride,
            boot.framebuffer_format,
        );
        let _ = arch_init;
        crate::boot_debug_println!("[FB4] x86 framebuffer mirror initialized");
    }

    // Wire bootvid onto the LFB so every `boot_println!` /
    // `gui_log` paint routes to the panel QEMU `-display gtk`
    // actually scans out. `init_from_framebuffer` is idempotent.
    crate::drivers::bootvid::init_from_framebuffer(
        framebuffer_va,
        boot.framebuffer_width,
        boot.framebuffer_height,
        boot.framebuffer_stride,
    );
    crate::boot_debug_println!("[FB5] bootvid attached");
    crate::drivers::bootvid::init();
    crate::boot_debug_println!("[FB6] bootvid initialized");
    true
}

/// Map the LFB region into the kernel page tables. GOP pixel memory
/// need not begin at a page boundary (OVMF VirtIO uses an offset of
/// 24 bytes). Page-table entries map aligned pages, whereas callers
/// must receive the address of the first pixel within that mapping.
///
/// A zero `size` is treated as "guess based on dimensions" (8 MiB
/// is enough for any sane 32 bpp panel up to ~1080p).
fn map_framebuffer_region(pa: u64, size: u64) -> u64 {
    let size = if size == 0 { 8 * 1024 * 1024 } else { size };
    #[cfg(target_arch = "x86_64")]
    {
        let page_offset = pa & 0xfff;
        let page_base = pa - page_offset;
        let page_count = (size + page_offset).div_ceil(0x1000);
        crate::boot_debug_println!("[FBR1] mapping framebuffer");
        let mapped = crate::mm::syspte::map_io_space(page_base, page_count)
            .map(|base| base + page_offset);
        crate::boot_debug_println!("[FBR2] framebuffer mapped");
        mapped.unwrap_or(pa) // Identity fallback remains valid only for firmware-exposed low addresses.
    }
    #[cfg(target_arch = "aarch64")]
    {
        let _ = crate::arch::aarch64::paging::identity_map_region(pa, size);
    }
    #[cfg(target_arch = "riscv64")]
    {
        let _ = crate::arch::riscv64::paging::identity_map_region(pa, size);
    }
    #[cfg(target_arch = "loongarch64")]
    {
        let _ = crate::arch::loongarch64::paging::identity_map_region(pa, size);
    }
    #[cfg(not(target_arch = "x86_64"))]
    return pa;
}
