//! Limited SP1 apartment initialization for interpreted guest threads.
//! Context values are opaque guest tokens: activation, context interfaces,
//! cross-apartment calls and message pumping are not implemented here.
use super::*;

#[derive(Default)]
pub(super) struct State {
    mta: Option<Mta>,
    strings: Vec<u64>,
}

struct Mta {
    context: u64,
    participants: u32,
}

#[derive(Default)]
pub(super) struct ThreadState {
    apartment: Option<Apartment>,
}

struct Apartment {
    sta: bool,
    references: u32,
    ole_references: u32,
    context: u64,
}

pub(super) fn cleanup_thread(image: &mut GuestImage) {
    if let Some(apartment) = image.win32.local.com.apartment.take() {
        if !apartment.sta {
            let Some(mta) = image.win32.com.mta.as_mut() else { return; };
            if mta.participants > 1 {
                mta.participants -= 1;
                return;
            }
            image.win32.com.mta = None;
        }
        // STA owns its context; the last MTA participant owns the shared
        // context's release. Repeated initialization is a per-thread count.
        let _ = free(image, apartment.context);
    }
}

pub(super) fn invoke(image: &mut GuestImage, name: &str, a: &[u64]) -> Result<u64, ApiError> {
    match name {
        "CoInitialize" | "CoInitializeEx" | "OleInitialize" => {
            let ole = name == "OleInitialize";
            let flags = if name == "CoInitializeEx" { a[1] as u32 } else { 2 };
            if a[0] != 0 || flags & !0x0e != 0 { return Ok(0x80070057); }
            let sta = flags & 2 != 0;
            if let Some(apartment) = image.win32.local.com.apartment.as_mut() {
                if apartment.sta != sta { return Ok(0x80010106); }
                let Some(count) = apartment.references.checked_add(1) else {
                    return Ok(0x8007000e);
                };
                apartment.references = count;
                let already_initialized = !ole || apartment.ole_references > 0;
                apartment.ole_references += u32::from(ole);
                return Ok(u64::from(already_initialized));
            }
            let existing_mta = if sta { None } else { image.win32.com.mta.as_mut() };
            let context = if let Some(mta) = existing_mta {
                let Some(count) = mta.participants.checked_add(1) else {
                    return Ok(0x8007000e);
                };
                mta.participants = count;
                mta.context
            } else {
                let context = match allocate(image, 0, PAGE as u64, READ) {
                    Ok(pointer) => pointer,
                    Err(_) => return Ok(0x8007000e),
                };
                if !sta {
                    image.win32.com.mta = Some(Mta { context, participants: 1 });
                }
                context
            };
            image.win32.local.com.apartment = Some(Apartment {
                sta, references: 1, ole_references: u32::from(ole), context,
            });
            Ok(0)
        }
        "CoUninitialize" | "OleUninitialize" => {
            if let Some(apartment) = image.win32.local.com.apartment.as_mut() {
                if name == "OleUninitialize" {
                    if apartment.ole_references == 0 { return Ok(0); }
                    apartment.ole_references -= 1;
                } else if apartment.references == apartment.ole_references {
                    return Ok(0);
                }
                if apartment.references > 1 { apartment.references -= 1; }
                else { cleanup_thread(image); }
            }
            // This export is void; zero here is only the interpreter's return
            // register convention, never an HRESULT success assertion.
            Ok(0)
        }
        "CoGetContextToken" => {
            let Some(apartment) = image.win32.local.com.apartment.as_ref() else {
                return Ok(0x800401f0);
            };
            let token = apartment.context;
            let width = if image.thread.context.mode == DecodeMode::X86 { 4 } else { 8 };
            put(&mut image.memory, a[0], token, width).map_err(|_| ApiError::MemoryFault)?;
            Ok(0)
        }
        "IIDFromString" => iid_from_string(&mut image.memory, a[0], a[1]),
        "SysAllocString" => {
            if a[0] == 0 { return Ok(0); }
            let units = wide_z_bytes(&mut image.memory, a[0])?;
            let length = units.len() - 2;
            allocate_bstr(image, length, &units[..length])
        }
        "SysAllocStringLen" => {
            let Some(length) = (a[1] as u32).checked_mul(2).map(|length| length as usize)
                .filter(|length| *length <= MAX_BUFFER) else { return Ok(0); };
            let contents = if a[0] == 0 || length == 0 {
                Vec::new()
            } else {
                read(&mut image.memory, a[0], length).map_err(|_| ApiError::MemoryFault)?
            };
            allocate_bstr(image, length, &contents)
        }
        "SysStringLen" => {
            if a[0] == 0 { return Ok(0); }
            let prefix = a[0].checked_sub(4).ok_or(ApiError::MemoryFault)?;
            Ok(word(&mut image.memory, prefix, 4).map_err(|_| ApiError::MemoryFault)? / 2)
        }
        "SysFreeString" => {
            release_bstr(image, a[0])?;
            Ok(0)
        }
        "VariantInit" => {
            put(&mut image.memory, a[0], 0, 2).map_err(|_| ApiError::MemoryFault)?;
            Ok(0)
        }
        "VariantClear" => clear_variant(image, a[0]),
        _ => Err(ApiError::Unsupported),
    }
}

