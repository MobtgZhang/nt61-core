//! Win7 version comparisons for the isolated process's compatibility identity.
use super::*;

const OLD_VERSION: u32 = 1150;
const BAD_ARGUMENTS: u32 = 160;

fn compare(current: u64, requested: u64, condition: u8) -> Result<(), ApiError> {
    let matches = match condition {
        1 => current == requested,
        2 => current > requested,
        3 => current >= requested,
        4 => current < requested,
        5 => current <= requested,
        _ => false,
    };
    if matches { Ok(()) } else { Err(OLD_VERSION.into()) }
}

fn hierarchy_condition(inherited: &mut u8, requested: u8) -> u8 {
    // Lower-order fields may refine an equal condition or a comparison in
    // the same direction. Conflicting directions inherit the prior one.
    // A missing condition freezes inheritance for subsequent fields.
    let compatible = matches!((*inherited, requested),
        (1, 1..=5) | (2 | 3, 1..=3) | (4 | 5, 1 | 4 | 5));
    if *inherited == 0 || (*inherited == 1 && compatible) {
        *inherited = requested;
    }
    if compatible { return requested; }
    if requested == 0 { *inherited |= 16; }
    *inherited & 15
}

pub(super) fn verify(
    image: &mut GuestImage,
    pointer: u64,
    types: u32,
    mask: u64,
) -> Result<u64, ApiError> {
    if types == 0 || mask == 0 { return Err(BAD_ARGUMENTS.into()); }
    let condition = |bit: u32| ((mask >> (bit * 3)) & 7) as u8;
    let mut field = |offset: u64, width| {
        let address = pointer.checked_add(offset).ok_or(ApiError::MemoryFault)?;
        word(&mut image.memory, address, width).map_err(|_| ApiError::MemoryFault)
    };
    // Product, suite, platform and build are independent constraints. Match
    // the loader's order so invalid conditions and mismatches keep priority.
    if types & 0x80 != 0 { compare(1, field(282, 1)?, condition(7))?; }
    if types & 0x40 != 0 {
        let requested = field(280, 2)?;
        let matches = match condition(6) {
            6 => requested & 0x100 == requested,
            7 => requested == 0 || requested & 0x100 != 0,
            _ => return Err(BAD_ARGUMENTS.into()),
        };
        if !matches { return Err(OLD_VERSION.into()); }
    }
    if types & 8 != 0 { compare(2, field(16, 4)?, condition(3))?; }
    if types & 4 != 0 { compare(7601, field(12, 4)?, condition(2))?; }
    let mut inherited = 0;
    let mut result = Ok(());
    // Major, minor and service-pack fields form one ordered comparison;
    // a decisive earlier field suppresses subsequent version checks.
    for (bit, offset, width, current) in [(1,4,4,6), (0,8,4,1), (5,276,2,1), (4,278,2,0)] {
        if types & (1 << bit) == 0 { continue; }
        let requested = field(offset, width)?;
        let op = hierarchy_condition(&mut inherited, condition(bit));
        result = compare(current, requested, op);
        if current != requested || !(1..=5).contains(&op) { break; }
    }
    result?;
    Ok(1)
}
