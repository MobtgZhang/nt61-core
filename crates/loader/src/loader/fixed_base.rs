//! Address policy for the fixed-address RISC-V user-image path.

/// Keep the linked base only when the complete image fits in Sv39's low
/// canonical half and does not occupy the null page. No alternate base is
/// returned: this loader cannot relocate arbitrary RISC-V fixed images.
pub(super) fn riscv64_user_image_base(preferred: u64, size: u64) -> Option<u64> {
    if preferred < 0x1000 || size == 0 || preferred.checked_add(size)? > (1u64 << 38) {
        return None;
    }
    Some(preferred)
}