fn iid_from_string(memory: &mut dyn GuestMemory, input: u64, output: u64) -> Result<u64, ApiError> {
    const E_INVALIDARG: u64 = 0x8007_0057;
    const CO_E_IIDSTRING: u64 = 0x8004_01f4;

    if output == 0 {
        return Ok(E_INVALIDARG);
    }
    let mut iid = [0; 16];
    let (status, written) = if input == 0 {
        (0, iid.len())
    } else {
        let mut text = [0u16; 38];
        for (index, unit) in text.iter_mut().enumerate() {
            let address = input.checked_add(2 * index as u64).ok_or(ApiError::MemoryFault)?;
            *unit = word(memory, address, 2).map_err(|_| ApiError::MemoryFault)? as u16;
            if *unit == 0 {
                return Ok(E_INVALIDARG);
            }
        }
        let end = input.checked_add(76).ok_or(ApiError::MemoryFault)?;
        if word(memory, end, 2).map_err(|_| ApiError::MemoryFault)? != 0 {
            return Ok(E_INVALIDARG);
        }
        if text[0] != u16::from(b'{') {
            return Ok(CO_E_IIDSTRING);
        }
        let written = parse_iid_fields(&text, &mut iid);
        let status = if written == iid.len() && text[37] == u16::from(b'}') {
            0
        } else {
            CO_E_IIDSTRING
        };
        (status, written)
    };
    if written != 0 && !memory.write_virt(output, &iid[..written]) {
        return Err(ApiError::MemoryFault);
    }
    Ok(status)
}

fn parse_iid_fields(text: &[u16; 38], iid: &mut [u8; 16]) -> usize {
    let mut written = 0;
    for (start, end, hyphen) in [
        (1, 9, true), (10, 14, true), (15, 19, true), (20, 22, false),
        (22, 24, true), (25, 27, false), (27, 29, false), (29, 31, false),
        (31, 33, false), (33, 35, false), (35, 37, false),
    ] {
        let mut field = 0u32;
        let mut complete = true;
        for &unit in &text[start..end] {
            let digit = match unit {
                0x30..=0x39 => unit - 0x30,
                0x41..=0x46 => unit - 0x41 + 10,
                0x61..=0x66 => unit - 0x61 + 10,
                _ => {
                    complete = false;
                    break;
                }
            };
            field = (field << 4) | u32::from(digit);
        }
        complete &= !hyphen || text[end] == u16::from(b'-');
        if complete || written == 0 {
            let width = (end - start) / 2;
            iid[written..written + width].copy_from_slice(&field.to_le_bytes()[..width]);
            written += width;
        }
        if !complete {
            break;
        }
    }
    written
}

fn clear_variant(image: &mut GuestImage, variant: u64) -> Result<u64, ApiError> {
    const VT_ARRAY: u16 = 0x2000;
    const VT_BYREF: u16 = 0x4000;
    const DISP_E_BADVARTYPE: u64 = 0x8002_0008;
    const E_NOTIMPL: u64 = 0x8000_4001;

    let kind = word(&mut image.memory, variant, 2).map_err(|_| ApiError::MemoryFault)? as u16;
    let base = kind & !(VT_ARRAY | VT_BYREF);
    if !matches!(base, 0..=14 | 16..=23 | 36)
        || (kind != base && base <= 1)
        || kind == 12
    {
        return Ok(DISP_E_BADVARTYPE);
    }
    if !image.memory.probe_write(variant, 2) {
        return Err(ApiError::MemoryFault);
    }
    if kind & VT_BYREF == 0 {
        if kind & VT_ARRAY != 0 || matches!(base, 9 | 13 | 36) {
            return Ok(E_NOTIMPL);
        }
        if base == 8 {
            let width = if image.thread.context.mode == DecodeMode::X86 { 4 } else { 8 };
            let payload = variant.checked_add(8).ok_or(ApiError::MemoryFault)?;
            let pointer = word(&mut image.memory, payload, width).map_err(|_| ApiError::MemoryFault)?;
            release_bstr(image, pointer)?;
        }
    }
    put(&mut image.memory, variant, 0, 2).map_err(|_| ApiError::MemoryFault)?;
    Ok(0)
}

fn release_bstr(image: &mut GuestImage, pointer: u64) -> Result<(), ApiError> {
    if pointer == 0 {
        return Ok(());
    }
    let index = image.win32.com.strings.iter().position(|&owned| owned == pointer)
        .ok_or(ApiError::MemoryFault)?;
    free(image, pointer - 4).map_err(|_| ApiError::MemoryFault)?;
    image.win32.com.strings.swap_remove(index);
    Ok(())
}

fn allocate_bstr(image: &mut GuestImage, length: usize, contents: &[u8]) -> Result<u64, ApiError> {
    if image.win32.com.strings.try_reserve(1).is_err() { return Ok(0); }
    let Ok(base) = allocate(image, 0, length as u64 + 6, READ | WRITE) else {
        return Ok(0);
    };
    let pointer = base + 4;
    if put(&mut image.memory, base, length as u64, 4).is_err()
        || (!contents.is_empty() && !image.memory.write_virt(pointer, contents))
        || put(&mut image.memory, pointer + length as u64, 0, 2).is_err()
    {
        free(image, base).map_err(|_| ApiError::MemoryFault)?;
        return Err(ApiError::MemoryFault);
    }
    image.win32.com.strings.push(pointer);
    Ok(pointer)
}
pub(super) fn create_instance(image: &mut GuestImage, args: &[u64]) -> Result<u64, ApiError> {
    let width = if image.thread.context.mode == DecodeMode::X86 { 4 } else { 8 };
    if args[4] == 0 || put(&mut image.memory, args[4], 0, width).is_err() {
        return Err(ApiError::Hresult(0x80004003, "invalid COM interface output"));
    }
    if image.win32.local.com.apartment.is_none() {
        return Err(ApiError::Hresult(0x800401f0, "COM apartment uninitialized"));
    }
    if args[0] == 0 || args[3] == 0 {
        return Err(ApiError::Hresult(0x80004003, "invalid COM class or interface ID"));
    }
    read(&mut image.memory, args[0], 16)?;
    read(&mut image.memory, args[3], 16)?;
    Err(ApiError::Hresult(0x80040154, "COM class not registered"))
}
