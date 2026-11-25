use super::{catalog, environment, heap_allocate, put, string, user, word, ApiError, GuestImage, GuestMemory, Outcome, NOACCESS};
use crate::arch::common::btl::ir::DecodeMode;
use alloc::{string::String, vec, vec::Vec};

const MAX_INITIALIZERS: u64 = 4096;

#[derive(Default)]
pub(super) struct State {
    arguments: Option<(usize, u64, u64)>,
    application_type: u32,
    math_error_handler: u64,
}

fn pointer_width(image: &GuestImage) -> usize {
    if image.thread.context.mode == DecodeMode::X86 { 4 } else { 8 }
}

fn data_address(image: &mut GuestImage) -> Option<u64> {
    let base = image.win32.modules.iter().find(|(name, _)| name.eq_ignore_ascii_case("msvcrt.dll"))?.1;
    let address = base.checked_add(u64::from(catalog::MSVCRT_DATA_RVA))?;
    let mut tag = [0; 8];
    (image.memory.read_virt(address + 16, &mut tag) == tag.len() && tag == catalog::MSVCRT_DATA_TAG)
        .then_some(address)
}

fn ansi(text: &str) -> impl Iterator<Item = u8> + '_ {
    text.chars().map(|value| u8::try_from(u32::from(value)).unwrap_or(b'?'))
}

pub(in crate::arch::common::btl::pe) fn initialize_data(
    image: &mut GuestImage, command_line: &str,
) -> Result<(), u32> {
    let Some(slot) = data_address(image) else { return Ok(()) };
    let bytes: Vec<_> = ansi(command_line).chain([0]).collect();
    let width = pointer_width(image);
    let address = heap_allocate(image, bytes.len() as u64)?;
    if !image.memory.write_virt(address, &bytes) { return Err(NOACCESS); }
    put(&mut image.memory, slot, address, width)
}

fn split_command(command: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut input = command.chars().peekable();
    let mut program = String::new();
    let mut quoted = false;
    while let Some(character) = input.peek().copied() {
        if !quoted && matches!(character, ' ' | '\t') { break; }
        input.next();
        if character == '"' { quoted = !quoted; } else { program.push(character); }
    }
    values.push(program);
    loop {
        while matches!(input.peek(), Some(' ' | '\t')) { input.next(); }
        if input.peek().is_none() { return values; }
        let mut argument = String::new();
        let mut quoted = false;
        while let Some(character) = input.peek().copied() {
            if !quoted && matches!(character, ' ' | '\t') { break; }
            let mut slashes = 0;
            while input.peek() == Some(&'\\') { input.next(); slashes += 1; }
            if input.peek() == Some(&'"') {
                argument.extend(core::iter::repeat_n('\\', slashes / 2));
                input.next();
                if slashes % 2 == 1 || (quoted && input.next_if_eq(&'"').is_some()) {
                    argument.push('"');
                } else {
                    quoted = !quoted;
                }
            } else if slashes != 0 {
                argument.extend(core::iter::repeat_n('\\', slashes));
            } else {
                argument.push(character);
                input.next();
            }
        }
        values.push(argument);
    }
}

fn string_vector(image: &mut GuestImage, strings: &[String], width: usize) -> Result<u64, ApiError> {
    let table_len = (strings.len() + 1) * width;
    let capacity = table_len + strings.iter().map(|text| ansi(text).count() + 1).sum::<usize>();
    let address = heap_allocate(image, capacity as u64)?;
    let mut bytes = vec![0; table_len];
    for (index, text) in strings.iter().enumerate() {
        let pointer = address + bytes.len() as u64;
        bytes[index * width..(index + 1) * width].copy_from_slice(&pointer.to_le_bytes()[..width]);
        bytes.extend(ansi(text));
        bytes.push(0);
    }
    if !image.memory.write_virt(address, &bytes) { return Err(ApiError::MemoryFault); }
    Ok(address)
}

fn main_args(image: &mut GuestImage, args: &[u64]) -> Result<u64, ApiError> {
    let width = pointer_width(image);
    if args[3] != 0 { return Err(ApiError::Unsupported); }
    if !image.memory.probe_write(args[0], 4) || !image.memory.probe_write(args[1], width)
        || !image.memory.probe_write(args[2], width) {
        return Err(ApiError::MemoryFault);
    }
    let (count, argv, env) = if let Some(result) = image.win32.crt.arguments {
        result
    } else {
        let x86 = width == 4;
        let params = word(&mut image.memory, image.peb + if x86 { 0x10 } else { 0x20 }, width)?;
        let command_address = word(&mut image.memory, params + if x86 { 0x44 } else { 0x78 }, width)?;
        let arguments = split_command(&string(&mut image.memory, command_address, true)?);
        let environment = environment::snapshot(image)?;
        let units: Vec<_> = environment.chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]])).collect();
        let variables: Vec<_> = units.split(|value| *value == 0).take_while(|value| !value.is_empty())
            .map(String::from_utf16_lossy).collect();
        let result = (arguments.len(), string_vector(image, &arguments, width)?,
            string_vector(image, &variables, width)?);
        image.win32.crt.arguments = Some(result);
        result
    };
    put(&mut image.memory, args[0], count as u64, 4)?;
    put(&mut image.memory, args[1], argv, width)?;
    put(&mut image.memory, args[2], env, width)?;
    Ok(0)
}

