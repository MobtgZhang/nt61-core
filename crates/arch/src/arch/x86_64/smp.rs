//! SMP boot
//
//! Bringing up an application processor (AP) on x86_64 is the
//! standard sequence: allocate a 4 KiB page, copy a tiny
//! trampoline there, send an INIT IPI, wait 10 ms, send two
//! SIPIs back-to-back, and the AP will then jump into the
//! trampoline. The trampoline is a 16-bit real-mode blob
//! followed by a 64-bit long-mode entry that sets up its own
//! GDT/IDT/TSS and parks itself in the scheduler.
//
//! CPU enumeration uses the ACPI MADT (see
//! `hal::common::acpi::madt_info`).

use core::arch::asm;
use core::sync::atomic::{AtomicU32, Ordering};

#[cfg(target_arch = "x86_64")]
use crate::arch::x86_64::cpu_flags;
#[cfg(target_arch = "x86_64")]
#[cfg(target_arch = "x86_64")]
use crate::arch::x86_64::gdt;
#[cfg(target_arch = "x86_64")]
use crate::arch::x86_64::tss;
use crate::hal::common::acpi::{madt_info, parse_madt};
#[cfg(target_arch = "x86_64")]
#[cfg(target_arch = "x86_64")]
use crate::hal::x86_64::apic::{apic_read, apic_write, lapic_reg};
#[cfg(target_arch = "x86_64")]
use crate::mm::frame;

static CPU_COUNT: AtomicU32 = AtomicU32::new(1);
/// Self-contained 16-bit -> 32-bit -> 64-bit AP startup blob.
///
/// The blob is position-independent and is written to the fixed,
/// conventional physical page TRAMPOLINE_PHYS (0x70000) so the SIPI
/// vector is a constant (0x70). A fixed address also lets the 64-bit
/// tail use plain rip-relative loads and a runtime-patched `call`.
///
/// Parameter block (physical, identity-mapped):
///   0x500 null GDT entry
///   0x508 32-bit CS  (selector 0x08)
///   0x510 data       (selector 0x10)
///   0x518 64-bit CS  (selector 0x18)
///   0x520 GDTR (16-bit limit 31, 32-bit base 0x500)
///   0x528 u32 kernel CR3 (PML4 physical address)
///   Trampoline page offsets (physical 0x70530+), read by the blob in
///   64-bit mode via rip-relative loads:
///   0x530 u64 per-AP stack top   (re-written before each SIPI)
///   0x538 u64 per-AP 1-based index (re-written before each SIPI)
///   0x540 u64 absolute &ap_entry_long
///   0x548 u32 arrival handshake (AP writes its 1-based index here)
#[rustfmt::skip]
/// AP startup blob stage 1: 16-bit real mode at 0x70000. Loads the
/// parameter GDT (GDTR at 0x520), enables protected mode, and
/// far-jumps to stage 2 at linear 0x70100.
#[rustfmt::skip]
const BLOB_STAGE1: [u8; 0x18] = [
    0xFA, 0x0F, 0x01, 0x16, 0x20, 0x05, 0x0F, 0x20, 0xC0, 0x66, 0x83, 0xC8,
    0x01, 0x0F, 0x22, 0xC0, 0x66, 0xEA, 0x00, 0x01, 0x07, 0x00, 0x08, 0x00,
];

/// AP startup blob stage 2: 32-bit protected mode at linear
/// 0x70100 (paging off, linear == physical). Enables PAE + long
/// mode (EFER.LME), turns on paging while preserving PE, then
/// far-jumps to stage 3 at 0x70200.
#[rustfmt::skip]
const BLOB_STAGE2: [u8; 0x3B] = [
    0x0F, 0x20, 0xE0, 0x66, 0x83, 0xC8, 0x20, 0x0F, 0x22, 0xE0, 0x66, 0xB8,
    0x10, 0x00, 0x8E, 0xD8, 0x8E, 0xC0, 0x8E, 0xD0, 0xA1, 0x28, 0x05, 0x00,
    0x00, 0x0F, 0x22, 0xD8, 0xB9, 0x80, 0x00, 0x00, 0xC0, 0x0F, 0x32, 0x66,
    0x0D, 0x00, 0x05, 0x0F, 0x30, 0x0F, 0x20, 0xC0, 0x0D, 0x00, 0x00, 0x00,
    0x80, 0x0F, 0x22, 0xC0, 0xEA, 0x00, 0x02, 0x07, 0x00, 0x18, 0x00,
];

