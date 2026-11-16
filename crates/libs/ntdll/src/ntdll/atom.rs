//! Atom services for the current single-session executive.
//! User buffers are captured before table lookup. Comparison uses simple BMP
//! Unicode uppercase; Win7 NLS and session namespace isolation remain pending.

extern crate alloc;
use super::{status::*, types::NTSTATUS};
use crate::{ke::sync::Spinlock, mm::user_copy};
use alloc::vec::Vec;

const FIRST: u16 = 0xc000;
const MAX_NAME: usize = 255;
const MISALIGNED: NTSTATUS = 0x80000002u32 as i32;
const WAS_LOCKED: NTSTATUS = 0x40000019;

#[derive(Clone, Copy)]
struct Name {
    units: [u16; MAX_NAME],
    length: usize,
}
impl Name {
    const fn empty() -> Self {
        Self {
            units: [0; MAX_NAME],
            length: 0,
        }
    }
    fn slice(&self) -> &[u16] {
        &self.units[..self.length]
    }
    fn integer(&self) -> Result<Option<u16>, NTSTATUS> {
        let name = self.slice();
        if name.len() < 2
            || name[0] != b'#' as u16
            || !name[1..].iter().all(|c| (0x30..=0x39).contains(c))
        {
            return Ok(None);
        }
        let mut value = 0u16;
        for digit in &name[1..] {
            value = value
                .checked_mul(10)
                .and_then(|n| n.checked_add(*digit - 0x30))
                .ok_or(STATUS_INVALID_PARAMETER)?;
        }
        if value == 0 || value >= FIRST {
            return Err(STATUS_INVALID_PARAMETER);
        }
        Ok(Some(value))
    }
}
fn uppercase(unit: u16) -> u16 {
    let Some(character) = char::from_u32(u32::from(unit)) else {
        return unit;
    };
    let mut mapping = character.to_uppercase();
    let first = mapping.next().unwrap_or(character);
    if mapping.next().is_none() && u32::from(first) <= u16::MAX as u32 {
        first as u16
    } else {
        unit
    }
}
fn same_name(left: &Name, right: &Name) -> bool {
    left.length == right.length
        && left
            .slice()
            .iter()
            .zip(right.slice())
            .all(|(&a, &b)| uppercase(a) == uppercase(b))
}
fn capture(pointer: *const u16, bytes: u32) -> Result<Name, NTSTATUS> {
    if bytes as usize > MAX_NAME * 2 || pointer.is_null() {
        return Err(STATUS_INVALID_PARAMETER);
    }
    if pointer as usize % 2 != 0 {
        return Err(MISALIGNED);
    }
    let mut buffer = [0u8; MAX_NAME * 2];
    user_copy::copy_from_user(&mut buffer[..bytes as usize], pointer as u64)?;
    let mut name = Name::empty();
    for pair in buffer[..bytes as usize].chunks_exact(2) {
        let unit = u16::from_le_bytes([pair[0], pair[1]]);
        if unit == 0 {
            break;
        }
        name.units[name.length] = unit;
        name.length += 1;
    }
    if name.length == 0 {
        return Err(STATUS_OBJECT_NAME_INVALID);
    }
    Ok(name)
}
#[derive(Clone, Copy)]
struct Entry {
    name: Name,
    references: u16,
    pinned: bool,
}
static ATOMS: Spinlock<Vec<Option<Entry>>> = Spinlock::new(Vec::new());

