//! RTC century discovery uses the same bounded ACPI reader as other HAL paths.
extern crate alloc;
use super::reader;
use alloc::vec::Vec;

/// FADT byte 108 advertises the CMOS century index; zero means absent.
/// Standard RTC/control bytes 0..0x0d and indices above 0x7f are excluded.
pub(super) fn find_century_register(
    mut read: impl FnMut(u64, usize) -> Option<Vec<u8>>,
    rsdp: u64,
) -> Option<u8> {
    for (address, stride) in reader::roots(&mut read, rsdp)? {
        let Some(root) = reader::table(&mut read, address) else {
            continue;
        };
        let Some(entries) = reader::root_entries(&root, stride) else {
            continue;
        };
        for physical in entries {
            let Some(header) = read(physical, reader::HEADER_SIZE) else {
                continue;
            };
            if header.get(..4) != Some(b"FACP") {
                continue;
            }
            let Some(fadt) = reader::table(&mut read, physical) else {
                continue;
            };
            let index = *fadt.get(108)?;
            return (0x0e..=0x7f).contains(&index).then_some(index);
        }
    }
    None
}
