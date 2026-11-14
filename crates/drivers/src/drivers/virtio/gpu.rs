//! virtio-gpu display driver (virtio spec 4.4).
//!
//! The boot display keeps one resource and its backing guest memory
//! alive. Blob-capable devices scan out the guest backing directly and
//! frame presentation only submits `RESOURCE_FLUSH`. Legacy devices use
//! the 2D transfer path. Resources and queues are never recreated per
//! frame.

use super::{queue::QueueBuffer, status, Transport, FEATURE_VERSION_1};

use core::cell::UnsafeCell;
use crate::ke::sync::Spinlock;

// All devices share stable DMA command storage; serialize its complete lifetime.
static COMMAND_LOCK: Spinlock<()> = Spinlock::new(());

pub const CMD_GET_DISPLAY_INFO: u32 = 0x0100;
pub const CMD_RESOURCE_CREATE_2D: u32 = 0x0101;
pub const CMD_RESOURCE_UNREF: u32 = 0x0102;
pub const CMD_SET_SCANOUT: u32 = 0x0103;
pub const CMD_RESOURCE_FLUSH: u32 = 0x0104;
pub const CMD_TRANSFER_TO_HOST_2D: u32 = 0x0105;
pub const CMD_GET_EDID: u32 = 0x010a;
pub const CMD_RESOURCE_ATTACH_BACKING: u32 = 0x0106;
pub const CMD_RESOURCE_CREATE_BLOB: u32 = 0x010d;
pub const CMD_SET_SCANOUT_BLOB: u32 = 0x010e;
pub const CMD_RESP_OK_NODATA: u32 = 0x1100;
pub const CMD_RESP_OK_TRANSFER_2D: u32 = 0x1100;

pub const FORMAT_B8G8R8X8_UNORM: u32 = 2;
pub const BLOB_MEM_GUEST: u32 = 1;
pub const BLOB_FLAG_USE_MAPPABLE: u32 = 1;
pub const F_RESOURCE_BLOB: u64 = 1 << 3;
pub const CFG_NUM_SCANOUTS: u64 = 8;

struct GpuResponseCell(UnsafeCell<CtrlHeader>);

unsafe impl Sync for GpuResponseCell {}

static GPU_RESPONSE: GpuResponseCell = GpuResponseCell(UnsafeCell::new(CtrlHeader {
    cmd_type: 0,
    flags: 0,
    fence_id: 0,
    ctx_id: 0,
    ring_idx: 0,
}));

#[repr(C, align(16))]
struct CommandScratch {
    bytes: [u8; 128],
}

struct CommandScratchCell(UnsafeCell<CommandScratch>);

unsafe impl Sync for CommandScratchCell {}

static GPU_COMMAND: CommandScratchCell =
    CommandScratchCell(UnsafeCell::new(CommandScratch { bytes: [0; 128] }));

static GPU_AUX_COMMAND: CommandScratchCell =
    CommandScratchCell(UnsafeCell::new(CommandScratch { bytes: [0; 128] }));

