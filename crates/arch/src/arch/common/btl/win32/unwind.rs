//! Bounded AMD64 version-1 virtual unwind. This restores a caller context;
//! it neither invokes language handlers nor dispatches or raises exceptions.
use super::*;

const CONTEXT_SIZE: usize = 1232;
const SP: usize = 0x98;
const PC: usize = 0xf8;

fn add(address: u64, offset: u64) -> Result<u64, ApiError> {
    address.checked_add(offset).ok_or(ApiError::MemoryFault)
}
fn read<const N: usize>(image: &mut GuestImage, address: u64) -> Result<[u8; N], ApiError> {
    let mut bytes = [0; N];
    if image.memory.read_virt(address, &mut bytes) != N {
        return Err(ApiError::MemoryFault);
    }
    Ok(bytes)
}
fn qword(image: &mut GuestImage, address: u64) -> Result<u64, ApiError> {
    Ok(u64::from_le_bytes(read(image, address)?))
}
fn value(bytes: &[u8], at: usize) -> u64 {
    let mut v = [0; 8];
    v.copy_from_slice(&bytes[at..at + 8]);
    u64::from_le_bytes(v)
}
fn store(bytes: &mut [u8], at: usize, v: u64) {
    bytes[at..at + 8].copy_from_slice(&v.to_le_bytes());
}
fn nonvolatile(reg: usize) -> bool {
    matches!(reg, 3 | 5 | 6 | 7 | 12..=15)
}

#[derive(Clone)]
struct State {
    context: [u8; CONTEXT_SIZE],
    pointers: Option<[u8; 256]>,
}
impl State {
    fn sp(&self) -> u64 {
        value(&self.context, SP)
    }
    fn advance(&mut self, amount: u64) -> Result<(), ApiError> {
        let sp = add(self.sp(), amount)?;
        store(&mut self.context, SP, sp);
        Ok(())
    }
    fn integer(
        &mut self,
        image: &mut GuestImage,
        reg: usize,
        address: u64,
    ) -> Result<(), ApiError> {
        if !nonvolatile(reg) {
            return Err(ApiError::Unsupported);
        }
        let v = qword(image, address)?;
        store(&mut self.context, 0x78 + reg * 8, v);
        if let Some(p) = &mut self.pointers {
            store(p, (16 + reg) * 8, address);
        }
        Ok(())
    }
    fn pop(&mut self, image: &mut GuestImage, reg: usize) -> Result<(), ApiError> {
        self.integer(image, reg, self.sp())?;
        self.advance(8)
    }
    fn ret(&mut self, image: &mut GuestImage) -> Result<(), ApiError> {
        let pc = qword(image, self.sp())?;
        store(&mut self.context, PC, pc);
        self.advance(8)
    }
}

struct Info {
    flags: u8,
    prolog: u8,
    frame: u8,
    codes: Vec<u8>,
    trailer: u64,
}
fn info(image: &mut GuestImage, address: u64) -> Result<Info, ApiError> {
    let h: [u8; 4] = read(image, address)?;
    let flags = h[0] >> 3;
    if h[0] & 7 != 1 || flags & !7 != 0 || (flags & 4 != 0 && flags & 3 != 0) {
        return Err(ApiError::Unsupported);
    }
    let mut codes = vec![0; usize::from(h[2]) * 2];
    if !codes.is_empty() && image.memory.read_virt(add(address, 4)?, &mut codes) != codes.len() {
        return Err(ApiError::MemoryFault);
    }
    Ok(Info {
        flags,
        prolog: h[1],
        frame: h[3],
        trailer: add(address, 4 + (codes.len() as u64 + 3) / 4 * 4)?,
        codes,
    })
}
fn slots(op: u8, operand: u8) -> Result<usize, ApiError> {
    Ok(match op {
        0 | 2 | 3 => 1,
        1 if operand == 0 => 2,
        1 if operand == 1 => 3,
        4 | 8 => 2,
        5 | 9 => 3,
        10 if operand <= 1 => 1,
        _ => return Err(ApiError::Unsupported),
    })
}
fn frame(state: &State, info: &Info, offset: u64) -> Result<u64, ApiError> {
    let reg = usize::from(info.frame & 15);
    if reg == 0 {
        return Ok(state.sp());
    }
    if !nonvolatile(reg) {
        return Err(ApiError::Unsupported);
    }
    let mut established = offset >= u64::from(info.prolog);
    let mut i = 0;
    while i < info.codes.len() {
        let op = info.codes[i + 1] & 15;
        let count = slots(op, info.codes[i + 1] >> 4)? * 2;
        if i + count > info.codes.len() {
            return Err(ApiError::Unsupported);
        }
        if op == 3 && u64::from(info.codes[i]) <= offset {
            established = true;
        }
        i += count;
    }
    if established {
        value(&state.context, 0x78 + reg * 8)
            .checked_sub(u64::from(info.frame >> 4) * 16)
            .ok_or(ApiError::MemoryFault)
    } else {
        Ok(state.sp())
    }
}

