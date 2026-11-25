use super::{put, read, word, ApiError, DecodeMode, GuestImage, GuestMemory, MAX_BUFFER};
use alloc::vec::Vec;

#[path = "../../../../../../libs/ntdll/src/ntdll/rtl_integer.rs"]
mod integer;

pub(super) fn invoke(image: &mut GuestImage, name: &str, args: &[u64]) -> Result<u64, ApiError> {
    let width = if image.thread.context.mode == DecodeMode::X86 { 4 } else { 8 };
    let memory = &mut image.memory;
    let fault = |_| ApiError::MemoryFault;
    let radix = args[1] as u32;
    let status = match name {
        "RtlCharToInteger" => {
            let mut stopped = None;
            let units = (0..=MAX_BUFFER as u64).map_while(|offset| {
                if offset == MAX_BUFFER as u64 {
                    stopped = Some(ApiError::Unsupported);
                    return None;
                }
                let mut byte = [0];
                if args[0].checked_add(offset).is_some_and(|address| memory.read_virt(address, &mut byte) == 1) {
                    Some(byte[0])
                } else {
                    stopped = Some(ApiError::MemoryFault);
                    None
                }
            });
            let result = integer::parse_ansi(units, radix);
            if let Some(error) = stopped {
                return Err(error);
            }
            match result {
                Ok(value) => {
                    put(memory, args[2], u64::from(value), 4).map_err(fault)?;
                    0
                }
                Err(status) => status,
            }
        }
        "RtlUnicodeStringToInteger" => {
            let descriptor = read(memory, args[0], 2 * width).map_err(fault)?;
            let length = usize::from(u16::from_le_bytes([descriptor[0], descriptor[1]]));
            let result = if length == 0 || length & 1 != 0 || !matches!(radix, 0 | 2 | 8 | 10 | 16) {
                Err(integer::INVALID_PARAMETER)
            } else {
                let buffer = word(memory, args[0] + width as u64, width).map_err(fault)?;
                let bytes = read(memory, buffer, length).map_err(fault)?;
                let units = bytes.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]]));
                integer::parse(units.skip_while(|&unit| unit <= 0x20), radix)
            };
            put(memory, args[2], u64::from(result.unwrap_or_default()), 4).map_err(fault)?;
            result.err().unwrap_or_default()
        }
        "RtlIntegerToChar" => {
            let mut storage = [0; 32];
            let digits = match integer::digits(args[0] as u32, radix, &mut storage) {
                Ok(digits) => digits,
                Err(status) => return Ok(u64::from(status)),
            };
            let (padding, terminate) = match integer::field(args[2] as i32, digits.len()) {
                Ok(field) => field,
                Err(status) => return Ok(u64::from(status)),
            };
            let length = padding + digits.len() + usize::from(terminate);
            if length > MAX_BUFFER {
                return Err(ApiError::Unsupported);
            }
            let mut bytes = Vec::with_capacity(length);
            bytes.resize(padding, b'0');
            bytes.extend_from_slice(digits);
            if terminate {
                bytes.push(0);
            }
            if !memory.write_virt(args[3], &bytes) {
                return Err(ApiError::MemoryFault);
            }
            0
        }
        "RtlIntegerToUnicodeString" => {
            let mut storage = [0; 32];
            let digits = match integer::digits(args[0] as u32, radix, &mut storage) {
                Ok(digits) => digits,
                Err(status) => return Ok(u64::from(status)),
            };
            let descriptor = read(memory, args[2], 2 * width).map_err(fault)?;
            let capacity = usize::from(u16::from_le_bytes([descriptor[2], descriptor[3]]));
            let length = digits.len() * 2;
            put(memory, args[2], length as u64, 2).map_err(fault)?;
            if capacity < length + 2 {
                return Ok(u64::from(integer::BUFFER_OVERFLOW));
            }
            let buffer = word(memory, args[2] + width as u64, width).map_err(fault)?;
            let mut bytes = [0; 66];
            for (index, digit) in digits.iter().enumerate() {
                bytes[index * 2] = *digit;
            }
            if !memory.write_virt(buffer, &bytes[..length + 2]) {
                return Err(ApiError::MemoryFault);
            }
            0
        }
        _ => return Err(ApiError::Unsupported),
    };
    Ok(u64::from(status))
}
