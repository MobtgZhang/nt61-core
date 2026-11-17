//! Classic event log source handles and checked ReportEvent A/W capture.
//! https://learn.microsoft.com/windows/win32/api/winbase/nf-winbase-registereventsourcew
//! https://learn.microsoft.com/windows/win32/api/winbase/nf-winbase-reporteventw
use super::*;

#[derive(Default)]
pub(super) struct State {
    sources: Vec<(u64, Vec<u16>)>,
    next: u32,
}

pub(super) fn handles(name: &str) -> bool {
    matches!(name, "RegisterEventSourceW" | "RegisterEventSourceA" |
        "DeregisterEventSource" | "ReportEventW" | "ReportEventA")
}

fn text(image: &mut GuestImage, address: u64, wide: bool) -> Result<Vec<u16>, ApiError> {
    let mut units = Vec::new();
    let width = if wide { 2 } else { 1 };
    for index in 0..32768u64 {
        let address = address.checked_add(index * width).ok_or(NOACCESS)?;
        let value = word(&mut image.memory, address, width as usize)? as u16;
        if value == 0 { return Ok(units); }
        units.push(if wide { value } else { nls::decode_ansi(value as u8) });
    }
    Err(INVALID_PARAMETER.into())
}

pub(super) fn invoke(image: &mut GuestImage, name: &str, a: &[u64],
    services: &mut dyn GuestServices) -> Result<u64, ApiError> {
    let wide = name.ends_with('W');
    if name.starts_with("RegisterEventSource") {
        if a[0] != 0 && !text(image, a[0], wide)?.is_empty() {
            return Err(1722u32.into()); // No remote event log RPC transport.
        }
        let source = text(image, a[1], wide)?;
        if source.is_empty() || source.len() > 254 { return Err(INVALID_PARAMETER.into()); }
        if !services.application_log_available() { return Err(120u32.into()); }
        let state = &mut image.win32.eventlog;
        if state.sources.len() >= 64 { return Err(8u32.into()); }
        let next = state.next.checked_add(1).filter(|n| *n <= 0x00ff_ffff).ok_or(8u32)?;
        let handle = 0x4600_0000 | u64::from(next);
        state.sources.push((handle, source));
        state.next = next;
        return Ok(handle);
    }
    let index = image.win32.eventlog.sources.iter().position(|(handle,_)| *handle == a[0]).ok_or(6u32)?;
    if name == "DeregisterEventSource" {
        image.win32.eventlog.sources.remove(index);
        return Ok(1);
    }
    let event_type = a[1] as u16;
    if !matches!(event_type, 0 | 1 | 2 | 4 | 8 | 16) { return Err(INVALID_PARAMETER.into()); }
    let count = a[5] as u16 as usize;
    let data_len = a[6] as u32 as usize;
    // Win7 event data limit; larger complete records fail with INVALID_PARAMETER.
    if data_len > 61440 { return Err(INVALID_PARAMETER.into()); }
    let mut sid = Vec::new();
    if a[4] != 0 {
        let header = read(&mut image.memory, a[4], 8)?;
        if header[0] != 1 || header[1] > 15 { return Err(1337u32.into()); }
        sid = read(&mut image.memory, a[4], 8 + usize::from(header[1]) * 4)?;
    }
    let width = if image.thread.context.mode == DecodeMode::X86 {4} else {8};
    let pointers = if count == 0 { Vec::new() } else { read(&mut image.memory,a[7],count * width)? };
    let mut strings = Vec::new();
    let mut total = data_len + sid.len() + 64;
    for pointer in pointers.chunks_exact(width) {
        let mut raw = [0u8;8];
        raw[..width].copy_from_slice(pointer);
        let value = text(image,u64::from_le_bytes(raw),wide)?;
        total = total.checked_add((value.len()+1)*2).ok_or(INVALID_PARAMETER)?;
        if total > 0x3ffff { return Err(INVALID_PARAMETER.into()); }
        strings.push(value);
    }
    let data = if data_len == 0 { Vec::new() } else { read(&mut image.memory,a[8],data_len)? };
    let event = nt61::ApplicationEvent {
        source: image.win32.eventlog.sources[index].1.clone(),
        event_type, category: a[2] as u16, event_id: a[3] as u32, sid, strings, data,
    };
    services.report_application_event(event)?;
    Ok(1)
}
