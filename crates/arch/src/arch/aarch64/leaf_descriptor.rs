//! AArch64 stage-1 4 KiB leaf attributes, shared with host format tests.
const USER: u64 = 1 << 6;
const READONLY: u64 = 1 << 7;
const NOT_GLOBAL: u64 = 1 << 11;
const PXN: u64 = 1 << 53;
const UXN: u64 = 1 << 54;

pub(super) fn attributes(user: bool, writable: bool, nx: bool) -> u64 {
    // Normal write-back memory (MAIR index 2), inner shareable, AF set.
    let mut attrs = 3 | (2 << 2) | (3 << 8) | (1 << 10);
    if user {
        // User translations belong to this ASID, never all processes.
        attrs |= USER | PXN | NOT_GLOBAL;
    }
    if !writable {
        attrs |= READONLY;
    }
    if nx {
        attrs |= PXN | UXN;
    } else if !user {
        attrs |= UXN;
    }
    attrs
}

pub(super) fn encode(pa: u64, attributes: u64) -> u64 {
    // Keep PXN/UXN above the address field. Masking attributes to 12 bits
    // silently grants execution to user data and kernel heap pages.
    (pa & 0x0000_ffff_ffff_f000) | (attributes & (0xfff | PXN | UXN)) | 3 | (1 << 10)
}
