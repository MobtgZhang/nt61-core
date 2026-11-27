pub const DEFAULT_SECURITY_ID: u32 = 256;
pub const ADMINISTRATORS: [u8; 16] = [1, 2, 0, 0, 0, 0, 0, 5, 32, 0, 0, 0, 32, 2, 0, 0];
pub const DACL_LENGTH: usize = 72;

pub fn default_dacl() -> [u8; DACL_LENGTH] {
    let system = [1, 1, 0, 0, 0, 0, 0, 5, 18, 0, 0, 0];
    let everyone = [1, 1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0];
    let entries: [(&[u8], u32); 3] = [
        (&system, 0x1f_01ff),
        (&ADMINISTRATORS, 0x1f_01ff),
        (&everyone, 0x12_00a9),
    ];
    let mut acl = [0; DACL_LENGTH];
    acl[0] = 2;
    acl[2] = DACL_LENGTH as u8;
    acl[4] = entries.len() as u8;
    let mut offset = 8;
    for (sid, access) in entries {
        let size = 8 + sid.len();
        acl[offset + 2..offset + 4].copy_from_slice(&(size as u16).to_le_bytes());
        acl[offset + 4..offset + 8].copy_from_slice(&access.to_le_bytes());
        acl[offset + 8..offset + size].copy_from_slice(sid);
        offset += size;
    }
    acl
}

pub fn sds_hash(descriptor: &[u8]) -> u32 {
    descriptor.chunks(4).fold(0u32, |sum, bytes| {
        let mut word = [0; 4];
        word[..bytes.len()].copy_from_slice(bytes);
        sum.rotate_left(3).wrapping_add(u32::from_le_bytes(word))
    })
}
