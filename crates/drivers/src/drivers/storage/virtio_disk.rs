//! PCI VirtIO block disks exposed to disk.sys. Requests own a persistent DMA
//! page and are serialized per controller, including partial-sector updates.
use crate::{
    drivers::virtio::{blk::VirtioBlk, pci::PciTransport, Transport},
    ke::sync::Spinlock,
};
use alloc::{boxed::Box, vec::Vec};
use core::sync::atomic::{AtomicU32, Ordering};
struct Device {
    block: VirtioBlk,
    transport: Box<dyn Transport>,
}
struct State {
    initialized: bool,
    devices: Vec<Device>,
}
static STATE: Spinlock<State> = Spinlock::new(State {
    initialized: false,
    devices: Vec::new(),
});
pub fn init() {
    let mut state = STATE.lock();
    if state.initialized {
        return;
    }
    state.initialized = true;
    for pci in crate::hal::common::pci::enumerate() {
        if pci.vendor_id != 0x1af4 || !matches!(pci.device_id, 0x1001 | 0x1042) {
            continue;
        }
        unsafe {
            let Some(transport) = PciTransport::probe(0, pci.bus, pci.device, pci.function) else {
                continue;
            };
            let mut transport: Box<dyn Transport> = Box::new(transport);
            let Some(block) = VirtioBlk::probe(state.devices.len(), transport.as_mut()) else {
                continue;
            };
            state.devices.push(Device { block, transport });
            crate::hal::serial::write_string_always("[DISK] PCI VirtIO block device ready\r\n");
        }
    }
    #[cfg(any(target_arch = "aarch64", target_arch = "riscv64"))]
    {
        #[cfg(target_arch = "aarch64")]
        use crate::drivers::arch::aarch64::{MMIO_BASE, MMIO_SLOTS, MMIO_STRIDE};
        #[cfg(target_arch = "riscv64")]
        use crate::drivers::arch::riscv64::{MMIO_BASE, MMIO_SLOTS, MMIO_STRIDE};
        for slot in 0..MMIO_SLOTS {
            // Architecture discovery has already mapped these MMIO windows.
            let Some(transport) = (unsafe {
                crate::drivers::virtio::mmio::MmioTransport::probe(
                    MMIO_BASE + slot as u64 * MMIO_STRIDE,
                )
            }) else {
                continue;
            };
            if transport.device_id() != crate::drivers::virtio::device_id::BLOCK {
                continue;
            }
            let mut transport: Box<dyn Transport> = Box::new(transport);
            if let Some(block) =
                unsafe { VirtioBlk::probe(state.devices.len(), transport.as_mut()) }
            {
                state.devices.push(Device { block, transport });
            }
        }
    }
}
pub fn count() -> usize {
    STATE.lock().devices.len()
}
pub fn info(index: usize) -> Option<super::disk::DiskInfo> {
    STATE
        .lock()
        .devices
        .get(index)
        .map(|d| super::disk::DiskInfo {
            sector_count: d.block.capacity_sectors(),
        })
}
static READ_COUNT: AtomicU32 = AtomicU32::new(0);
static WRITE_COUNT: AtomicU32 = AtomicU32::new(0);

pub fn read(index: usize, offset: u64, buffer: &mut [u8]) -> usize {
    READ_COUNT.fetch_add(1, Ordering::Relaxed);
    let mut state = STATE.lock();
    let Some(d) = state.devices.get_mut(index) else {
        return 0;
    };
    if offset
        .checked_add(buffer.len() as u64)
        .is_none_or(|end| end > d.block.capacity_sectors().saturating_mul(512))
    {
        return 0;
    }
    let mut done = 0;
    let mut sector = [0u8; 512];
    while done < buffer.len() {
        let address = offset + done as u64;
        let within = (address % 512) as usize;
        if !unsafe {
            d.block
                .read_sectors(d.transport.as_mut(), address / 512, &mut sector)
        } {
            break;
        }
        let count = (512 - within).min(buffer.len() - done);
        buffer[done..done + count].copy_from_slice(&sector[within..within + count]);
        done += count;
    }
    done
}
pub fn write(index: usize, offset: u64, buffer: &[u8]) -> usize {
    WRITE_COUNT.fetch_add(1, Ordering::Relaxed);
    let mut state = STATE.lock();
    let Some(d) = state.devices.get_mut(index) else {
        return 0;
    };
    if offset
        .checked_add(buffer.len() as u64)
        .is_none_or(|end| end > d.block.capacity_sectors().saturating_mul(512))
    {
        return 0;
    }
    let mut done = 0;
    let mut sector = [0u8; 512];
    while done < buffer.len() {
        let address = offset + done as u64;
        let within = (address % 512) as usize;
        let count = (512 - within).min(buffer.len() - done);
        if (within != 0 || count != 512)
            && !unsafe {
                d.block
                    .read_sectors(d.transport.as_mut(), address / 512, &mut sector)
            }
        {
            break;
        }
        sector[within..within + count].copy_from_slice(&buffer[done..done + count]);
        if !unsafe {
            d.block
                .write_sectors(d.transport.as_mut(), address / 512, &sector)
        } {
            break;
        }
        done += count;
    }
    if done != 0 && d.block.has_flush() && !unsafe { d.block.flush(d.transport.as_mut()) } {
        return 0;
    }
    done
}

