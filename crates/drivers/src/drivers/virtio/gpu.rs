//! virtio-gpu display driver (virtio spec 4.4).
//!
//! The boot display keeps one resource and its backing guest memory
//! alive. Blob-capable devices scan out the guest backing directly and
//! frame presentation only submits `RESOURCE_FLUSH`. Legacy devices use
//! the 2D transfer path. Resources and queues are never recreated per
//! frame.

use super::{queue::QueueBuffer, status, Transport, FEATURE_VERSION_1};

use core::cell::UnsafeCell;

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
const _: () = assert!(core::mem::size_of::<ResourceCreate2d>() == 40);
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
        let features = FEATURE_VERSION_1 | (device_features & F_RESOURCE_BLOB);
        transport.write_driver_features(features);
        transport.write_status(status::ACKNOWLEDGE | status::DRIVER | status::FEATURES_OK);
        if transport.read_status() & status::FAILED != 0 {
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
        if width == 0 || height == 0 || fb_addr.is_null() {
            return false;
        }

        if self.blob_supported {
            return self.initialize_display_blob(transport, scanout_id, fb_addr, width, height);
        }
        self.initialize_display_2d(transport, scanout_id, fb_addr, width, height)
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
        if self.wait_completion(head).is_none() {
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
        crate::hal::serial::write_string_always("[GPU] scanout blob size=0x");
        crate::hal::serial::write_hex_u64_always(core::mem::size_of::<SetScanoutBlob>() as u64);
        crate::hal::serial::write_string_always("\r\n");
        let scanout_buffer = Self::scratch_buffer(scanout);
        Self::clear_response();
        crate::hal::serial::write_string_always("[GPU] scanout buffer len=0x");
        crate::hal::serial::write_hex_u64_always(scanout_buffer.len as u64);
        crate::hal::serial::write_string_always("\r\n");
        let Some(head) = self
            .ctrl_queue
            .add_chain(&[scanout_buffer, Self::response_buffer()])
        else {
            crate::hal::serial::write_string_always("[GPU] blob scanout add_chain failed\r\n");
            return false;
        };
        let (desc_addr, desc_len, desc_flags, desc_next) = self.ctrl_queue.debug_descriptor(head);
        crate::hal::serial::write_string_always("[GPU] scanout desc addr=0x");
        crate::hal::serial::write_hex_u64_always(desc_addr);
        crate::hal::serial::write_string_always(" len=0x");
        crate::hal::serial::write_hex_u64_always(desc_len as u64);
        crate::hal::serial::write_string_always(" flags=0x");
        crate::hal::serial::write_hex_u64_always(desc_flags as u64);
        crate::hal::serial::write_string_always(" next=0x");
        crate::hal::serial::write_hex_u64_always(desc_next as u64);
        crate::hal::serial::write_string_always("\r\n");
        let (next_addr, next_len, next_flags, next_next) =
            self.ctrl_queue.debug_descriptor(desc_next);
        crate::hal::serial::write_string_always("[GPU] scanout next desc addr=0x");
        crate::hal::serial::write_hex_u64_always(next_addr);
        crate::hal::serial::write_string_always(" len=0x");
        crate::hal::serial::write_hex_u64_always(next_len as u64);
        crate::hal::serial::write_string_always(" flags=0x");
        crate::hal::serial::write_hex_u64_always(next_flags as u64);
        crate::hal::serial::write_string_always(" next=0x");
        crate::hal::serial::write_hex_u64_always(next_next as u64);
        crate::hal::serial::write_string_always("\r\n");
        unsafe { transport.notify(0) };
        if self.wait_completion(head).is_none() {
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
        if self.wait_completion(head).is_none() {
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
        crate::hal::serial::write_string_always("[GPU] backing va=0x");
        crate::hal::serial::write_hex_u64_always(fb_addr as u64);
        crate::hal::serial::write_string_always(" pa=0x");
        crate::hal::serial::write_hex_u64_always(backing_phys);
        crate::hal::serial::write_string_always(" size=");
        crate::hal::serial::write_hex_u64_always((width * height * 4).into());
        crate::hal::serial::write_string_always("\r\n");
        let mut backing = ResourceMemoryEntry {
            addr: backing_phys,
            size: width * height * 4,
            _padding: 0,
        };
        let mut attach = ResourceAttachBacking {
            hdr: Self::header(CMD_RESOURCE_ATTACH_BACKING),
            resource_id,
            num_entries: 1,
        };
        let Some(head) = self.ctrl_queue.add_chain(&[
            Self::read_buffer(&mut attach),
            Self::read_buffer(&mut backing),
            Self::response_buffer(),
        ]) else {
            crate::hal::serial::write_string_always("[GPU] attach add_chain failed\r\n");
            return false;
        };
        unsafe { transport.notify(0) };
        if self.wait_completion(head).is_none() {
            crate::hal::serial::write_string_always("[GPU] attach completion failed\r\n");
            return false;
        }
        let response = Self::read_response();
        if response.cmd_type != CMD_RESP_OK_NODATA {
            crate::hal::serial::write_string_always("[GPU] attach response failed\r\n");
            return false;
        }

        let mut scanout = SetScanout {
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
            .add_chain(&[Self::read_buffer(&mut scanout), Self::response_buffer()])
        else {
            crate::hal::serial::write_string_always("[GPU] scanout add_chain failed\r\n");
            return false;
        };
        unsafe { transport.notify(0) };
        if self.wait_completion(head).is_none() {
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
        if self.resource_id == 0 {
            return false;
        }

        if self.blob_supported {
            return self.flush_display(transport);
        }

        let transfer = TransferToHost2d {
            hdr: Self::header(CMD_TRANSFER_TO_HOST_2D),
            rect_x: 0,
            rect_y: 0,
            rect_width: self.width,
            rect_height: self.height,
            offset: 0,
            resource_id: self.resource_id,
            _padding: 0,
        };
        let Some(head) = self
            .ctrl_queue
            .add_chain(&[Self::scratch_buffer(transfer), Self::response_buffer()])
        else {
            return false;
        };
        unsafe { transport.notify(0) };
        if self.wait_completion(head).is_none() {
            crate::hal::serial::write_string_always("[GPU] transfer completion failed\r\n");
            return false;
        }
        let response = Self::read_response();
        if response.cmd_type != CMD_RESP_OK_TRANSFER_2D {
            crate::hal::serial::write_string_always("[GPU] transfer response failed ok=0x");
            crate::hal::serial::write_hex_u64_always(CMD_RESP_OK_TRANSFER_2D as u64);
            crate::hal::serial::write_string_always(" got=0x");
            crate::hal::serial::write_hex_u64_always(response.cmd_type as u64);
            crate::hal::serial::write_string_always("\r\n");
            return false;
        }

        let flush = ResourceFlush {
            hdr: Self::header(CMD_RESOURCE_FLUSH),
            rect_x: 0,
            rect_y: 0,
            rect_width: self.width,
            rect_height: self.height,
            resource_id: self.resource_id,
            _padding: 0,
        };
        let Some(head) = self
            .ctrl_queue
            .add_chain(&[Self::scratch_buffer(flush), Self::response_buffer()])
        else {
            return false;
        };
        unsafe { transport.notify(0) };
        if self.wait_completion(head).is_none() {
            crate::hal::serial::write_string_always("[GPU] flush completion failed\r\n");
            return false;
        }
        let response = Self::read_response();
        if response.cmd_type != CMD_RESP_OK_NODATA {
            crate::hal::serial::write_string_always("[GPU] flush response failed\r\n");
            return false;
        }
        true
    }

    fn flush_display(&mut self, transport: &mut dyn Transport) -> bool {
        let flush = ResourceFlush {
            hdr: Self::header(CMD_RESOURCE_FLUSH),
            rect_x: 0,
            rect_y: 0,
            rect_width: self.width,
            rect_height: self.height,
            resource_id: self.resource_id,
            _padding: 0,
        };
        let Some(head) = self
            .ctrl_queue
            .add_chain(&[Self::scratch_buffer(flush), Self::response_buffer()])
        else {
            return false;
        };
        unsafe { transport.notify(0) };
        if self.wait_completion(head).is_none() {
            return false;
        }
        Self::read_response().cmd_type == CMD_RESP_OK_NODATA
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

    fn read_buffer<T>(value: &mut T) -> QueueBuffer {
        QueueBuffer::new(value as *mut T as *mut u8, core::mem::size_of::<T>(), false)
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

    fn wait_completion(&mut self, expected_head: u16) -> Option<()> {
        let mut timeout = 0u64;
        loop {
            if let Some((head_idx, _)) = self.ctrl_queue.pop_used() {
                if head_idx == expected_head {
                    return Some(());
                }
            }
            timeout += 1;
            if timeout > 50_000_000 {
                crate::hal::serial::write_string_always("[GPU] completion timeout\r\n");
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