/// AP startup blob stage 3: 64-bit long mode at linear 0x70200
/// with the kernel CR3 active (low memory is identity-mapped).
/// Loads the per-AP stack (0x70530) and index (0x70538) via
/// rip-relative loads, then jumps through the absolute pointer at
/// 0x70540 into ap_entry_long.
#[rustfmt::skip]
const BLOB_STAGE3: [u8; 0x24] = [
    0x66, 0xB8, 0x10, 0x00, 0x66, 0x8E, 0xD8, 0x66, 0x8E, 0xC0, 0x66, 0x8E,
    0xD0, 0x48, 0x8B, 0x25, 0x1C, 0x03, 0x00, 0x00, 0x48, 0x8B, 0x3D, 0x1D,
    0x03, 0x00, 0x00, 0xFF, 0x25, 0x1F, 0x03, 0x00, 0x00, 0xF4, 0xEB, 0xFE,
];

/// Copy the three blob stages into the trampoline page at their
/// fixed offsets: stage 1 -> 0x000, stage 2 -> 0x100, stage 3 -> 0x200.
/// The far jumps use full linear addresses (0x70100 / 0x70200), so
/// every copy site must install all three stages together.
fn install_blob_stages(code: *mut u8) {
    unsafe {
        core::ptr::copy_nonoverlapping(BLOB_STAGE1.as_ptr(), code, BLOB_STAGE1.len());
        core::ptr::copy_nonoverlapping(BLOB_STAGE2.as_ptr(), code.add(0x100), BLOB_STAGE2.len());
        core::ptr::copy_nonoverlapping(BLOB_STAGE3.as_ptr(), code.add(0x200), BLOB_STAGE3.len());
    }
}

/// Fixed low-memory trampoline page (4 KiB aligned, < 1 MiB so the
/// SIPI vector can address it) and the parameter block in the BIOS
/// data area. Both are identity-mapped into the kernel page table.
const TRAMPOLINE_PHYS: u64 = 0x0007_0000;
const PARAM_PHYS: u64 = 0x0000_0500;

// Per-CPU GDT storage for APs.
// Each AP accesses its own index during sequential boot initialization.
// The compiler may optimize assuming no aliasing, so we use unsafe
// blocks with explicit comments explaining why each access is safe:
// - During boot, only one CPU accesses its own AP_GDTS[ap_idx] slot
// - APs boot sequentially (BSP controls SIPI timing)
// - Each slot is written exactly once during initialization
static mut AP_GDTS: [gdt::PerCpuGdt; tss::MAX_APS] = [const {
    gdt::PerCpuGdt {
        e0: gdt::GdtEntry::empty(),
        e1: gdt::GdtEntry::kernel_code64(),
        e2: gdt::GdtEntry::kernel_data64(),
        e3: gdt::GdtEntry::empty(),      // slot 3: kernel SS (OVMF)
        e4: gdt::GdtEntry::empty(),      // slot 4: user SS — init() fills via OVMF GDT
        e5: gdt::GdtEntry::empty(),      // slot 5: user CS — init() fills via OVMF GDT
        e6: gdt::GdtEntry::empty(),      // slot 6: (unused)
        e7: gdt::GdtEntry::empty(),      // slot 7: (unused)
        e8: gdt::TssDescriptor::empty(), // slot 8: TSS
        e9: gdt::GdtEntry::empty(),      // slot 9: (TSS high)
        e_a: gdt::GdtEntry::empty(),     // slot 10: (unused)
        e_b: gdt::GdtEntry::empty(),     // slot 11: (unused)
    }
}; tss::MAX_APS];