/// Disk class facade for architectures whose storage is carried by VirtIO.
#[cfg(not(target_arch = "x86_64"))]
pub struct VirtioDiskBackend;

#[cfg(not(target_arch = "x86_64"))]
impl super::disk::DiskBackend for VirtioDiskBackend {
    unsafe fn init(&self) {
        init();
        register_dispatch();
        crate::drivers::partmgr::init();
        for index in 0..count() {
            crate::drivers::volmgr::refresh_disk(index);
        }
    }
    fn disk_count(&self) -> usize {
        count()
    }
    fn get_disk(&self, index: usize) -> Option<super::disk::DiskInfo> {
        info(index)
    }
    fn disk_read(&self, index: usize, offset: u64, byte_count: usize, buffer: &mut [u8]) -> usize {
        buffer
            .get_mut(..byte_count)
            .map_or(0, |buffer| read(index, offset, buffer))
    }
    fn disk_write(&self, index: usize, offset: u64, byte_count: usize, buffer: &[u8]) -> usize {
        buffer
            .get(..byte_count)
            .map_or(0, |buffer| write(index, offset, buffer))
    }
    fn smoke_test(&self) -> bool {
        (0..count()).all(|index| read(index, 0, &mut [0; 512]) == 512)
    }
    fn read_count(&self) -> u32 {
        READ_COUNT.load(Ordering::Relaxed)
    }
    fn write_count(&self) -> u32 {
        WRITE_COUNT.load(Ordering::Relaxed)
    }
}

#[cfg(not(target_arch = "x86_64"))]
unsafe fn register_dispatch() {
    use crate::io;
    static REGISTERED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
    if REGISTERED.swap(true, Ordering::AcqRel) {
        return;
    }
    let driver = io::allocate_driver(b"\\Driver\\disk");
    if driver.is_null() {
        return;
    }
    let mut table = io::dispatch::DispatchTable::new();
    table.major_functions[io::major::IRP_MJ_READ as usize] = Some(dispatch_read);
    table.major_functions[io::major::IRP_MJ_WRITE as usize] = Some(dispatch_write);
    table.install(driver);
    for index in 0..count() {
        let name = alloc::format!("\\Device\\Harddisk{}", index);
        let device = io::create_device(driver, io::DeviceType::Disk, name.as_bytes());
        if device.is_null() {
            continue;
        }
        (*device).device_extension = Box::into_raw(Box::new(index)).cast();
        let mut storage = super::StorageDevice::new();
        storage.device_type = super::StorageDeviceType::VirtioDisk;
        storage.total_sectors = info(index).map_or(0, |d| d.sector_count);
        storage.controller = index;
        storage.present = true;
        if let Some(id) = super::register_device(storage) {
            super::io_dispatch::register_device(id, device);
        }
    }
}

#[cfg(not(target_arch = "x86_64"))]
unsafe extern "C" fn dispatch_read(
    device: *mut crate::io::DeviceObject,
    irp: *mut crate::io::Irp,
) -> i32 {
    dispatch_request(device, irp, false)
}
#[cfg(not(target_arch = "x86_64"))]
unsafe extern "C" fn dispatch_write(
    device: *mut crate::io::DeviceObject,
    irp: *mut crate::io::Irp,
) -> i32 {
    dispatch_request(device, irp, true)
}
#[cfg(not(target_arch = "x86_64"))]
unsafe fn dispatch_request(
    device: *mut crate::io::DeviceObject,
    irp: *mut crate::io::Irp,
    writing: bool,
) -> i32 {
    use crate::libs::ntdll::status::{STATUS_IO_DEVICE_ERROR, STATUS_SUCCESS};
    if device.is_null() || irp.is_null() || (*device).device_extension.is_null() {
        return STATUS_IO_DEVICE_ERROR;
    }
    let index = *(*device).device_extension.cast::<usize>();
    let (offset, count, buffer) = crate::io::dispatch::storage_request_parameters(irp);
    if count == 0 {
        return STATUS_SUCCESS;
    }
    if buffer.is_null() {
        return STATUS_IO_DEVICE_ERROR;
    }
    let count = count as usize;
    let completed = if writing {
        write(index, offset, core::slice::from_raw_parts(buffer, count))
    } else {
        read(
            index,
            offset,
            core::slice::from_raw_parts_mut(buffer, count),
        )
    };
    if completed == count {
        STATUS_SUCCESS
    } else {
        STATUS_IO_DEVICE_ERROR
    }
}