#[repr(C)]
#[derive(Clone, Copy)]
pub struct CtrlHeader {
    pub cmd_type: u32,
    pub flags: u32,
    pub fence_id: u64,
    pub ctx_id: u32,
    pub ring_idx: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ResourceCreate2d {
    pub hdr: CtrlHeader,
    pub resource_id: u32,
    pub format: u32,
    pub width: u32,
    pub height: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ResourceMemoryEntry {
    pub addr: u64,
    pub size: u32,
    pub _padding: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ResourceAttachBacking {
    pub hdr: CtrlHeader,
    pub resource_id: u32,
    pub num_entries: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct SetScanout {
    pub hdr: CtrlHeader,
    pub r_x: u32,
    pub r_y: u32,
    pub r_width: u32,
    pub r_height: u32,
    pub scanout_id: u32,
    pub resource_id: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ResourceCreateBlob {
    pub hdr: CtrlHeader,
    pub resource_id: u32,
    pub blob_mem: u32,
    pub blob_flags: u32,
    pub nr_entries: u32,
    pub blob_id: u64,
    pub size: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct SetScanoutBlob {
    pub hdr: CtrlHeader,
    pub r_x: u32,
    pub r_y: u32,
    pub r_width: u32,
    pub r_height: u32,
    pub scanout_id: u32,
    pub resource_id: u32,
    pub width: u32,
    pub height: u32,
    pub format: u32,
    pub _padding: u32,
    pub strides: [u32; 4],
    pub offsets: [u32; 4],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct TransferToHost2d {
    pub hdr: CtrlHeader,
    pub rect_x: u32,
    pub rect_y: u32,
    pub rect_width: u32,
    pub rect_height: u32,
    pub offset: u64,
    pub resource_id: u32,
    pub _padding: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ResourceFlush {
    pub hdr: CtrlHeader,
    pub rect_x: u32,
    pub rect_y: u32,
    pub rect_width: u32,
    pub rect_height: u32,
    pub resource_id: u32,
    pub _padding: u32,
}

const _: () = assert!(core::mem::size_of::<CtrlHeader>() == 24);
const _: () = assert!(core::mem::size_of::<ResourceCreate2d>() == 40);
const _: () = assert!(core::mem::size_of::<ResourceMemoryEntry>() == 16);
const _: () = assert!(core::mem::size_of::<ResourceAttachBacking>() == 32);
const _: () = assert!(core::mem::size_of::<SetScanout>() == 48);
const _: () = assert!(core::mem::size_of::<ResourceCreateBlob>() == 56);
const _: () = assert!(core::mem::size_of::<SetScanoutBlob>() == 96);
const _: () = assert!(core::mem::size_of::<TransferToHost2d>() == 56);
const _: () = assert!(core::mem::size_of::<ResourceFlush>() == 48);

pub struct VirtioGpu {
    transport_index: usize,
    ctrl_queue: super::queue::QueueLayout,
    num_scanouts: u32,
    next_resource_id: u32,
    resource_id: u32,
    width: u32,
    height: u32,
    blob_supported: bool,
    failed: bool,
}

impl VirtioGpu {
    /// # Safety
    ///
    /// Caller must ensure transport MMIO and queue memory are valid.
    pub unsafe fn probe(
        transport_index: usize,
        transport: &mut dyn Transport,
    ) -> Option<VirtioGpu> {
        transport.reset();
        transport.write_status(status::ACKNOWLEDGE);
        transport.write_status(status::ACKNOWLEDGE | status::DRIVER);

        let device_features = transport.read_device_features();
        if device_features & FEATURE_VERSION_1 == 0 {
            transport.write_status(status::FAILED);
            return None;
        }
        let features = FEATURE_VERSION_1 | (device_features & F_RESOURCE_BLOB);
        transport.write_driver_features(features);
        transport.write_status(status::ACKNOWLEDGE | status::DRIVER | status::FEATURES_OK);
        if transport.read_status() & (status::FEATURES_OK | status::FAILED) != status::FEATURES_OK {
            return None;
        }

        let max = transport.max_queue_size(0);
        let ctrl_queue = super::queue::QueueLayout::allocate(max as u16)?;
        if !transport.setup_queue(0, &ctrl_queue) {
            return None;
        }
        transport.write_status(
            status::ACKNOWLEDGE | status::DRIVER | status::FEATURES_OK | status::DRIVER_OK,
        );

        let num_scanouts = transport.config_read_u32(CFG_NUM_SCANOUTS);
        Some(VirtioGpu {
            transport_index,
            ctrl_queue,
            num_scanouts,
            next_resource_id: 1,
            resource_id: 0,
            width: 0,
            height: 0,
            blob_supported: device_features & F_RESOURCE_BLOB != 0,
            failed: false,
        })
    }

    pub fn num_scanouts(&self) -> u32 {
        self.num_scanouts
    }

    /// # Safety
    ///
    /// Caller must ensure `fb_addr` remains mapped and unchanged for the
    /// lifetime of the persistent GPU resource.
    pub unsafe fn initialize_display(
        &mut self,
        transport: &mut dyn Transport,
        scanout_id: u32,
        fb_addr: *const u8,
        width: u32,
        height: u32,
    ) -> bool {
        let _commands = COMMAND_LOCK.lock();
        if self.failed || self.resource_id != 0 { return false; }
        let initialized = self.initialize_display_locked(transport, scanout_id, fb_addr, width, height);
        if !initialized {
            // Stop all device access before shared command storage can be reused.
            transport.reset();
            self.failed = true;
            self.resource_id = 0;
        }
        initialized
    }

    unsafe fn initialize_display_locked(
        &mut self, transport: &mut dyn Transport, scanout_id: u32,
        fb_addr: *const u8, width: u32, height: u32,
    ) -> bool {
        if width == 0 || height == 0 || fb_addr.is_null()
            || scanout_id >= self.num_scanouts
            || width.checked_mul(height).and_then(|n| n.checked_mul(4)).is_none()
        {
            crate::hal::serial::write_string_always("[GPU] invalid scanout count=");
            crate::hal::serial::write_hex_u64_always(self.num_scanouts as u64);
            crate::hal::serial::write_string_always(" width=");
            crate::hal::serial::write_hex_u64_always(width as u64);
            crate::hal::serial::write_string_always(" height=");
            crate::hal::serial::write_hex_u64_always(height as u64);
            crate::hal::serial::write_string_always("\r\n");
            return false;
        }

        // Never describe discontiguous guest pages as one DMA entry.
        let Some(base) = crate::mm::virt_to_phys(fb_addr as u64) else {
            crate::hal::serial::write_string_always("[GPU] canvas address unmapped\r\n");
            return false;
        };
        let bytes = width as u64 * height as u64 * 4;
        let mut offset = 4096 - ((fb_addr as u64) & 4095);
        while offset < bytes {
            if crate::mm::virt_to_phys(fb_addr as u64 + offset) != Some(base + offset) {
                crate::hal::serial::write_string_always("[GPU] noncontiguous canvas base=");
                crate::hal::serial::write_hex_u64_always(base);
                crate::hal::serial::write_string_always(" offset=");
                crate::hal::serial::write_hex_u64_always(offset);
                crate::hal::serial::write_string_always(" actual=");
                crate::hal::serial::write_hex_u64_always(crate::mm::virt_to_phys(fb_addr as u64 + offset).unwrap_or(u64::MAX));
                crate::hal::serial::write_string_always("\r\n");
                return false;
            }
            offset += 4096;
        }
        if self.blob_supported {
            if self.initialize_display_blob(transport, scanout_id, fb_addr, width, height) {
                return self.present_region_locked(transport, 0, 0, width, height);
            }
            // An advertised optional blob path can still be rejected by the
            // host backend. Keep the standard 2D display path available.
            if self.failed { return false; }
            self.blob_supported = false;
        }
        self.initialize_display_2d(transport, scanout_id, fb_addr, width, height)
            && self.present_region_locked(transport, 0, 0, width, height)
    }

    unsafe fn initialize_display_blob(
        &mut self,
        transport: &mut dyn Transport,
        scanout_id: u32,
        fb_addr: *const u8,
        width: u32,
        height: u32,
    ) -> bool {
        let resource_id = self.next_resource_id;
        self.next_resource_id += 1;
        let Some(backing_phys) = crate::mm::virt_to_phys(fb_addr as u64) else {
            crate::hal::serial::write_string_always("[GPU] blob backing translate failed\r\n");
            return false;
        };
        let backing_len = width * height * 4;
        let backing = ResourceMemoryEntry {
            addr: backing_phys,
            size: backing_len,
            _padding: 0,
        };
        let create = ResourceCreateBlob {
            hdr: Self::header(CMD_RESOURCE_CREATE_BLOB),
            resource_id,
            blob_mem: BLOB_MEM_GUEST,
            blob_flags: BLOB_FLAG_USE_MAPPABLE,
            nr_entries: 1,
            blob_id: 0,
            size: backing_len as u64,
        };
        let Some(head) = self.ctrl_queue.add_chain(&[
            Self::scratch_buffer(create),
            Self::aux_buffer(backing),
            Self::response_buffer(),
        ]) else {
            crate::hal::serial::write_string_always("[GPU] blob create add_chain failed\r\n");
            return false;
        };
        unsafe { transport.notify(0) };
        if self.wait_completion(transport, head).is_none() {
            crate::hal::serial::write_string_always("[GPU] blob create completion failed\r\n");
            return false;
        }
        if Self::read_response().cmd_type != CMD_RESP_OK_NODATA {
            crate::hal::serial::write_string_always("[GPU] blob create response failed\r\n");
            return false;
        }

        let scanout = SetScanoutBlob {
            hdr: Self::header(CMD_SET_SCANOUT_BLOB),
            r_x: 0,
            r_y: 0,
            r_width: width,
            r_height: height,
            scanout_id,
            resource_id,
            width,
            height,
            format: FORMAT_B8G8R8X8_UNORM,
            _padding: 0,
            strides: [width * 4; 4],
            offsets: [0; 4],
        };
        let Some(head) = self
            .ctrl_queue
            .add_chain(&[Self::scratch_buffer(scanout), Self::response_buffer()])
        else {
            crate::hal::serial::write_string_always("[GPU] blob scanout add_chain failed\r\n");
            return false;
        };
        unsafe { transport.notify(0) };
        if self.wait_completion(transport, head).is_none() {
            crate::hal::serial::write_string_always("[GPU] blob scanout completion failed\r\n");
            return false;
        }
        let response = Self::read_response();
        if response.cmd_type != CMD_RESP_OK_NODATA {
            crate::hal::serial::write_string_always("[GPU] blob scanout response failed type=0x");
            crate::hal::serial::write_hex_u64_always(response.cmd_type as u64);
            crate::hal::serial::write_string_always(" scanout_id=");
            crate::hal::serial::write_hex_u64_always(scanout_id as u64);
            crate::hal::serial::write_string_always(" resource_id=");
            crate::hal::serial::write_hex_u64_always(resource_id as u64);
            crate::hal::serial::write_string_always("\r\n");
            self.release_resource(transport, resource_id);
            return false;
        }

        self.resource_id = resource_id;
        self.width = width;
        self.height = height;
        true
    }

    unsafe fn initialize_display_2d(
        &mut self,
        transport: &mut dyn Transport,
        scanout_id: u32,
        fb_addr: *const u8,
        width: u32,
        height: u32,
    ) -> bool {
        let resource_id = self.next_resource_id;
        self.next_resource_id += 1;
        let create = ResourceCreate2d {
            hdr: Self::header(CMD_RESOURCE_CREATE_2D),
            resource_id,
            format: FORMAT_B8G8R8X8_UNORM,
            width,
            height,
        };
        let Some(head) = self
            .ctrl_queue
            .add_chain(&[Self::scratch_buffer(create), Self::response_buffer()])
        else {
            crate::hal::serial::write_string_always("[GPU] create add_chain failed\r\n");
            return false;
        };
        unsafe { transport.notify(0) };
        if self.wait_completion(transport, head).is_none() {
            crate::hal::serial::write_string_always("[GPU] create completion failed\r\n");
            return false;
        }
        let response = Self::read_response();
        if response.cmd_type != CMD_RESP_OK_NODATA {
            crate::hal::serial::write_string_always("[GPU] create response failed\r\n");
            crate::hal::serial::write_hex_u64_always(response.cmd_type as u64);
            crate::hal::serial::write_string_always("\r\n");
            return false;
        }

        let Some(backing_phys) = crate::mm::virt_to_phys(fb_addr as u64) else {
            crate::hal::serial::write_string_always("[GPU] backing translate failed\r\n");
            return false;
        };
        let backing = ResourceMemoryEntry {
            addr: backing_phys,
            size: width * height * 4,
            _padding: 0,
        };
        let attach = ResourceAttachBacking {
            hdr: Self::header(CMD_RESOURCE_ATTACH_BACKING),
            resource_id,
            num_entries: 1,
        };
        let Some(head) = self.ctrl_queue.add_chain(&[
            Self::scratch_buffer(attach),
            Self::aux_buffer(backing),
            Self::response_buffer(),
        ]) else {
            crate::hal::serial::write_string_always("[GPU] attach add_chain failed\r\n");
            return false;
        };
        unsafe { transport.notify(0) };
        if self.wait_completion(transport, head).is_none() {
            crate::hal::serial::write_string_always("[GPU] attach completion failed\r\n");
            return false;
        }
        let response = Self::read_response();
        if response.cmd_type != CMD_RESP_OK_NODATA {
            crate::hal::serial::write_string_always("[GPU] attach response failed\r\n");
            return false;
        }

        let scanout = SetScanout {
            hdr: Self::header(CMD_SET_SCANOUT),
            r_x: 0,
            r_y: 0,
            r_width: width,
            r_height: height,
            scanout_id,
            resource_id,
        };
        let Some(head) = self
            .ctrl_queue
            .add_chain(&[Self::scratch_buffer(scanout), Self::response_buffer()])
        else {
            crate::hal::serial::write_string_always("[GPU] scanout add_chain failed\r\n");
            return false;
        };
        unsafe { transport.notify(0) };
        if self.wait_completion(transport, head).is_none() {
            crate::hal::serial::write_string_always("[GPU] scanout completion failed\r\n");
            return false;
        }
        let response = Self::read_response();
        if response.cmd_type != CMD_RESP_OK_NODATA {
            crate::hal::serial::write_string_always("[GPU] scanout response failed\r\n");
            return false;
        }

        self.resource_id = resource_id;
        self.width = width;
        self.height = height;
        true
    }

    /// Present the persistent backing store. This performs no resource
    /// creation and no CPU framebuffer copy.
    pub fn present_display(&mut self, transport: &mut dyn Transport) -> bool {
        self.present_region(transport, 0, 0, self.width, self.height)
    }

    /// Submit clipped damage against the persistent scanout. Empty damage
    /// is a successful no-op; guest offsets use the full resource stride.
    pub fn present_region(&mut self, transport: &mut dyn Transport, x: u32, y: u32, width: u32, height: u32) -> bool {
        let _commands = COMMAND_LOCK.lock();
        self.present_region_locked(transport, x, y, width, height)
    }

    fn present_region_locked(&mut self, transport: &mut dyn Transport, x: u32, y: u32, width: u32, height: u32) -> bool {
        if self.failed || self.resource_id == 0 { return false; }
        let width = width.min(self.width.saturating_sub(x));
        let height = height.min(self.height.saturating_sub(y));
        if width == 0 || height == 0 { return true; }
        if !self.blob_supported {
            let transfer = TransferToHost2d {
                hdr: Self::header(CMD_TRANSFER_TO_HOST_2D),
                rect_x: x, rect_y: y, rect_width: width, rect_height: height,
                offset: (y as u64 * self.width as u64 + x as u64) * 4,
                resource_id: self.resource_id, _padding: 0,
            };
            let Some(head) = self.ctrl_queue.add_chain(&[Self::scratch_buffer(transfer), Self::response_buffer()]) else { return false; };
            // SAFETY: the queue and persistent backing stay mapped until completion.
            unsafe { transport.notify(0) };
            if self.wait_completion(transport, head).is_none() || Self::read_response().cmd_type != CMD_RESP_OK_NODATA {
                return false;
            }
        }
        let flush = ResourceFlush {
            hdr: Self::header(CMD_RESOURCE_FLUSH), rect_x: x, rect_y: y,
            rect_width: width, rect_height: height, resource_id: self.resource_id, _padding: 0,
        };
        let Some(head) = self.ctrl_queue.add_chain(&[Self::scratch_buffer(flush), Self::response_buffer()]) else { return false; };
        // SAFETY: command buffers and queue remain valid until completion.
        unsafe { transport.notify(0) };
        self.wait_completion(transport, head).is_some() && Self::read_response().cmd_type == CMD_RESP_OK_NODATA
    }

    fn release_resource(&mut self, transport: &mut dyn Transport, resource_id: u32) {
        #[repr(C)]
        struct Unref { hdr: CtrlHeader, resource_id: u32, padding: u32 }
        let request = Unref { hdr: Self::header(CMD_RESOURCE_UNREF), resource_id, padding: 0 };
        if let Some(head) = self.ctrl_queue.add_chain(&[Self::scratch_buffer(request), Self::response_buffer()]) {
            unsafe { transport.notify(0); }
            let _ = self.wait_completion(transport, head);
        }
    }

    pub fn display_geometry(&self) -> Option<(u32, u32)> {
        if self.resource_id == 0 {
            None
        } else {
            Some((self.width, self.height))
        }
    }

    pub fn transport_index(&self) -> usize {
        self.transport_index
    }

    fn header(cmd_type: u32) -> CtrlHeader {
        CtrlHeader {
            cmd_type,
            flags: 0,
            fence_id: 0,
            ctx_id: 0,
            ring_idx: 0,
        }
    }

    fn scratch_buffer<T>(value: T) -> QueueBuffer {
        assert!(core::mem::size_of::<T>() <= core::mem::size_of::<CommandScratch>());
        let dst = GPU_COMMAND.0.get().cast::<T>();
        unsafe { core::ptr::write_volatile(dst, value) };
        QueueBuffer::new(
            GPU_COMMAND.0.get().cast::<u8>(),
            core::mem::size_of::<T>(),
            false,
        )
    }

    fn aux_buffer<T>(value: T) -> QueueBuffer {
        assert!(core::mem::size_of::<T>() <= core::mem::size_of::<CommandScratch>());
        let dst = GPU_AUX_COMMAND.0.get().cast::<T>();
        unsafe { core::ptr::write_volatile(dst, value) };
        QueueBuffer::new(
            GPU_AUX_COMMAND.0.get().cast::<u8>(),
            core::mem::size_of::<T>(),
            false,
        )
    }

    fn response_buffer() -> QueueBuffer {
        Self::clear_response();
        QueueBuffer::new(
            GPU_RESPONSE.0.get() as *mut u8,
            core::mem::size_of::<CtrlHeader>(),
            true,
        )
    }

    fn read_response() -> CtrlHeader {
        unsafe { core::ptr::read_volatile(GPU_RESPONSE.0.get()) }
    }

    fn clear_response() {
        unsafe {
            core::ptr::write_volatile(
                GPU_RESPONSE.0.get(),
                CtrlHeader {
                    cmd_type: 0,
                    flags: 0,
                    fence_id: 0,
                    ctx_id: 0,
                    ring_idx: 0,
                },
            );
        }
    }

    fn wait_completion(&mut self, transport: &mut dyn Transport, expected_head: u16) -> Option<()> {
        let mut timeout = 0u64;
        loop {
            if let Some((head_idx, len)) = self.ctrl_queue.pop_used() {
                if head_idx == expected_head && len >= core::mem::size_of::<CtrlHeader>() {
                    return Some(());
                }
                // A malformed completion cannot be treated as a successful frame.
                unsafe { transport.reset(); }
                self.failed = true;
                return None;
            }
            timeout += 1;
            if timeout > 50_000_000 {
                crate::hal::serial::write_string_always("[GPU] completion timeout; resetting device\r\n");
                unsafe { transport.reset(); }
                self.failed = true;
                return None;
            }
            core::hint::spin_loop();
        }
    }
}

/// Raw device pointers are only touched behind a single owner lock.
pub struct GpuSession {
    pub gpu: VirtioGpu,
    pub transport: super::pci::PciTransport,
}

unsafe impl Send for GpuSession {}
