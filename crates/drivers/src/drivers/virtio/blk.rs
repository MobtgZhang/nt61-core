//! virtio-blk block device driver (virtio spec 4.1; Linux
//! include/uapi/linux/virtio_blk.h).
//!
//! One request virtqueue carries (header, data, status) descriptor
//! chains. The header carries the operation type (IN/OUT/FLUSH), a
//! priority (always zero) and the starting sector (512-byte units).

use super::queue::QueueBuffer;

use super::{status, Transport, FEATURE_VERSION_1};

// virtio_blk.h feature bits.
pub const F_SIZE_MAX: u64 = 1 << 1;
pub const F_SEG_MAX: u64 = 1 << 2;
pub const F_RO: u64 = 1 << 5;
pub const F_BLK_SIZE: u64 = 1 << 6;
pub const F_FLUSH: u64 = 1 << 9;

// virtio_blk.h operation types (VIRTIO_BLK_T_*).
pub const TYPE_IN: u32 = 0;
pub const TYPE_OUT: u32 = 1;
pub const TYPE_FLUSH: u32 = 4;

// virtio_blk.h S_OK / S_IOERR status bytes.
pub const S_OK: u8 = 0;
pub const S_IOERR: u8 = 1;

// Device-specific configuration offsets (virtio_blk.h virtio_blk_config).
pub const CFG_CAPACITY: u64 = 0;
pub const CFG_SIZE_MAX: u64 = 8;
pub const CFG_SEG_MAX: u64 = 12;
pub const CFG_BLK_SIZE: u64 = 16;

/// Standard 512-byte logical sector.
pub const SECTOR_SIZE: u32 = 512;

/// Maximum number of virtio-blk devices tracked by the dispatch layer.
pub const MAX_DISPATCH_DEVICES: usize = 8;

/// virtio_blk_req_header: one request descriptor's header.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RequestHeader {
    pub req_type: u32,
    pub ioprio: u32,
    pub sector: u64,
}

/// One configured virtio-blk device.
pub struct VirtioBlk {
    transport_index: usize,
    queue: super::queue::QueueLayout,
    capacity_sectors: u64,
    /// Set of feature bits negotiated with the device.
    features: u64,
    /// Dedicated identity-mapped DMA page: request header, one sector, status.
    /// It remains allocated for the device lifetime, including after a timeout.
    dma: u64,
    failed: bool,
}

impl VirtioBlk {
    /// # Safety
    ///
    /// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
    /// Bring up a virtio-blk device over the given transport.
    /// Performs the standard status negotiation sequence
    /// (ACKNOWLEDGE -> DRIVER -> FEATURES_OK -> DRIVER_OK).
    pub unsafe fn probe(
        transport_index: usize,
        transport: &mut dyn Transport,
    ) -> Option<VirtioBlk> {
        crate::hal::serial::write_string_always("[VBLK] reset enter\r\n");
        transport.reset();
        crate::hal::serial::write_string_always("[VBLK] reset done\r\n");
        if transport.read_status() != 0 {
            return None;
        }
        transport.write_status(status::ACKNOWLEDGE);
        transport.write_status(status::ACKNOWLEDGE | status::DRIVER);

        let device_features = transport.read_device_features();
        crate::hal::serial::write_string_always("[VBLK] features read\r\n");
        // Negotiate the base spec version plus the features the driver
        // understands. Only F_FLUSH and F_BLK_SIZE influence our I/O
        // path; everything else is optional and left unset.
        let features =
            device_features & (FEATURE_VERSION_1 | F_FLUSH | F_BLK_SIZE | F_RO);
        transport.write_driver_features(features);
        transport.write_status(status::ACKNOWLEDGE | status::DRIVER | status::FEATURES_OK);
        crate::hal::serial::write_string_always("[VBLK] features ok written\r\n");

        let current = transport.read_status();
        crate::hal::serial::write_string_always("[VBLK] status read\r\n");
        if current & status::FAILED != 0 || current & status::FEATURES_OK == 0 {
            return None;
        }

        let max = transport.max_queue_size(0);
        crate::hal::serial::write_string_always("[VBLK] max read\r\n");
        if max < 3 { return None; }
        let queue = super::queue::QueueLayout::allocate(1u16 << (31 - max.min(128).leading_zeros()))?;
        crate::hal::serial::write_string_always("[VBLK] queue allocated\r\n");
        if !transport.setup_queue(0, &queue) {
            return None;
        }
        crate::hal::serial::write_string_always("[VBLK] queue setup\r\n");

        transport.write_status(
            status::ACKNOWLEDGE | status::DRIVER | status::FEATURES_OK | status::DRIVER_OK,
        );

        let capacity = transport.config_read_u64(CFG_CAPACITY);
        crate::hal::serial::write_string_always("[VBLK] capacity read\r\n");
        let dma=crate::mm::allocate_pages(1)?;
        if !crate::arch::identity_map_region(dma & !0x1f_ffff,0x20_0000){crate::mm::free_pages(dma,1);return None;}
        core::ptr::write_bytes(dma as *mut u8,0,4096);
        Some(VirtioBlk {
            transport_index,
            queue,
            capacity_sectors: capacity,
            features, dma, failed:false,
        })
    }

