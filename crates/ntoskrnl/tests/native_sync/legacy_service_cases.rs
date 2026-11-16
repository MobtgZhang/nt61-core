//! Complete original entry bodies, independent of the unimplemented LPC/ALPC
//! message/connection backend. Ring3 separately verifies the SSDT adapters.
use crate::ntdll::legacy_services::*;

#[test]
fn complete_connect_port_ignores_all_handle_bits_without_touching_memory() {
    let mut sentinel = [0x6a09e667u32; 32];
    let pointer = sentinel.as_mut_ptr() as usize;
    for handle in [0, 1, 4, usize::MAX, 0x8000_0000, 0xffff_ffff, 1usize << 32, pointer] {
        assert_eq!(nt_complete_connect_port(handle as _), 0);
        assert_eq!(sentinel, [0x6a09e667; 32]);
    }
}

#[test]
fn unconditional_legacy_bodies_keep_native_result_widths() {
    assert_eq!(nt_query_port_information_process(), 1u8);
    assert_eq!(nt_set_ldt_entries(0, 0, 0, 0, 0, 0) as u32, 0xc0000002);
    assert_eq!(nt_set_ldt_entries(u32::MAX, 1, 2, 3, 4, u32::MAX) as u32, 0xc0000002);
}
