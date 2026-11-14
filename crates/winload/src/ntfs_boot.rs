//! Minimal NTFS boot-sector reader for winload.efi.
//!
//! UEFI's `SimpleFileSystem` protocol only supports FAT-family volumes by
//! default. When the System partition is formatted as NTFS, winload falls
//! back to this in-tree reader to resolve `\Windows\System32\...` paths.
//!
//! The design mirrors the NTFS reader in the boot manager (`boot/src/main.rs`).
//! Both walk the MFT via the `$INDEX_ROOT` attribute to resolve path
//! components. The only OS-specific dependency is `BlockIO` for sector I/O,
//! which is available in both bootmgr and winload.

use alloc::vec::Vec;
use uefi::boot::OpenProtocolAttributes;
use uefi::boot::OpenProtocolParams;
use uefi::proto::media::block::BlockIO;

static mut CACHED_NTFS_BLOCK_IO_PTR: usize = 0;
static mut CACHED_NTFS_BLOCK_IO_IFACE: *const core::ffi::c_void = core::ptr::null();
static mut NTFS_BLOCK_IO_READY: bool = false;

/// read `count` sectors starting at `lba` from the given BlockIO handle.
fn read_partition_sectors(
    block_io: &uefi::proto::media::block::BlockIO,
    media_id: u32,
    start_lba: u64,
    count: u32,
) -> Option<Vec<u8>> {
    if block_io.media().block_size() != 512 {
        return None;
    }
    crate::block_read::read_sectors(
        start_lba,
        count,
        block_io.media().io_align() as usize,
        |lba, buffer| block_io.read_blocks(media_id, lba, buffer).ok(),
    )
}

/// read a file from the NTFS System partition by MFT path.
/// Returns `None` if the path cannot be resolved (NTFS not detected,
/// path not found, or I/O error).
pub fn read_ntfs_system_file(path: &str) -> Option<Vec<u8>> {
    use uefi::boot as ub;

    uefi::println!("[NTFS-BOOT] read begin {}", path);
    let is_ready = unsafe { core::ptr::addr_of!(NTFS_BLOCK_IO_READY).read_volatile() };
    if !is_ready {
        let handles = ub::find_handles::<BlockIO>().ok()?;
        let mut selected_ptr = 0usize;
        for handle in handles.iter() {
            let block = match unsafe {
                ub::open_protocol::<BlockIO>(
                    OpenProtocolParams {
                        handle: *handle,
                        agent: ub::image_handle(),
                        controller: None,
                    },
                    OpenProtocolAttributes::GetProtocol,
                )
            } {
                Ok(block) => block,
                Err(_) => continue,
            };
            let block_ref = match block.get() {
                Some(block_ref) => block_ref,
                None => {
                    drop(block);
                    continue;
                }
            };
            let media = block_ref.media();
            let mut boot_sector = [0u8; 512];
            if media.block_size() != 512
                || !media.is_logical_partition()
                || block_ref
                    .read_blocks(media.media_id(), 0u64, &mut boot_sector)
                    .is_err()
                || &boot_sector[3..11] != b"NTFS    "
            {
                drop(block);
                continue;
            }

            selected_ptr = handle.as_ptr() as usize;
            unsafe {
                core::ptr::addr_of_mut!(CACHED_NTFS_BLOCK_IO_IFACE)
                    .write_volatile(block_ref as *const BlockIO as *const core::ffi::c_void);
            }
            core::mem::forget(block);
            break;
        }
        unsafe {
            core::ptr::addr_of_mut!(CACHED_NTFS_BLOCK_IO_PTR).write_volatile(selected_ptr);
            core::ptr::addr_of_mut!(NTFS_BLOCK_IO_READY).write_volatile(selected_ptr != 0);
        }
        if selected_ptr == 0 {
            return None;
        }
        uefi::println!("[NTFS-BOOT] cache ready ptr=0x{:x}", selected_ptr);
    }

    uefi::println!("[NTFS-BOOT] using cached BlockIO {}", path);
    let block_ref = unsafe { &*(CACHED_NTFS_BLOCK_IO_IFACE as *const BlockIO) };
    struct Reader<'a>(&'a BlockIO);
    impl nt61_bootfs::SectorReader for Reader<'_> {
        fn read_sectors(&self, lba: u64, count: u32) -> Option<Vec<u8>> {
            read_partition_sectors(self.0, self.0.media().media_id(), lba, count)
        }
    }
    nt61_bootfs::ntfs::read_file(&Reader(block_ref), path)
}
