//! MakeAbsoluteSD converts checked guest data without allocating host objects.
//! Contract: https://learn.microsoft.com/windows/win32/api/securitybaseapi/nf-securitybaseapi-makeabsolutesd
//! Win7 SP1 AMD64 ntdll SHA256
//! 91e3d98ad3119e8addf8d2aa1dd6795162842fff7101e4c70c5137e847b4ff50:
//! RtlSelfRelativeToAbsoluteSD ordinal 1150, RVA 0xcb50; the size-writeback
//! failure branch is at 0xcd11. Successful calls preserve supplied capacities.
use super::super::{put, read, word};
use super::security::descriptor;
use crate::arch::common::btl::{ir::DecodeMode, memory::GuestMemory, pe::GuestImage};

pub fn invoke(image: &mut GuestImage, name: &str, a: &[u64]) -> Option<Result<u64, u32>> {
    if name != "MakeAbsoluteSD" {
        return None;
    }
    Some((|| {
        if a.len() < 11 {
            return Err(87);
        }
        if a[0] == 0 {
            return Err(1338);
        }
        // Reject absolute descriptors before following their pointer fields.
        let h = read(&mut image.memory, a[0], 4)?;
        if u16::from_le_bytes([h[2], h[3]]) & 0x8000 == 0 {
            return Err(1361); // ERROR_BAD_DESCRIPTOR_FORMAT
        }
        let descriptor = descriptor(image, a[0])?;
        let width = if image.thread.context.mode == DecodeMode::X86 {
            4
        } else {
            8
        };
        let first = if width == 8 { 8 } else { 4 };
        let header_size = first + 4 * width;
        let required = [
            header_size,
            descriptor.blobs[3].len(),
            descriptor.blobs[2].len(),
            descriptor.blobs[0].len(),
            descriptor.blobs[1].len(),
        ];
        let outputs = [a[1], a[3], a[5], a[7], a[9]];
        let sizes = [a[2], a[4], a[6], a[8], a[10]];
        let mut short = outputs[0] == 0;
        for index in 0..5 {
            let capacity = word(&mut image.memory, sizes[index], 4)?;
            short |= capacity < required[index] as u64;
        }
        if short {
            // Resolve every writable size pointer before changing any caller
            // memory. A bad guest pointer cannot leave a half-written result.
            for size in sizes {
                if size == 0 || !image.memory.probe_write(size, 4) {
                    return Err(998);
                }
            }
            for index in 0..5 {
                put(&mut image.memory, sizes[index], required[index] as u64, 4)?;
            }
            return Err(122);
        }
        for index in 0..5 {
            if required[index] != 0
                && (outputs[index] == 0
                    || !image.memory.probe_write(outputs[index], required[index]))
            {
                return Err(998);
            }
        }
        // The shipped x64 routine copies the 20-byte relative header before
        // replacing all pointer slots, preserving bytes 4..8 as ABI padding.
        let mut header = read(&mut image.memory, a[0], 20)?;
        header.resize(header_size, 0);
        header[2..4].copy_from_slice(&(descriptor.control & !0x8000).to_le_bytes());
        for (index, output) in [a[7], a[9], a[5], a[3]].iter().enumerate() {
            let pointer = if descriptor.blobs[index].is_empty() {
                0
            } else {
                *output
            };
            let start = first + index * width;
            header[start..start + width].copy_from_slice(&pointer.to_le_bytes()[..width]);
        }
        // Snapshotting happened during descriptor decoding, so every component
        // is validated before the first destination write.
        for (index, output) in [a[7], a[9], a[5], a[3]].iter().enumerate() {
            if !descriptor.blobs[index].is_empty()
                && !image.memory.write_virt(*output, &descriptor.blobs[index])
            {
                return Err(998);
            }
        }
        if !image.memory.write_virt(a[1], &header) {
            return Err(998);
        }
        Ok(1)
    })())
}
