//! Architecture-specific driver support for loongarch64.
//!
//! QEMU loongarch64 virt machine has no virtio-mmio; all virtio
//! devices are attached to the GPEx PCI host bridge (hw/loongarch/
//! virt.h). ECAM config space starts at VIRT_PCI_CFG_BASE = 0x20000000,
//! the first memory BAR window at VIRT_PCI_MEM_BASE = 0x40000000.
//! Discovery walks ECAM bus 0 for modern virtio-pci functions.

use crate::drivers::virtio::{device_id, pci::PciTransport};

// hw/loongarch/virt.h constants.
pub const ECAM_BASE: u64 = 0x20000000;
pub const PCI_MEM_BASE: u64 = 0x40000000;
pub const PCI_BUS_COUNT: u8 = 1;

/// Discovered virtio device handles.
pub struct VirtioDevices {
    pub blk: alloc::vec::Vec<crate::drivers::virtio::blk::VirtioBlk>,
    pub net: alloc::vec::Vec<crate::drivers::virtio::net::VirtioNet>,
    pub gpu: alloc::vec::Vec<crate::drivers::virtio::gpu::VirtioGpu>,
    pub transports: alloc::vec::Vec<PciTransport>,
}

use crate::ke::sync::Spinlock;

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
/// Scan the ECAM window for modern virtio-pci devices and initialise
/// the matching drivers.
pub unsafe fn probe_virtio_devices() -> VirtioDevices {
    let mut transports = alloc::vec::Vec::new();
    let blk = alloc::vec::Vec::new();
    let mut net = alloc::vec::Vec::new();
    let gpu = alloc::vec::Vec::new();

    for bus in 0..PCI_BUS_COUNT {
        for transport in PciTransport::scan_bus(ECAM_BASE, bus) {
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
                _ => {}
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