// Recognize the instruction sequence before touching the stack. A partial
// epilog prefix in an ordinary body must fall back to metadata, not execute.
fn epilog(
    image: &mut GuestImage,
    state: &State,
    pc: u64,
    end: u64,
    frame_reg: u8,
) -> Result<Option<State>, ApiError> {
    let mut at = pc;
    let mut adjusted = state.clone();
    let mut pops = Vec::new();
    let byte = |image: &mut GuestImage, p| read::<1>(image, p).map(|b| b[0]);
    let b = byte(image, at)?;
    if matches!(b, 0x48 | 0x49) {
        let op = byte(image, add(at, 1)?)?;
        if b == 0x48 && matches!(op, 0x81 | 0x83) && byte(image, add(at, 2)?)? == 0xc4 {
            let (delta, size) = if op == 0x83 {
                (i64::from(byte(image, add(at, 3)?)? as i8), 4)
            } else {
                (i64::from(i32::from_le_bytes(read(image, add(at, 3)?)?)), 7)
            };
            let sp = adjusted
                .sp()
                .checked_add_signed(delta)
                .ok_or(ApiError::MemoryFault)?;
            store(&mut adjusted.context, SP, sp);
            at = add(at, size)?;
        } else if op == 0x8d {
            let m = byte(image, add(at, 2)?)?;
            if !matches!(m >> 6, 1 | 2) || m & 0x38 != 0x20 {
                return Ok(None);
            }
            let mut reg = m & 7;
            let displacement = 3;
            if reg == 4 {
                // SP1's v1 epilog scanner does not recognize a SIB-encoded
                // LEA, even with R12 as the frame register and no index.
                // Apply the unwind metadata just as it does for body code.
                return Ok(None);
            }
            reg += (b & 1) * 8;
            if reg != frame_reg || !nonvolatile(usize::from(reg)) {
                return Ok(None);
            }
            let (delta, width) = if m >> 6 == 1 {
                (i64::from(byte(image, add(at, displacement)?)? as i8), 1)
            } else {
                (
                    i64::from(i32::from_le_bytes(read(image, add(at, displacement)?)?)),
                    4,
                )
            };
            let sp = value(&state.context, 0x78 + usize::from(reg) * 8)
                .checked_add_signed(delta)
                .ok_or(ApiError::MemoryFault)?;
            store(&mut adjusted.context, SP, sp);
            at = add(at, displacement + width)?;
        } else {
            return Ok(None);
        }
    }
    for _ in 0..32 {
        if at >= end {
            return Ok(None);
        }
        let b = byte(image, at)?;
        let (op, high, size) = if b == 0x41 {
            (byte(image, add(at, 1)?)?, 8, 2)
        } else {
            (b, 0, 1)
        };
        if (0x58..=0x5f).contains(&op) {
            let reg = usize::from(op - 0x58 + high);
            if !nonvolatile(reg) {
                return Ok(None);
            }
            pops.push(reg);
            at = add(at, size)?;
            continue;
        }
        if b == 0xc3 || (b == 0xf3 && byte(image, add(at, 1)?)? == 0xc3) {
            for reg in pops {
                adjusted.pop(image, reg)?;
            }
            adjusted.ret(image)?;
            return Ok(Some(adjusted));
        }
        // Tail-call epilogs need separate instruction/address validation.
        // Reject explicitly instead of blindly applying body unwind codes.
        if matches!(b, 0xe9 | 0xeb | 0xff) {
            return Err(ApiError::Unsupported);
        }
        return Ok(None);
    }
    Err(ApiError::Unsupported)
}

