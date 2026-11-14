//! Architecture-specific driver support for aarch64.
//!
//! QEMU aarch64 virt machine exposes virtio devices on the
//! virtio-mmio transport (hw/arm/virt.c VIRT_MMIO): 32 slots starting
//! at 0x0a000000 with a 0x200-byte stride. All devices share one
//! level-triggered SPI (GIC SPI 36 / INTID 68); the kernel wires the
//! shared handler during GIC bring-up, so this module only discovers
//! and registers devices.

use crate::drivers::virtio::{device_id, mmio::MmioTransport};

// hw/arm/virt.c VIRT_MMIO constants.
pub const MMIO_BASE: u64 = 0x0a000000;
pub const MMIO_SLOTS: usize = 32;
pub const MMIO_STRIDE: u64 = 0x200;

/// Discovered virtio device handles, indexed by discovery order.
pub struct VirtioDevices {
    pub blk: alloc::vec::Vec<crate::drivers::virtio::blk::VirtioBlk>,
    pub net: alloc::vec::Vec<crate::drivers::virtio::net::VirtioNet>,
    pub gpu: alloc::vec::Vec<crate::drivers::virtio::gpu::VirtioGpu>,
    /// Transports backing the discovered devices; the index in each
    /// device record refers back here.
    pub transports: alloc::vec::Vec<MmioTransport>,
}

use crate::ke::sync::Spinlock;

/// Cached probe result for the dispatch handlers (Spinlock for safe access).
static CACHED_DEVICES: Spinlock<Option<VirtioDevices>> = Spinlock::new(None);

/// Discover auxiliary devices without taking ownership of block queues.
///
/// # Safety
/// The architecture must map its PCI and MMIO windows before discovery.
/// The common disk backend registers storage dispatch after discovery.
pub unsafe fn register_dispatch() {
    *CACHED_DEVICES.lock() = Some(probe_virtio_devices());
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// Probe all virtio-mmio slots and initialise matching drivers.
pub unsafe fn probe_virtio_devices() -> VirtioDevices {
    let mut transports = alloc::vec::Vec::new();
    let blk = alloc::vec::Vec::new();
    let mut net = alloc::vec::Vec::new();
    let gpu = alloc::vec::Vec::new();

    for slot in 0..MMIO_SLOTS {
        let base = MMIO_BASE + (slot as u64) * MMIO_STRIDE;
        let transport = match MmioTransport::probe(base) {
            Some(t) => t,
            None => continue,
        };

        match transport.device_id() {
            device_id::BLOCK => {
                    // The shared disk backend owns block queues and disk numbering.
                }
                device_id::NET => {
            if !nt61_boot_types::boot_options::active().network_enabled() { continue; }
                let idx = transports.len();
                transports.push(transport);
                let t = transports.last_mut().unwrap();
                if let Some(dev) = crate::drivers::virtio::net::VirtioNet::probe(idx, t) {
                    net.push(dev);
                }
            }
            device_id::GPU => {
                    // BootVid owns scanout; do not reset its active queues here.
                }
            _ => {
                // Device type not yet supported; leave the slot idle.
            }
        }
    }

    VirtioDevices {
        blk,
        net,
        gpu,
        transports,
    }
}