fn lookup(name: Name, add: bool) -> Result<u16, NTSTATUS> {
    if let Some(integer) = name.integer()? {
        return Ok(integer);
    }
    let mut table = ATOMS.lock();
    for (index, slot) in table.iter_mut().enumerate() {
        if let Some(entry) = slot {
            if same_name(&entry.name, &name) {
                if add && !entry.pinned {
                    match entry.references.checked_add(1) {
                        Some(count) => entry.references = count,
                        None => entry.pinned = true,
                    }
                }
                return Ok(FIRST + index as u16);
            }
        }
    }
    if !add {
        return Err(STATUS_OBJECT_NAME_NOT_FOUND);
    }
    let index = if let Some(index) = table.iter().position(Option::is_none) {
        index
    } else {
        if table.len() == 0x4000 || table.try_reserve(1).is_err() {
            return Err(STATUS_NO_MEMORY);
        }
        table.push(None);
        table.len() - 1
    };
    table[index] = Some(Entry {
        name,
        references: 1,
        pinned: false,
    });
    Ok(FIRST + index as u16)
}
fn add_or_find(name: *const u16, length: u32, output: *mut u16, add: bool) -> NTSTATUS {
    let result = (|| {
        let name = capture(name, length)?;
        if !output.is_null() {
            if output as usize % 2 != 0 {
                return Err(MISALIGNED);
            }
            user_copy::probe_user_write(output as u64, 2)?;
        }
        let atom = lookup(name, add)?;
        if !output.is_null() {
            user_copy::write_user_object(output as u64, &atom)?;
        }
        Ok(())
    })();
    result.err().unwrap_or(STATUS_SUCCESS)
}
/// Capture a counted UTF-16 name and acquire one atom reference.
pub extern "C" fn nt_add_atom(name: *const u16, length: u32, output: *mut u16) -> NTSTATUS {
    add_or_find(name, length, output, true)
}
/// Lookup without changing references; the output address is optional.
pub extern "C" fn nt_find_atom(name: *const u16, length: u32, output: *mut u16) -> NTSTATUS {
    add_or_find(name, length, output, false)
}
pub extern "C" fn nt_delete_atom(atom: u16) -> NTSTATUS {
    if atom == 0 {
        return STATUS_INVALID_HANDLE;
    }
    if atom < FIRST {
        return STATUS_SUCCESS;
    }
    let mut table = ATOMS.lock();
    let Some(slot) = table.get_mut(usize::from(atom - FIRST)) else {
        return STATUS_INVALID_HANDLE;
    };
    let Some(entry) = slot else {
        return STATUS_INVALID_HANDLE;
    };
    if entry.pinned {
        return WAS_LOCKED;
    }
    entry.references -= 1;
    if entry.references == 0 {
        *slot = None;
    }
    STATUS_SUCCESS
}
fn snapshot(atom: u16) -> Result<Entry, NTSTATUS> {
    if atom == 0 {
        return Err(STATUS_INVALID_HANDLE);
    }
    if atom >= FIRST {
        return ATOMS
            .lock()
            .get(usize::from(atom - FIRST))
            .copied()
            .flatten()
            .ok_or(STATUS_INVALID_HANDLE);
    }
    let mut name = Name::empty();
    let mut digits = [0u16; 5];
    let mut count = 0;
    let mut value = atom;
    while value != 0 {
        digits[count] = 0x30 + value % 10;
        count += 1;
        value /= 10;
    }
    name.units[0] = b'#' as u16;
    for (index, digit) in digits[..count].iter().rev().enumerate() {
        name.units[index + 1] = *digit;
    }
    name.length = count + 1;
    Ok(Entry {
        name,
        references: 1,
        pinned: true,
    })
}
fn query_basic(atom: u16, address: u64, length: usize) -> Result<(NTSTATUS, u32), NTSTATUS> {
    if length < 6 {
        return Ok((STATUS_INFO_LENGTH_MISMATCH, 6));
    }
    let entry = snapshot(atom)?;
    let capacity = length - 6;
    let too_small = capacity < 4;
    let characters = if too_small {
        entry.name.length
    } else {
        entry.name.length.min((capacity - 2) / 2)
    };
    let mut buffer = [0u8; 6 + (MAX_NAME + 1) * 2];
    buffer[..2].copy_from_slice(&entry.references.to_le_bytes());
    buffer[2..4].copy_from_slice(&u16::from(entry.pinned).to_le_bytes());
    buffer[4..6].copy_from_slice(&((characters * 2) as u16).to_le_bytes());
    let written = if too_small {
        6
    } else {
        for (index, unit) in entry.name.units[..characters].iter().enumerate() {
            buffer[6 + index * 2..8 + index * 2].copy_from_slice(&unit.to_le_bytes());
        }
        6 + (characters + 1) * 2
    };
    user_copy::copy_to_user(address, &buffer[..written])?;
    Ok((
        if too_small {
            STATUS_BUFFER_TOO_SMALL
        } else {
            STATUS_SUCCESS
        },
        if too_small { 6 } else { written as u32 },
    ))
}
fn query_table(address: u64, length: usize) -> Result<(NTSTATUS, u32), NTSTATUS> {
    if length < 4 {
        return Ok((STATUS_INFO_LENGTH_MISMATCH, 4));
    }
    let mut ids = Vec::new();
    let count;
    {
        let table = ATOMS.lock();
        count = table.iter().filter(|slot| slot.is_some()).count();
        let capacity = count.min((length - 4) / 2);
        ids.try_reserve_exact(capacity)
            .map_err(|_| STATUS_NO_MEMORY)?;
        for (index, slot) in table.iter().enumerate() {
            if slot.is_some() && ids.len() < capacity {
                ids.push(FIRST + index as u16);
            }
        }
    }
    user_copy::write_user_object(address, &(count as u32))?;
    for (index, atom) in ids.iter().enumerate() {
        user_copy::write_user_object(address + 4 + index as u64 * 2, atom)?;
    }
    Ok(if ids.len() == count {
        (STATUS_SUCCESS, 4 + count as u32 * 2)
    } else {
        (STATUS_INFO_LENGTH_MISMATCH, 4)
    })
}
/// Native layouts: class 0 is basic information, class 1 enumerates string IDs.
pub extern "C" fn nt_query_information_atom(
    atom: u16,
    class: u32,
    information: *mut u8,
    length: u32,
    return_length: *mut u32,
) -> NTSTATUS {
    let result = (|| {
        if length != 0 {
            if information as usize % 4 != 0 {
                return Err(MISALIGNED);
            }
            user_copy::probe_user_write(information as u64, length as usize)?;
        }
        if !return_length.is_null() {
            if return_length as usize % 4 != 0 {
                return Err(MISALIGNED);
            }
            user_copy::probe_user_write(return_length as u64, 4)?;
        }
        let (status, required) = match class {
            0 => query_basic(atom, information as u64, length as usize)?,
            1 => query_table(information as u64, length as usize)?,
            _ => (STATUS_INVALID_INFO_CLASS, 0),
        };
        if !return_length.is_null() {
            user_copy::write_user_object(return_length as u64, &required)?;
        }
        Ok(status)
    })();
    match result {
        Ok(status) | Err(status) => status,
    }
}