pub(super) fn unwind(image: &mut GuestImage, a: &[u64]) -> Result<u64, ApiError> {
    if image.thread.context.mode != DecodeMode::X64 || a[0] & !3 != 0 {
        return Err(ApiError::Unsupported);
    }
    let mut state = State {
        context: read(image, a[4])?,
        pointers: if a[7] == 0 {
            None
        } else {
            Some(read(image, a[7])?)
        },
    };
    let mut entry = a[3];
    let mut visited = Vec::new();
    let mut estab = None;
    let mut handler = None;
    let mut result = 0;
    let mut machine_frame = false;
    let mut finished = false;
    for depth in 0..64 {
        if visited.contains(&entry) {
            return Err(ApiError::Unsupported);
        }
        visited.push(entry);
        let record: [u8; 12] = read(image, entry)?;
        let start = add(
            a[1],
            u64::from(u32::from_le_bytes(
                record[0..4].try_into().map_err(|_| ApiError::Unsupported)?,
            )),
        )?;
        let end = add(
            a[1],
            u64::from(u32::from_le_bytes(
                record[4..8].try_into().map_err(|_| ApiError::Unsupported)?,
            )),
        )?;
        let rva = u32::from_le_bytes(
            record[8..12]
                .try_into()
                .map_err(|_| ApiError::Unsupported)?,
        );
        if start >= end || rva & 3 != 0 {
            return Err(ApiError::Unsupported);
        }
        let info = info(image, add(a[1], u64::from(rva))?)?;
        let offset = a[2].checked_sub(start).ok_or(ApiError::Unsupported)?;
        if depth == 0 && a[2] >= end {
            return Err(ApiError::Unsupported);
        }
        let frame = *estab.get_or_insert(frame(&state, &info, offset)?);
        if depth == 0 && offset >= u64::from(info.prolog) {
            if let Some(s) = epilog(image, &state, a[2], end, info.frame & 15)? {
                state = s;
                finished = true;
                break;
            }
        }
        let mut i = 0;
        while i < info.codes.len() {
            let op = info.codes[i + 1] & 15;
            let operand = info.codes[i + 1] >> 4;
            let len = slots(op, operand)? * 2;
            if i + len > info.codes.len() {
                return Err(ApiError::Unsupported);
            }
            let extra = if len == 4 {
                u64::from(u16::from_le_bytes([info.codes[i + 2], info.codes[i + 3]]))
            } else if len == 6 {
                u64::from(u32::from_le_bytes(
                    info.codes[i + 2..i + 6]
                        .try_into()
                        .map_err(|_| ApiError::Unsupported)?,
                ))
            } else {
                0
            };
            if depth != 0 || u64::from(info.codes[i]) <= offset {
                match op {
                    0 => state.pop(image, usize::from(operand))?,
                    1 => state.advance(if operand == 0 { extra * 8 } else { extra })?,
                    2 => state.advance(u64::from(operand) * 8 + 8)?,
                    3 if operand == 0 && info.frame & 15 != 0 => {
                        store(&mut state.context, SP, frame)
                    }
                    4 | 5 => state.integer(
                        image,
                        usize::from(operand),
                        add(frame, if op == 4 { extra * 8 } else { extra })?,
                    )?,
                    8 | 9 if operand >= 6 => {
                        let address = add(frame, if op == 8 { extra * 16 } else { extra })?;
                        let data: [u8; 16] = read(image, address)?;
                        let at = 0x1a0 + usize::from(operand) * 16;
                        state.context[at..at + 16].copy_from_slice(&data);
                        if let Some(p) = &mut state.pointers {
                            store(p, usize::from(operand) * 8, address);
                        }
                    }
                    10 => {
                        if machine_frame {
                            return Err(ApiError::Unsupported);
                        }
                        let sp = add(state.sp(), u64::from(operand) * 8)?;
                        let pc = qword(image, sp)?;
                        let sp = qword(image, add(sp, 24)?)?;
                        store(&mut state.context, PC, pc);
                        store(&mut state.context, SP, sp);
                        machine_frame = true;
                    }
                    _ => return Err(ApiError::Unsupported),
                }
            }
            i += len;
        }
        if info.flags & 4 != 0 {
            entry = info.trailer;
            continue;
        }
        if !machine_frame {
            state.ret(image)?;
        }
        if offset >= u64::from(info.prolog) && u64::from(info.flags) & a[0] != 0 {
            result = add(
                a[1],
                u64::from(u32::from_le_bytes(read(image, info.trailer)?)),
            )?;
            handler = Some(add(info.trailer, 4)?);
        }
        finished = true;
        break;
    }
    if !finished {
        return Err(ApiError::Unsupported);
    }
    // Validate every output before publishing any context changes. Malformed
    // metadata, stack faults or bad output pointers cannot leave partial state.
    for (address, size) in [(a[4], CONTEXT_SIZE), (a[6], 8)] {
        if !image.memory.probe_write(address, size) {
            return Err(ApiError::MemoryFault);
        }
    }
    if state.pointers.is_some() && !image.memory.probe_write(a[7], 256)
        || handler.is_some() && !image.memory.probe_write(a[5], 8)
    {
        return Err(ApiError::MemoryFault);
    }
    if !image.memory.write_virt(a[4], &state.context) {
        return Err(ApiError::MemoryFault);
    }
    if let Some(p) = state.pointers {
        if !image.memory.write_virt(a[7], &p) {
            return Err(ApiError::MemoryFault);
        }
    }
    put(
        &mut image.memory,
        a[6],
        estab.ok_or(ApiError::Unsupported)?,
        8,
    )
    .map_err(|_| ApiError::MemoryFault)?;
    if let Some(h) = handler {
        put(&mut image.memory, a[5], h, 8).map_err(|_| ApiError::MemoryFault)?;
    }
    Ok(result)
}