fn next_initializer(image: &mut GuestImage, mut next: u64, end: u64) -> Result<Option<(u64, u64)>, ApiError> {
    let width = pointer_width(image);
    while next < end {
        let procedure = word(&mut image.memory, next, width).map_err(|_| ApiError::MemoryFault)?;
        next += width as u64;
        if procedure != 0 { return Ok(Some((procedure, next))); }
    }
    Ok(None)
}

pub(super) fn resume(image: &mut GuestImage, next: u64, end: u64) -> Result<Outcome, ApiError> {
    let Some((procedure, next)) = next_initializer(image, next, end)? else { return Ok(Outcome::Return(0)) };
    image.win32.local.user.completion = Some(user::Completion::CrtInitializer { next, end });
    user::enter_callback(image, procedure, [0; 4])?;
    Ok(Outcome::Continue)
}

pub(super) fn invoke(image: &mut GuestImage, name: &str, args: &[u64]) -> Result<u64, ApiError> {
    match name {
        "__getmainargs" => main_args(image, args),
        "_wtol" => wide_to_long(image, args[0]),
        "iswctype" => Ok(u64::from(super::nls::ctype1(args[0] as u16)? & args[1] as u16)),
        "_ismbblead" => Ok(0),
        "_controlfp" => control_fp(image, args[0] as u32, args[1] as u32),
        "__p__fmode" | "__p__commode" => {
            let address = data_address(image).ok_or(ApiError::Unsupported)?;
            Ok(address + if name == "__p__fmode" { 8 } else { 12 })
        }
        "__set_app_type" => {
            image.win32.crt.application_type = args[0] as u32;
            Ok(0)
        }
        "__setusermatherr" => {
            image.win32.crt.math_error_handler = args[0];
            Ok(0)
        }
        "_cexit" => Ok(0),
        "_initterm" => {
            let [start, end, ..] = *args else { return Err(ApiError::Unsupported) };
            let width = pointer_width(image) as u64;
            if end < start || (end - start) % width != 0 || (end - start) / width > MAX_INITIALIZERS {
                return Err(ApiError::MemoryFault);
            }
            if let Some((procedure, next)) = next_initializer(image, start, end)? {
                image.win32.local.user.completion = Some(user::Completion::CrtInitializer { next, end });
                image.win32.local.user.callback = Some((procedure, [0; 4]));
            }
            Ok(0)
        }
        _ => Err(ApiError::Unsupported),
    }
}

fn wide_to_long(image: &mut GuestImage, source: u64) -> Result<u64, ApiError> {
    let mut input = string(&mut image.memory, source, true)?;
    let trimmed = input.trim_start_matches([' ', '\t', '\n', '\r', '\u{b}', '\u{c}']);
    let negative = trimmed.starts_with('-');
    let offset = input.len() - trimmed.len() + usize::from(trimmed.starts_with(['-', '+']));
    let digits = input.split_off(offset);
    let mut value = 0u32;
    for digit in digits.bytes().take_while(u8::is_ascii_digit) {
        value = value.wrapping_mul(10).wrapping_add(u32::from(digit - b'0'));
    }
    Ok(u64::from(if negative { value.wrapping_neg() } else { value }))
}

fn control_fp(image: &mut GuestImage, value: u32, mask: u32) -> Result<u64, ApiError> {
    let state = &mut image.thread.context;
    let mut control = state.x87_control;
    let mappings = [(0x10, 0x01), (0x80000, 0x02), (0x08, 0x04), (0x04, 0x08),
        (0x02, 0x10), (0x01, 0x20), (0x100, 0x0400), (0x200, 0x0800), (0x40000, 0x1000)];
    for (public, hardware) in mappings {
        if mask & public != 0 && public != 0x80000 {
            control = (control & !hardware) | if value & public != 0 { hardware } else { 0 };
        }
    }
    let precision = match control & 0x0300 { 0x0000 => 0x20000, 0x0200 => 0x10000, _ => 0 };
    let precision = (precision & !(mask & 0x30000)) | (value & mask & 0x30000);
    control = (control & !0x0300) | match precision { 0x20000 => 0, 0x10000 => 0x0200, _ => 0x0300 };
    state.x87_control = control;
    Ok(u64::from(mappings.into_iter().filter(|(_, hardware)| control & hardware != 0)
        .fold(precision, |bits, (public, _)| bits | public)))
}

#[cfg(test)]
mod tests {
    use super::split_command;

    #[test]
    fn msvcrt_command_line_preserves_empty_arguments_and_escaped_quotes() {
        assert_eq!(split_command(r#""C:\Program Files\app.exe" "" "a b" c\"d "x\\""#),
            [r"C:\Program Files\app.exe", "", "a b", "c\"d", "x\\"]);
    }
}
