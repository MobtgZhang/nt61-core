//! LoongArch native leaf format, shared with host permission tests.
pub(super) fn encode(pa: u64, writable: bool, user: bool, nx: bool) -> u64 {
    let mut pte = (pa & 0x0000_ffff_ffff_f000)
        | 1 // V
        | (1 << 4) // coherent cached memory
        | (1 << 7); // software present
    if writable {
        // Hardware store permission is D, while W is software metadata.
        // A read-only page must leave both clear.
        pte |= 2 | (1 << 8) | (1 << 10);
    }
    if nx {
        pte |= 1 << 62;
    }
    if user {
        pte |= 3 << 2; // PLV3; leave G clear to retain ASID ownership
    } else {
        pte |= 1 << 6; // shared kernel translations may be global
    }
    pte
}