    pub fn capacity_sectors(&self) -> u64 {
        self.capacity_sectors
    }

    pub fn has_flush(&self) -> bool {
        self.features & F_FLUSH != 0
    }

    pub fn has_blk_size(&self) -> bool {
        self.features & F_BLK_SIZE != 0
    }

    /// # Safety
    ///
    /// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
    /// Read one or more 512-byte sectors into buffer.
    pub unsafe fn read_sectors(
        &mut self,
        transport: &mut dyn Transport,
        lba: u64,
        buffer: &mut [u8],
    ) -> bool {
        self.submit(transport, TYPE_IN, lba, buffer).is_some()
    }

    /// # Safety
    ///
    /// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
    /// Write one or more 512-byte sectors from buffer.
    pub unsafe fn write_sectors(
        &mut self,
        transport: &mut dyn Transport,
        lba: u64,
        buffer: &[u8],
    ) -> bool {
        self.submit_write(transport, TYPE_OUT, lba, buffer)
            .is_some()
    }

    /// # Safety
    ///
    /// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
    /// Flush the device cache.
    pub unsafe fn flush(&mut self, transport: &mut dyn Transport) -> bool {
        self.submit_flush(transport).is_some()
    }

    /// A page-owned bounce buffer avoids handing noncontiguous or temporary
    /// caller memory to DMA. The owner serializes requests on this queue.
    fn request(&mut self,transport:&mut dyn Transport,kind:u32,lba:u64)->Option<()> {
        if self.failed || (kind==TYPE_OUT && self.features & F_RO!=0){return None;}
        unsafe {
            let base=self.dma as *mut u8;
            core::ptr::write_volatile(base as *mut RequestHeader,RequestHeader{req_type:kind,ioprio:0,sector:lba});
            core::ptr::write_volatile(base.add(528),S_IOERR);
            let header=QueueBuffer::new(base,16,false);
            let data=QueueBuffer::new(base.add(16),512,kind==TYPE_IN);
            let status=QueueBuffer::new(base.add(528),1,true);
            let head=if kind==TYPE_FLUSH{self.queue.add_chain(&[header,status])}
                else{self.queue.add_chain(&[header,data,status])}?;
            core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
            transport.notify(0);
            for _ in 0..10_000_000 {
                if let Some((completed,length))=self.queue.pop_used(){
                    core::sync::atomic::fence(core::sync::atomic::Ordering::Acquire);
                    if completed!=head || length<if kind==TYPE_IN{513}else{1}{self.failed=true;transport.reset();return None;}
                    return (core::ptr::read_volatile(base.add(528))==S_OK).then_some(());
                }
                core::hint::spin_loop();
            }
            // Do not free or reuse the DMA page while a delayed device could
            // still own descriptors. Subsequent requests fail explicitly.
            self.failed=true;transport.reset();None
        }
    }
    fn valid_range(&self,lba:u64,len:usize)->bool{
        len.is_multiple_of(512) && lba.checked_add((len/512)as u64).is_some_and(|end|end<=self.capacity_sectors)
    }
    fn submit(&mut self,transport:&mut dyn Transport,kind:u32,lba:u64,buffer:&mut[u8])->Option<usize>{
        if !self.valid_range(lba,buffer.len()){return None;}
        for (i,sector) in buffer.chunks_exact_mut(512).enumerate(){
            self.request(transport,kind,lba+i as u64)?;
            unsafe{core::ptr::copy_nonoverlapping((self.dma+16)as *const u8,sector.as_mut_ptr(),512);}
        }
        Some(buffer.len())
    }
    fn submit_write(&mut self,transport:&mut dyn Transport,kind:u32,lba:u64,buffer:&[u8])->Option<usize>{
        if !self.valid_range(lba,buffer.len()){return None;}
        for (i,sector) in buffer.chunks_exact(512).enumerate(){
            unsafe{core::ptr::copy_nonoverlapping(sector.as_ptr(),(self.dma+16)as *mut u8,512);}
            self.request(transport,kind,lba+i as u64)?;
        }
        Some(buffer.len())
    }
    fn submit_flush(&mut self,transport:&mut dyn Transport)->Option<usize>{
        if !self.has_flush(){return None;}self.request(transport,TYPE_FLUSH,0)?;Some(0)
    }

    pub fn transport_index(&self) -> usize {
        self.transport_index
    }
}