/// Number of detected CPUs.
pub fn cpu_count() -> u32 {
    CPU_COUNT.load(Ordering::Relaxed)
}

/// Always-visible serial print of prefix + hex value + suffix. Uses
/// early_write_str, which is visible even in release builds where
/// boot_println! is compiled out.
fn smp_print_hex(prefix: &[u8], val: u64, suffix: &[u8]) {
    crate::arch::boot::early_write_str(prefix);
    let mut tmp = [0u8; 16];
    let mut tl = 0;
    if val == 0 {
        tmp[tl] = b'0';
        tl += 1;
    }
    let mut n = val;
    while n > 0 && tl < 16 {
        let d = (n & 0xF) as u8;
        tmp[tl] = if d < 10 { b'0' + d } else { b'a' + d - 10 };
        tl += 1;
        n >>= 4;
    }
    for i in 0..tl / 2 {
        tmp.swap(i, tl - 1 - i);
    }
    crate::arch::boot::early_write_str(&tmp[..tl]);
    crate::arch::boot::early_write_str(suffix);
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// BSP entry point. `pml4_pfn` is the kernel page table.
pub unsafe fn smp_boot_aps(pml4_pfn: u64) {
    crate::arch::boot::early_write_str(b"[SMP] enter\r\n");
    // Parse the MADT and discover how many LAPICs exist.
    if !parse_madt() {
        // // kprintln!("[smp] MADT not found, running as single-CPU");
        CPU_COUNT.store(1, Ordering::SeqCst);
        crate::arch::boot::early_write_str(b"[SMP] madt_missing\r\n");
        return;
    }
    crate::arch::boot::early_write_str(b"[SMP] madt_ok\r\n");
    let info = madt_info();
    // // kprintln!("[smp] MADT parsed:");
    // // kprintln!("  - LAPIC base: 0x{:x}", info.local_apic_address);
    // // kprintln!("  - LAPIC flags: 0x{:x}", info.flags);
    // // kprintln!("  - CPU count: {}", info.lapic_count);
    if info.has_io_apic {
        // // kprintln!("  - I/O APIC: id={} addr=0x{:x} gsi_base={}",
        // //                   info.io_apic_id, info.io_apic_address, info.io_apic_gsi_base);
    }
    // // kprintln!("  - Interrupt overrides: {}", info.int_override_count);
    // // kprintln!("  - NMI sources: {}", info.nmi_source_count);

    // Allocate scheduler PRCBs before any AP calls
    // `init_smp_this_cpu()`. The AP entry path cannot allocate its own
    // PRCB because it may run concurrently with the BSP.
    crate::ke::scheduler::init_smp(u32::from(info.lapic_count));
    crate::arch::boot::early_write_str(b"[SMP] prcb_ok\r\n");
    smp_print_hex(b"[SMP] lapic_count=", info.lapic_count as u64, b"\r\n");

    if info.lapic_count <= 1 {
        crate::arch::boot::early_write_str(b"[SMP] single-apic-exit\r\n");
        // // kprintln!("[smp] only BSP detected, running single-CPU");
        CPU_COUNT.store(1, Ordering::SeqCst);
        return;
    }
    // Identity-map the fixed trampoline page and the parameter
    // block page, then install the blob, GDT, GDTR and kernel CR3.
    // Identity-map (VA == PA) every page the AP touches before it can
    // map anything for itself. The AP runs with the kernel CR3 and has
    // no IDT, so a single missing mapping means an immediate page fault
    // -> triple fault -> reset loop that never reaches ap_entry_long.
    // syspte::map_io_space is NOT enough: it installs a mapping at a
    // SYSTEM_PTE VA, not at the physical address the trampoline and
    // blob access linearly.
    if !crate::arch::identity_map_region(TRAMPOLINE_PHYS, 0x1000) {
        crate::arch::boot::early_write_str(b"[SMP] map_trampoline_failed\r\n");
        CPU_COUNT.store(1, Ordering::SeqCst);
        return;
    }
    if !crate::arch::identity_map_region(PARAM_PHYS & !0xFFF, 0x1000) {
        crate::arch::boot::early_write_str(b"[SMP] map_param_failed\r\n");
        CPU_COUNT.store(1, Ordering::SeqCst);
        return;
    }
    // LAPIC MMIO: the AP enables its local APIC before it allocates
    // its per-CPU area, so the canonical 0xFEE00000 page must already
    // be identity-mapped in the kernel CR3.
    if !crate::arch::identity_map_region(0xFEE0_0000, 0x1000) {
        crate::arch::boot::early_write_str(b"[SMP] map_lapic_failed\r\n");
        CPU_COUNT.store(1, Ordering::SeqCst);
        return;
    }
    // Re-map and software-enable the BSP LAPIC in the *current* (final)
    // kernel page tables. `apic::init()` ran before the switch to the
    // final CR3; its SYSTEM_PTE mapping does not survive that switch,
    // leaving the LAPIC software-disabled so every INIT/SIPI the BSP
    // sends is silently dropped and the APs never wake up.
    crate::hal::x86_64::apic::init_smp();
    smp_print_hex(
        b"[smp] svr=0x",
        crate::hal::x86_64::apic::apic_read(crate::hal::x86_64::apic::lapic_reg::SVR) as u64,
        b"\r\n",
    );
    let ap_stack_phys = match frame::allocate_pages(1) {
        Some(p) => p,
        None => {
            crate::arch::boot::early_write_str(b"[SMP] ap_stack_alloc_failed\r\n");
            // // kprintln!("[smp] OOM allocating per-AP stack, single-CPU mode");
            CPU_COUNT.store(1, Ordering::SeqCst);
            return;
        }
    };
    // The AP stack top is at `ap_stack_phys + 4096` (stack grows
    // down on x86_64).
    let ap_stack_top = ap_stack_phys + 4096;
    // Identity-map the AP stack in the kernel page table.
    // FIX 2.6: Check for mapping failure
    if !crate::arch::identity_map_region(ap_stack_phys, 0x1000) {
        crate::arch::boot::early_write_str(b"[SMP] map_ap_stack_failed\r\n");
        // // kprintln!("[smp] failed to identity-map AP stack page, single-CPU mode");
        CPU_COUNT.store(1, Ordering::SeqCst);
        return;
    }
    // Install the trampoline blob, GDT, GDTR and kernel CR3 into
    // the identity-mapped low-memory pages.
    unsafe {
        let code = TRAMPOLINE_PHYS as *mut u8;
        let param = PARAM_PHYS as *mut u8;
        // Copy the position-independent blob.
        install_blob_stages(code);
        // Store the absolute 64-bit entry address at page offset 0x540;
        // the blob's `jmp [rip+disp32]` loads it. A rel32 call cannot
        // span from the low trampoline into the higher-half kernel.
        let ap_entry: unsafe extern "C" fn(u64) -> ! = ap_entry_long;
        core::ptr::write_volatile(code.add(0x540) as *mut u64, ap_entry as u64);
        // GDT: null / 32-bit CS / data / 64-bit CS.
        core::ptr::write_volatile(param.add(0x08) as *mut u64, 0x00CF_9A00_0000_FFFF);
        core::ptr::write_volatile(param.add(0x10) as *mut u64, 0x00CF_9200_0000_FFFF);
        core::ptr::write_volatile(param.add(0x18) as *mut u64, 0x00AF_9A00_0000_FFFF);
        // GDTR at 0x520: 16-bit limit + 32-bit base.
        core::ptr::write_volatile(param.add(0x20) as *mut u16, 31);
        core::ptr::write_volatile(param.add(0x22) as *mut u32, PARAM_PHYS as u32);
        // CR3 wants the page-table base address, not the PFN.
        core::ptr::write_volatile(
            param.add(0x28) as *mut u32,
            ((pml4_pfn & 0x000F_FFFF_FFFF) * 0x1000) as u32,
        );
        // Diagnostics: verify the blob landed and log the patched-in
        // entry pointer so a bad patch is visible in the serial log.
        smp_print_hex(b"[smp] entry=0x", ap_entry as u64, b" cr3=0x");
        smp_print_hex(
            b"",
            ((pml4_pfn & 0x000F_FFFF_FFFF) * 0x1000) as u64,
            b" stack=0x",
        );
        smp_print_hex(b"", ap_stack_top, b"\r\n");
    }
    // Bring up each AP.
    let mut brought_up: u32 = 1;
    for i in 1..info.lapic_count as usize {
        let apic_id = info.lapic_ids[i];
        // Patch the per-AP parameters in the trampoline page. The blob
        // reads them via rip-relative loads inside the trampoline page.
        unsafe {
            let code = TRAMPOLINE_PHYS as *mut u8;
            // Re-install the full blob right before every SIPI. The
            // trampoline page lives in low conventional memory that
            // earlier boots have seen scribbled between the initial
            // copy and the AP wake-up; re-copying is cheap and makes
            // the AP entry path self-healing.
            install_blob_stages(code);
            let ap_entry: unsafe extern "C" fn(u64) -> ! = ap_entry_long;
            core::ptr::write_volatile(code.add(0x540) as *mut u64, ap_entry as u64);
            // Per-AP stack top (page offset 0x530) and 1-based index
            // (page offset 0x538): exactly what the blob reads.
            core::ptr::write_volatile(code.add(0x530) as *mut u64, ap_stack_top);
            core::ptr::write_volatile(code.add(0x538) as *mut u64, i as u64);
            // Clear the arrival handshake before waking this AP.
            core::ptr::write_volatile(code.add(0x548) as *mut u32, 0);
        }
        if !start_ap(apic_id, TRAMPOLINE_PHYS) {
            // // kprintln!("[smp] AP lapic_id={} failed to start", apic_id);
            continue;
        }
        // Wait for the AP to arrive in ap_entry_long: it writes its
        // 1-based index into the handshake slot at page offset 0x548.
        let handshake = (TRAMPOLINE_PHYS + 0x548) as *const u32;
        let mut arrived = false;
        // TCG is slow: give each AP up to 5 s to traverse the
        // trampoline and reach ap_entry_long before giving up.
        // TCG is slow and any MMIO access (HPET) inside this loop
        // grabs the QEMU BQL and starves the main loop so the HPET
        // counter never advances - a hard deadlock. Wait by pure
        // bounded pause spinning instead: no clocks, no MMIO.
        for _ in 0..200_000_000u64 {
            if core::ptr::read_volatile(handshake) == (i as u32) + 1 {
                arrived = true;
                break;
            }
            unsafe {
                core::arch::asm!("pause");
            }
        }
        if !arrived {
            smp_print_hex(b"[smp] AP handshake timeout idx=", i as u64, b" val=");
            smp_print_hex(b"", core::ptr::read_volatile(handshake) as u64, b"\r\n");
            // Retry ONLY when the handshake is still zero (the AP never
            // left the trampoline). A non-zero value means the AP is
            // already running ap_entry_long: re-sending the SIPI would
            // restart it from scratch and re-allocate its 64 KiB stack
            // and per-CPU area, wasting NonPaged pool (a real OOM source
            // during SMP bring-up).
            if core::ptr::read_volatile(handshake) == 0 {
                unsafe {
                    let code = TRAMPOLINE_PHYS as *mut u8;
                    install_blob_stages(code);
                    let ap_entry: unsafe extern "C" fn(u64) -> ! = ap_entry_long;
                    core::ptr::write_volatile(code.add(0x540) as *mut u64, ap_entry as u64);
                    core::ptr::write_volatile(code.add(0x530) as *mut u64, ap_stack_top);
                    core::ptr::write_volatile(code.add(0x538) as *mut u64, i as u64);
                    core::ptr::write_volatile(code.add(0x548) as *mut u32, 0);
                }
                start_ap(apic_id, TRAMPOLINE_PHYS);
                for _ in 0..400_000_000u64 {
                    if core::ptr::read_volatile(handshake) == (i as u32) + 1 {
                        arrived = true;
                        break;
                    }
                    unsafe {
                        core::arch::asm!("pause");
                    }
                }
                if arrived {
                    smp_print_hex(b"[smp] AP retry ok idx=", i as u64, b"\r\n");
                } else {
                    smp_print_hex(b"[smp] AP retry timeout idx=", i as u64, b"\r\n");
                }
            } else {
                // The AP is already up (its handshake raced with our
                // timeout) - treat it as arrived.
                arrived = true;
            }
        }
        if arrived {
            brought_up += 1;
        }
    }
    CPU_COUNT.store(brought_up, Ordering::SeqCst);
    // // kprintln!("[smp] {} CPU(s) up", brought_up);
}

/// Send INIT + SIPI + SIPI to bring up one AP. Returns true on
/// success (the AP is presumed up; we don't yet have a way to
/// confirm). `apic_id` is the LAPIC ID from the MADT.
fn start_ap(apic_id: u8, trampoline_phys: u64) -> bool {
    // 1. INIT IPI. Delivery mode lives in ICR bits 8-10: INIT = 100b
    // = 4 << 8 = 0x400, plus the level bit (bit 14) for the assert
    // phase => 0x4500. (An earlier value of 0x5000 decoded as
    // delivery=FIXED/vector=0 and never reset the AP.)
    send_ipi(apic_id, 0x0000_4500); // INIT, level assert, no shorthand
    spin_delay(10 * 200_000);
    // 2. First SIPI.
    let sipi = 0x0000_0600 | ((trampoline_phys >> 12) & 0xFF) as u32;
    send_ipi(apic_id, sipi);
    spin_delay(1 * 200_000);
    // 3. Second SIPI.
    send_ipi(apic_id, sipi);
    spin_delay(1 * 200_000);
    // Diagnostics: read back LAPIC ID, ESR and ICR after the SIPIs so a
    // dropped start-up IPI (illegal vector, non-existent destination,
    // pending delivery status) is visible on the serial log instead of
    // manifesting only as a silent handshake timeout.
    smp_print_hex(b"[smp] apic id=0x", apic_read(0x020) as u64, b" esr=0x");
    smp_print_hex(b"", apic_read(0x280) as u64, b" icr=0x");
    smp_print_hex(b"", apic_read(0x300) as u64, b"\r\n");
    true
}

/// Bounded busy-wait: spin `loops` times with `pause`. Deliberately
/// clockless - under TCG any MMIO access in a tight loop deadlocks
/// against the QEMU BQL, so SMP bring-up must never touch a timer.
fn spin_delay(loops: u64) {
    use core::arch::asm;
    for _ in 0..loops {
        unsafe {
            asm!("pause");
        }
    }
}

fn send_ipi(apic_id: u8, icr_low: u32) {
    apic_write(lapic_reg::ICR_HIGH, ((apic_id as u32) << 24) & 0xFF000000);
    apic_write(lapic_reg::ICR_LOW, icr_low);
    // Do NOT poll the ICR delivery-status bit (bit 12): under TCG it
    // never clears, and the MMIO read inside the polling loop grabs
    // the BQL and can deadlock the whole VM. The IPI is delivered
    // asynchronously by hardware regardless of that status bit.
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// 64-bit long-mode AP entry. Called by the trampoline after the
/// AP has switched to long mode. Sets up a per-CPU GDT/TSS,
/// allocates a per-CPU area, installs `IA32_GS_BASE`, and parks
/// the CPU in the scheduler.
#[no_mangle]
pub unsafe extern "C" fn ap_entry_long(ap_index: u64) -> ! {
    let ap_idx = ap_index as usize;
    smp_print_hex(b"[smp] AP enter idx=", ap_index, b"\r\n");
    if ap_idx >= tss::MAX_APS {
        smp_print_hex(b"[smp] ap_index out of range: ", ap_index, b"\r\n");
        loop {
            asm!("hlt");
        }
    }

    // CR4 is per-CPU. An AP boots with a boot-strapped CR4 (PAE only), so
    // SMEP/SMAP have to be re-established on every AP rather than relying
    // on the BSP having set them.
    cpu_flags::enable_user_boundary_protection();
    smp_print_hex(b"[smp] AP CR4 ok idx=", ap_index, b"\r\n");

    // Allocate a per-AP kernel stack. 64 KiB is more than enough
    // for bring-up; the real per-thread kernel stacks are
    // allocated later by the scheduler.
    const AP_STACK_SIZE: usize = 64 * 1024;
    let stack_base =
        crate::mm::pool::allocate(crate::mm::pool::PoolType::NonPaged, AP_STACK_SIZE) as u64;
    if stack_base == 0 {
        smp_print_hex(b"[smp] AP stack OOM idx=", ap_index, b"\r\n");
        loop {
            asm!("hlt");
        }
    }
    let rsp0 = stack_base + AP_STACK_SIZE as u64 - 16;
    smp_print_hex(b"[smp] AP stack ok idx=", ap_index, b" base=0x");
    smp_print_hex(b"", stack_base, b"\r\n");

    // Initialise the per-AP TSS and GDT.
    // SAFETY: AP_GDTS is module-scoped. Each AP accesses only its own index.
    // APs boot sequentially under BSP control, so there are no concurrent
    // writes to the same slot. The unsafe blocks below are safe because:
    // 1. Only this AP (identified by ap_idx) writes to AP_GDTS[ap_idx]
    // 2. No other CPU accesses this slot during initialization
    let (tss_ptr, tss_limit) = match tss::init_ap_tss(ap_idx, rsp0) {
        Some(v) => v,
        None => {
            smp_print_hex(b"[smp] AP TSS init failed idx=", ap_index, b"\r\n");
            loop {
                asm!("hlt");
            }
        }
    };
    // SAFETY: Same reasoning as above - this AP's GDT slot is private during boot
    unsafe {
        gdt::build_per_cpu_gdt(&mut AP_GDTS[ap_idx], tss_ptr as u64, tss_limit);
        gdt::ap_install(&AP_GDTS[ap_idx]);
    }
    smp_print_hex(b"[smp] AP GDT ok idx=", ap_index, b"\r\n");

    // 2. Initialise the local APIC. The BSP may have set the
    //    spurious-interrupt vector; each AP must re-enable it.
    crate::arch::x86_64::idt::install();
    smp_print_hex(
        b"[smp] AP IDT expected base=0x",
        crate::arch::x86_64::idt::static_base(),
        b"\r\n",
    );
    smp_print_hex(
        b"[smp] AP IDT actual base=0x",
        crate::arch::x86_64::idt::nt61_idt_current_base(),
        b"\r\n",
    );
    smp_print_hex(
        b"[smp] AP IDT bytes low=0x",
        crate::arch::x86_64::idt::nt61_idt_current_bytes_low(),
        b"\r\n",
    );
    smp_print_hex(
        b"[smp] AP IDT bytes high=0x",
        crate::arch::x86_64::idt::nt61_idt_current_bytes_high(),
        b"\r\n",
    );
    #[cfg(target_arch = "x86_64")]
    crate::hal::x86_64::apic::init_smp();
    smp_print_hex(b"[smp] AP APIC ok idx=", ap_index, b"\r\n");

    // 3. Allocate a per-CPU area for this AP. This ensures each CPU
    //    has its own independent PerCpuArea, preventing APs from
    //    overwriting BSP's per-CPU data.
    #[cfg(target_arch = "x86_64")]
    let per_cpu_page = crate::arch::x86_64::syscall::allocate_per_cpu_area(ap_index as u32);
    smp_print_hex(b"[smp] AP percpu alloc idx=", ap_index, b" page=0x");
    smp_print_hex(b"", per_cpu_page, b"\r\n");
    if per_cpu_page == 0 {
        smp_print_hex(b"[smp] AP per-CPU area OOM idx=", ap_index, b"\r\n");
        loop {
            asm!("hlt");
        }
    }

    // 4. Install the per-CPU area as this CPU's GS base.
    //    This MUST be done before any gs:[] access.
    #[cfg(target_arch = "x86_64")]
    crate::arch::x86_64::syscall::set_kernel_gs_base(per_cpu_page);
    smp_print_hex(b"[smp] AP GS_BASE idx=", ap_index, b" val=0x");
    smp_print_hex(b"", per_cpu_page, b"\r\n");
    // Signal the BSP that this AP made it into long mode. The
    // handshake slot lives in the identity-mapped trampoline page
    // (offset 0x548) and stores the 1-based AP index. APs boot
    // sequentially under BSP control, so a single slot suffices.
    // Kept after the GS_BASE print so the serial log distinguishes
    // "blob reached long mode" from "identity map missing".
    core::ptr::write_volatile(
        (TRAMPOLINE_PHYS + 0x548) as *mut u32,
        (ap_index as u32).wrapping_add(1),
    );

    // 5. Hand control to the scheduler. `init_smp_this_cpu`
    //    allocates a Prcb, creates the idle thread, and sets up
    //    the per-CPU current_thread/current_process slots.
    crate::ke::scheduler::init_smp_this_cpu(ap_index as u32);
    crate::ke::scheduler::idle_loop()
}

/// SMP smoke test.
///
/// Verifies:
/// 1. The SMP module initializes without panicking.
/// 2. `cpu_count()` returns at least 1 (the BSP).
/// 3. `parse_madt()` is callable (returns false on non-UEFI boot, true on OVMF).
/// 4. The static CPU_COUNT atomic is accessible.
pub fn smoke_test() -> bool {
    use core::sync::atomic::Ordering;

    // // kprintln!("  [SMP SMOKE] running SMP smoke test...");

    let mut ok = true;

    // Step 1: cpu_count returns a value
    let count = cpu_count();
    // // kprintln!("    [SMP SMOKE] cpu_count() = {}", count);
    if count < 1 {
        // // kprintln!("    [SMP SMOKE FAIL] cpu_count() < 1");
        ok = false;
    }

    // Step 2: Verify CPU_COUNT atomic is accessible
    let _ = CPU_COUNT.load(Ordering::Relaxed);

    // Step 3: Try to parse MADT (will return false if ACPI not available)
    let madt_ok = parse_madt();
    if !madt_ok {
        ok = false;
    }
    // // kprintln!("    [SMP SMOKE] parse_madt() = {}", madt_ok);

    // Step 4: After MADT parse, check if more CPUs are available
    let _count_after = cpu_count();
    // _count_after is intentionally unused - reserved for future logging
    // // kprintln!("    [SMP SMOKE] cpu_count() after MADT parse = {}", _count_after);

    if ok {
        // // kprintln!("  [SMP SMOKE] all SMP checks passed");
    } else {
        // // kprintln!("  [SMP SMOKE FAIL] one or more SMP checks failed");
    }
    ok
}

// ---------------------------------------------------------------------------
// extern "Rust" implementations required by arch::common::smp
//
// `arch::common::smp` declares these symbols via `unsafe extern "Rust"`.
// Each supported architecture must provide them, otherwise downstream
// crates that depend on `nt61` (e.g. `nt61-winload`) hit linker errors
// in debug builds where dead-code elimination doesn't strip the
// `arch::common::smp` wrappers. The x86_64 variants simply forward to
// the existing in-crate SMP machinery (cpu_count / smp_boot_aps) and
// the per-CPU area's CPU id.
// ---------------------------------------------------------------------------

#[no_mangle]
pub extern "Rust" fn __smp_cpu_count() -> u32 {
    cpu_count()
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
#[no_mangle]
pub unsafe extern "Rust" fn __smp_boot_secondary(pml4_pfn: u64) {
    smp_boot_aps(pml4_pfn);
}

#[no_mangle]
pub extern "Rust" fn __smp_get_current_cpu_id() -> u32 {
    super::percpu_impl::__percpu_get_current_cpu_id()
}
