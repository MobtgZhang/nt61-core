//! RISC-V video-registry adapter for the shared basic display path.
//!
//! Queue negotiation and scanout belong to `virtio::gpu` through BootVid on
//! every architecture. This adapter uses the installed firmware framebuffer;
//! it must never reset that device or invent a second MMIO command interface.

use crate::drivers::video::core::gpu_common::{
    DisplayMode, GpuDeviceInfo, GpuDriver, GpuError, GpuFeatures, GpuFramebufferInfo, PixelFormat,
};
use crate::hal::common::framebuffer_impl as framebuffer;
use super::pci_ids;

#[derive(Debug)]
pub struct VirtioGpuDevice {
    pub fb_phys: u64,
    pub fb_size: u64,
    pub fb_virt: u64,
    pub width: u32,
    pub height: u32,
    pub pitch: u32,
    pub format: PixelFormat,
    pub device_id: u32,
}

impl VirtioGpuDevice {
    pub fn new(device_id: u32) -> Self {
        Self { fb_phys: 0, fb_size: 0, fb_virt: 0, width: 0, height: 0,
            pitch: 0, format: PixelFormat::Bgra8888, device_id }
    }

    pub fn flush(&mut self, x: u32, y: u32, width: u32, height: u32) -> Result<(), GpuError> {
        framebuffer::mark_damage(x, y, width, height);
        if crate::drivers::bootvid::present_desktop_frame() { Ok(()) }
        else { Err(GpuError::Timeout) }
    }
}

impl GpuDriver for VirtioGpuDevice {
    fn device_info(&self) -> GpuDeviceInfo {
        GpuDeviceInfo { vendor_id: pci_ids::VIRTIO_VENDOR_ID, device_id: self.device_id as u16,
            revision: 0, bus: 0, device: 0, function: 0, subsystem_vendor_id: 0, subsystem_id: 0 }
    }
    fn features(&self) -> GpuFeatures {
        // Basic display and the desktop compositor use software rendering.
        GpuFeatures::default()
    }
    fn init(&mut self) -> Result<(), GpuError> {
        self.init_framebuffer(None).map(|_| ())
    }
    fn init_framebuffer(&mut self, mode: Option<DisplayMode>) -> Result<GpuFramebufferInfo, GpuError> {
        let fb = framebuffer::info();
        if fb.address == 0 || fb.width == 0 || fb.height == 0 || fb.bpp != 32 {
            return Err(GpuError::MemAccessError);
        }
        if mode.is_some_and(|m| m.width != fb.width || m.height != fb.height || m.bpp != fb.bpp) {
            return Err(GpuError::InvalidCmd); // Firmware mode is retained.
        }
        self.fb_phys = crate::mm::virt_to_phys(fb.address).ok_or(GpuError::MemAccessError)?;
        self.fb_virt = fb.address;
        self.width = fb.width;
        self.height = fb.height;
        self.pitch = fb.pitch;
        self.fb_size = fb.pitch as u64 * fb.height as u64;
        self.framebuffer_info().ok_or(GpuError::MemAccessError)
    }
    fn set_mode(&mut self, mode: &DisplayMode) -> Result<(), GpuError> {
        self.init_framebuffer(Some(*mode)).map(|_| ())
    }
    fn get_mode(&self) -> Option<DisplayMode> {
        (self.width != 0).then(|| DisplayMode::new(self.width, self.height, 60, 32))
    }
    fn enable_vblank(&mut self, _: u32) -> Result<(), GpuError> { Err(GpuError::InvalidCmd) }
    fn disable_vblank(&mut self, _: u32) {}
    fn wait_vblank(&self, _: u32, _: u32) -> Result<(), GpuError> { Err(GpuError::InvalidCmd) }
    fn clear(&mut self, color: u32) {
        if self.fb_virt == 0 { return; }
        for y in 0..self.height {
            for x in 0..self.width {
                unsafe { ((self.fb_virt + y as u64 * self.pitch as u64 + x as u64 * 4) as *mut u32).write_volatile(color); }
            }
        }
        framebuffer::mark_damage(0, 0, self.width, self.height);
    }
    fn set_pixel(&mut self, x: u32, y: u32, color: u32) {
        if self.fb_virt != 0 && x < self.width && y < self.height { framebuffer::set_pixel(x, y, color); }
    }
    fn framebuffer_info(&self) -> Option<GpuFramebufferInfo> {
        (self.fb_virt != 0).then_some(GpuFramebufferInfo { address: self.fb_phys,
            virtual_address: self.fb_virt, size: self.fb_size, width: self.width,
            height: self.height, pitch: self.pitch, bpp: 32, format: self.format })
    }
    fn enable_bus_mastering(&mut self) {} // Owned by the shared PCI transport.
    fn shutdown(&mut self) {} // The desktop retains primary scanout ownership.
}

#[cfg(target_arch = "riscv64")]
pub fn probe() -> bool {
    crate::drivers::bus::pci_bus::find_device(pci_ids::VIRTIO_VENDOR_ID, pci_ids::VIRTIO_GPU_DEVICE_ID).is_some()
}
#[cfg(not(target_arch = "riscv64"))]
pub fn probe() -> bool { false }

pub fn init() -> Option<VirtioGpuDevice> {
    if !probe() { return None; }
    let mut device = VirtioGpuDevice::new(pci_ids::VIRTIO_GPU_DEVICE_ID as u32);
    device.init().ok()?;
    Some(device)
}
