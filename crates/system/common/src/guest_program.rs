//! Mounted-volume x86/x64 console PE execution using the portable backend.
//! This is a bounded command invocation, not a substitute for a full Win32
//! process (GUI dispatch, SEH and guest thread creation
//! remain separate). Normal imports link only to checked guest DLL code.
use crate::arch::common::btl::{
    io::{IoHost, IoServices},
    nt61::{DcExtent, SocketKind},
    pe::ImageRunResult,
    runtime::StopReason,
};
use alloc::{boxed::Box, format, string::String, vec::Vec};

struct VolumeIo {
    output: fn(&[u8]),
    desktop_client: u32,
    desktop_windows:bool,
    application_directory: String,
}
impl Drop for VolumeIo {
    fn drop(&mut self) {
        if self.desktop_windows {
            let mut packet = Box::new(nt61_abi::dialog_bridge::EMPTY);
            packet.values[0] = nt61_abi::dialog_bridge::WINDOW;
            packet.values[4] = 8;
            packet.values[63] = self.desktop_client;
            let _ = crate::arch::common::dialog_broker::request(packet);
        }
    }
}
fn socket_error(error: crate::netstack::socket::SocketError) -> u32 {
    use crate::netstack::socket::SocketError;

    match error {
        SocketError::WouldBlock => 10035,
        SocketError::ConnectionRefused => 10061,
        SocketError::ConnectionReset => 10054,
        SocketError::NotConnected => 10057,
        SocketError::InvalidSocket => 10038,
        SocketError::AddressInUse => 10048,
        SocketError::AddressNotAvailable => 10049,
        SocketError::InvalidArgument => 10022,
        SocketError::OutOfMemory => 10055,
    }
}

impl IoHost for VolumeIo {
    fn resolve_ipv4(&mut self, name: &str) -> Result<[u8; 4], u32> {
        use crate::netstack::dns::{resolve, DnsError};

        resolve(name).map(u32::to_be_bytes).map_err(|error| match error {
            DnsError::Timeout => 11002,
            DnsError::NotFound => 11001,
            DnsError::AddrFamilyNotSupported => 10047,
            DnsError::Format | DnsError::Other => 11003,
        })
    }

    fn open_socket(&mut self, kind: SocketKind) -> Result<u32, u32> {
        use crate::netstack::socket::{socket_auto, SocketType};

        let socket_type = match kind {
            SocketKind::TcpV4 => SocketType::Stream,
            SocketKind::UdpV4 => SocketType::Dgram,
        };
        socket_auto(socket_type).ok_or(10055)
    }

    fn close_socket(&mut self, socket: u32) -> Result<(), u32> {
        crate::netstack::socket::close_owned(socket).map_err(socket_error)
    }

    fn connect_tcp(&mut self, socket: u32, ip: [u8; 4], port: u16) -> Result<(), u32> {
        use crate::netstack::socket::{connect, SockAddr};

        connect(socket, &SockAddr::new(2, port, u32::from_be_bytes(ip))).map_err(socket_error)
    }

    fn send_tcp(&mut self, socket: u32, bytes: &[u8]) -> Result<usize, u32> {
        crate::netstack::socket::send(socket, bytes).map_err(socket_error)
    }

    fn recv_tcp(&mut self, socket: u32, bytes: &mut [u8]) -> Result<usize, u32> {
        crate::drivers::net::poll_all();
        crate::netstack::socket::recv(socket, bytes).map_err(socket_error)
    }

    fn message_beep(&mut self, kind: u32) -> bool {
        crate::drivers::audio::system_sounds::message_beep(kind)
    }

    fn create_memory_dc(&mut self, source: u64) -> Option<u64> {
        let dc = crate::libs::win32k::dc::gre_create_compatible_dc(source);
        (dc != 0).then_some(dc)
    }

    fn delete_memory_dc(&mut self, dc: u64) -> bool {
        crate::libs::win32k::dc::gre_delete_dc(dc)
    }

    fn device_caps(&mut self, dc: u64, index: i32) -> Option<u32> {
        crate::libs::win32k::dc::caps::device_caps(dc, index)
    }

    fn draw_text_layout(&mut self, dc: u64, clip: Option<[i32; 4]>, lines: &[(i32, i32, &[u16])]) -> bool {
        crate::libs::win32k::font_services::draw_text_layout(dc, clip, lines)
    }

    fn create_font_w(&mut self, description: &[u8; 92]) -> Option<u64> {
        let font = crate::libs::win32k::font_services::create_logfont_w(description);
        (font != 0).then_some(font)
    }

    fn select_font(&mut self, dc: u64, font: u64) -> Option<u64> {
        use crate::libs::win32k::{dc, objects};
        let default = objects::gdi_get_stock_object(13);
        if default == 0 { return None; }
        dc::get_dc(dc)?;
        if dc::gre_get_current_font(dc) == 0 {
            dc::select_typed_object(dc, default, Some(objects::GdiObjectType::Font));
        }
        let previous = dc::select_typed_object(dc, font, Some(objects::GdiObjectType::Font));
        (previous != 0).then_some(previous)
    }

    fn delete_font(&mut self, font: u64) -> bool {
        crate::libs::win32k::objects::gdi_delete_object(font)
    }

    fn enumerate_fonts_w(&mut self, dc: u64, family: Option<&str>) -> Option<Vec<crate::arch::common::btl::nt61::FontEnumeration>> {
        crate::libs::win32k::font_services::enumerate_fonts_w(dc, family)
    }

    fn text_metrics_w(&mut self, dc: u64) -> Option<[u8; 60]> {
        crate::libs::win32k::font_services::text_metrics_w(dc)
    }

    fn text_face_w(&mut self, dc: u64) -> Option<Vec<u16>> {
        crate::libs::win32k::font_services::text_face_w(dc)
    }

    fn text_extent_w(&mut self, dc: u64, text: &[u16]) -> Option<[i32; 2]> {
        crate::libs::win32k::font_services::text_extent_w(dc, text)
    }

    fn set_bk_mode(&mut self, dc: u64, mode: i32) -> Option<i32> {
        crate::libs::win32k::dc::set_bk_mode(dc, mode)
    }

    fn set_map_mode(&mut self, dc: u64, mode: i32) -> Option<i32> {
        Some(crate::libs::win32k::dc::gre_set_map_mode(dc, mode))
    }

    fn set_dc_extent(&mut self, dc: u64, kind: DcExtent, [x, y]: [i32; 2]) -> Option<(bool, [i32; 2])> {
        use crate::libs::win32k::dc::{gre_set_viewport_ext_ex, gre_set_window_ext_ex, SizeL};
        let mut previous = SizeL::new(0, 0);
        let success = match kind {
            DcExtent::Window => gre_set_window_ext_ex(dc, x, y, Some(&mut previous)),
            DcExtent::Viewport => gre_set_viewport_ext_ex(dc, x, y, Some(&mut previous)),
        };
        Some((success, [previous.cx, previous.cy]))
    }

    fn logical_to_device(&mut self, dc: u64, points: &mut [u8]) -> bool {
        crate::libs::win32k::transforms::logical_to_device(dc, points)
    }

    fn create_event(&mut self, manual: bool, signaled: bool) -> Result<u64, u32> {
        crate::libs::ntdll::sync::create_event_kernel(u32::from(!manual), signaled)
            .map(|handle| handle as u64).map_err(|status| status as u32)
    }
    fn change_event(&mut self, event: u64, signaled: bool) -> Result<(), u32> {
        // SAFETY: IoServices supplies only its owned backend handle. The
        // optional previous-state pointer is null; no guest pointer enters NT.
        let status = unsafe {
            if signaled { crate::libs::ntdll::sync::nt_set_event(event as _, core::ptr::null_mut()) }
            else { crate::libs::ntdll::sync::nt_reset_event(event as _, core::ptr::null_mut()) }
        };
        if status < 0 { Err(status as u32) } else { Ok(()) }
    }
    fn wait_event(&mut self, event: u64, timeout: Option<i64>) -> Result<u32, u32> {
        let status = crate::libs::ntdll::sync::nt_wait_for_single_object_kernel(event as _, 0, timeout);
        if status < 0 { Err(status as u32) } else { Ok(status as u32) }
    }
    fn poll_events(&mut self, events: &[u64], all: bool) -> Result<u32, u32> {
        let mut handles = [core::ptr::null_mut(); 64];
        let handles = handles.get_mut(..events.len()).ok_or(0xc000_000du32)?;
        for (handle, event) in handles.iter_mut().zip(events) {
            *handle = *event as _;
        }
        let status = crate::libs::ntdll::sync::nt_wait_for_multiple_objects_kernel(
            handles.len() as u32, handles, u32::from(!all), 0, Some(0),
        );
        if status < 0 { Err(status as u32) } else { Ok(status as u32) }
    }
    fn close_event(&mut self, event: u64) -> Result<(), u32> {
        // SAFETY: this is an owned opaque native handle, not a guest address.
        let status = unsafe { crate::libs::ntdll::file::nt_close(event as _) };
        if status < 0 { Err(status as u32) } else { Ok(()) }
    }
    fn create_directory(&mut self, path: &str) -> Result<(), u32> {
        crate::fs::system_volume::mkdir(path)
    }
    fn file_attributes(&mut self, path: &str) -> Result<u32, u32> {
        crate::fs::system_volume::file_attributes(path)
    }
    fn set_file_attributes(&mut self, path: &str, attributes: u32) -> Result<(), u32> {
        crate::fs::system_volume::set_file_attributes(path, attributes)
    }
    fn set_file_times(&mut self, path: &str, times: [Option<u64>; 3]) -> Result<(), u32> {
        crate::fs::system_volume::set_file_times(path, times)
    }
    fn directory_entries(&mut self, path: &str) -> Result<Vec<crate::arch::common::btl::nt61::DirectoryEntry>, u32> {
        Ok(crate::fs::system_volume::directory_metadata(path)?.into_iter().map(|entry| {
            crate::arch::common::btl::nt61::DirectoryEntry {
                name: entry.name, attributes: entry.file_attributes,
                size: entry.file_size, creation_time: entry.creation_time,
                access_time: entry.access_time, write_time: entry.modification_time,
            }
        }).collect())
    }
    fn application_log_available(&mut self) -> bool { true }
    fn report_application_event(&mut self, event: crate::arch::common::btl::nt61::ApplicationEvent) -> Result<(), u32> {
        crate::rtl::eventlog::classic::report(event, crate::ke::time::get_system_time())
    }

    fn native_processor_info(&mut self) -> Result<crate::arch::common::btl::nt61::NativeProcessorInfo, u32> {
        #[cfg(target_arch = "x86_64")]
        {
            // SAFETY: CPUID is available on every x86-64 processor; leaf 1
            // is a read-only processor identification query.
            let signature = core::arch::x86_64::__cpuid(1).eax;
            let family = (signature >> 8) & 15;
            let model = (signature >> 4) & 15;
            let level = family + if family == 15 {(signature >> 20) & 255} else {0};
            let model = model | if family == 6 || family == 15 {(signature >> 12) & 0xf0} else {0};
            return Ok(crate::arch::common::btl::nt61::NativeProcessorInfo {
                architecture: 9, processor_type: 8664, level: level as u16,
                revision: ((model << 8) | (signature & 15)) as u16,
            });
        }
        #[cfg(not(target_arch = "x86_64"))]
        Err(0xc0000002)
    }
    fn unbiased_interrupt_time(&mut self) -> Result<u64, u32> {
        // This counter advances on running timer ticks only. There is no
        // suspended-time correction applied to it on this kernel.
        Ok(crate::ke::time::get_interrupt_time())
    }
    fn time_adjustment(&mut self) -> Result<crate::arch::common::btl::nt61::TimeAdjustment, u32> {
        // advance_tick advances both clocks by this interval. The kernel has
        // no periodic clock adjustment, so automatic adjustment is disabled.
        let increment = crate::ke::time::HUNDRED_NS_PER_MS as u32;
        Ok(crate::arch::common::btl::nt61::TimeAdjustment {
            adjustment: increment,
            increment,
            disabled: true,
        })
    }
    fn random_bytes(&mut self, bytes: &mut [u8]) -> Result<(), u32> {
        // The architecture checks hardware feature availability and retries
        // transient failures. No clock, address or fixed-seed fallback.
        for chunk in bytes.chunks_mut(8) {
            let word = crate::arch::hardware_random_u64().ok_or(0xc00000bbu32)?;
            chunk.copy_from_slice(&word.to_le_bytes()[..chunk.len()]);
        }
        Ok(())
    }
    fn printer_available(&mut self) -> bool { true }
    fn print_document(&mut self, data: &[u8]) -> Result<u32,u32> { crate::arch::common::dialog_broker::spool(data) }

    fn desktop_dialog(&mut self, request: &[u8]) -> Result<Vec<u8>, u32> {
        let mut packet = crate::arch::common::dialog_broker::decode(request)?;
        if packet.values[0]==nt61_abi::dialog_bridge::WINDOW{self.desktop_windows=true;}
        packet.values[63] = self.desktop_client;
        let response = crate::arch::common::dialog_broker::request(packet)?;
        Ok(crate::arch::common::dialog_broker::bytes(&response).to_vec())
    }

    fn memory_status(&mut self) -> Result<crate::arch::common::btl::nt61::MemoryStatus, u32> {
        Ok(crate::arch::common::btl::nt61::MemoryStatus {
            physical_total: crate::mm::get_total_physical(),
            physical_available: crate::mm::get_free_physical(),
            paging_total: crate::mm::pagefile::get_total_pagefile_pages().saturating_mul(4096),
            paging_available: crate::mm::pagefile::get_total_free_pagefile_pages()
                .saturating_mul(4096),
        })
    }
    fn processor_mask(&mut self) -> Result<u64, u32> {
        let count = crate::arch::common::smp::cpu_count().min(64);
        if count == 0 {
            return Err(0xc0000001);
        }
        Ok(u64::MAX >> (64 - count))
    }
    fn clocks(&mut self) -> Result<(u64, u64), u32> {
        Ok((
            crate::ke::time::get_system_time(),
            crate::ke::time::get_interrupt_time(),
        ))
    }
    fn delay_execution(&mut self, alertable: bool, interval: i64) -> u32 {
        crate::libs::ntdll::sync::nt_delay_execution_kernel(u8::from(alertable), interval) as u32
    }
    fn read_library(
        &mut self,
        name: &str,
        mode: crate::arch::common::btl::ir::DecodeMode,
    ) -> Result<Vec<u8>, u32> {
        use crate::arch::common::btl::pe::ImportResolver;
        crate::userspace::loader::VolumeLibraries {
            application_directory: &self.application_directory,
        }
        .read_library(name, mode)
    }
    fn read_system_library(
        &mut self,
        name: &str,
        mode: crate::arch::common::btl::ir::DecodeMode,
    ) -> Result<Vec<u8>, u32> {
        crate::userspace::loader::VolumeLibraries {
            application_directory: &self.application_directory,
        }
        .resolve_system_library(name, mode)
        .map(|library| library.bytes)
    }
    fn resolve_library(
        &mut self,
        name: &str,
        mode: crate::arch::common::btl::ir::DecodeMode,
        system: bool,
    ) -> Result<crate::arch::common::btl::pe::ResolvedLibrary, u32> {
        use crate::arch::common::btl::pe::ImportResolver;
        let mut libraries = crate::userspace::loader::VolumeLibraries {
            application_directory: &self.application_directory,
        };
        if system { libraries.resolve_system_library(name, mode) }
        else { libraries.resolve_library(name, mode) }
    }
    fn open_read(&mut self, path: &str) -> Result<(), u32> {
        // This read validates file existence through the mounted FS without
        // loading its complete contents or opening directories as files.
        crate::fs::system_volume::read_at(path, 0, 1).map(|_| ())
    }
    fn open_file(&mut self, path: &str, disposition: u32, writable: bool, allow_directory: bool) -> Result<(u64, bool), u32> {
        crate::fs::system_volume::open_guest_file(path, disposition, writable, allow_directory)
    }
    fn file_size(&mut self, path: &str) -> Result<u64, u32> {
        crate::fs::system_volume::file_size(path)
    }
    fn read_at(&mut self, path: &str, offset: u64, len: usize) -> Result<Vec<u8>, u32> {
        crate::fs::system_volume::read_at(path, offset, len)
    }
    fn write_at(&mut self, path: &str, offset: u64, bytes: &[u8]) -> Result<usize, u32> {
        crate::fs::system_volume::write_at(path, offset, bytes)
    }
    fn console_write(&mut self, bytes: &[u8]) -> Result<(), u32> {
        (self.output)(bytes);
        Ok(())
    }
}

/// `None` means no executable was found. An existing but unsupported PE returns
/// an error and diagnostic, never a fake successful process or native jump.
pub fn try_execute(
    line: &str,
    directory: &str,
    search_path: &str,
    output: fn(&[u8]),
) -> Option<u32> {
    let line = line.trim();
    let name = if let Some(quoted) = line.strip_prefix('"') {
        let (name, tail) = quoted.split_once('"')?;
        if !tail.is_empty() && !tail.starts_with(char::is_whitespace) {
            return None;
        }
        name
    } else {
        line.split_whitespace().next()?
    };
    let leaf = name.rsplit(['\\', '/']).next()?;
    let suffixes: &[&str] = match leaf.rsplit_once('.') {
        Some((_, extension)) if extension.eq_ignore_ascii_case("exe")
            || extension.eq_ignore_ascii_case("com") => &[""],
        Some(_) => return None,
        None => &[".com", ".exe"],
    };
    let mut candidates = Vec::new();
    if name.contains(['\\', '/']) || name.contains(':') {
        let absolute = if name.as_bytes().get(1) == Some(&b':') || name.starts_with(['\\', '/']) {
            String::from(name)
        } else {
            format!("{directory}\\{name}")
        };
        candidates.extend(suffixes.iter().map(|suffix| format!("{absolute}{suffix}")));
    } else {
        let directories = core::iter::once(directory).chain(search_path.split(';').filter(|entry| !entry.is_empty()));
        for entry in directories {
            let entry = entry.trim_matches('"');
            candidates.extend(suffixes.iter().map(|suffix| format!("{entry}\\{name}{suffix}")));
        }
    }
    for candidate in candidates {
        let Ok(candidate) = crate::fs::system_volume::normalize(&candidate) else {
            continue;
        };
        let header = match crate::fs::system_volume::read_at(&candidate, 0, 64) {
            Ok(header) => header,
            Err(_) => continue,
        };
        if header.len() < 64 || &header[..2] != b"MZ" {
            if candidate.rsplit_once('.').is_some_and(|(_, extension)| extension.eq_ignore_ascii_case("com")) {
                crate::hal::serial::write_string_always(&format!("[PE] Legacy COM execution is unavailable: {candidate}\r\n"));
            }
            output(b"The file is not a valid Windows executable.\r\n");
            return Some(193);
        }
        crate::hal::serial::write_string_always(&format!("[PE] Loading {candidate}\r\n"));
        let mut image =
            match crate::userspace::loader::load_translated_image(&candidate, line, directory) {
                Ok(image) => image,
                Err(error) => {
                    let status = error.ntstatus();
                    let diagnostic = format!(
                        "Cannot execute {candidate}: {error}; NTSTATUS 0x{status:08X}.\r\n"
                    );
                    crate::hal::serial::write_string_always("[PE] ");
                    crate::hal::serial::write_string_always(&diagnostic);
                    output(diagnostic.as_bytes());
                    return Some(status);
                }
            };
        crate::hal::serial::write_string_always(&format!("[PE] Running {candidate}\r\n"));
        static NEXT_DESKTOP_CLIENT: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(1);
        let mut services = IoServices::new(VolumeIo {
            desktop_client: NEXT_DESKTOP_CLIENT.fetch_add(1,core::sync::atomic::Ordering::Relaxed),
            desktop_windows:false,
            output,
            application_directory: candidate
                .rsplit_once('\\')
                .map_or(directory, |(parent, _)| parent)
                .into(),
        });
        // Bounded maintenance/desktop console execution. Every quantum lets
        // runnable kernel/desktop threads proceed; no global BTL lock is held.
        // Large SFX installers need more than 8M instructions just to inspect
        // their embedded archive. Keep a finite 128M-instruction cap and
        // retain a scheduler yield after every 8192-instruction quantum.
        let execution_started = crate::ke::time::get_interrupt_time();
        let mut run_elapsed = 0u64;
        let mut publisher = crate::mm::jit_code::KernelPublisher;
        let mut jit = crate::arch::common::btl::runtime::jit::Jit::new(&mut publisher);
        for quantum in 0..16384 {
            if quantum != 0 && quantum % 128 == 0 {
                crate::hal::serial::write_string_always(&format!(
                    "[PE] Progress {candidate}: retired={} pc=0x{:X}\r\n",
                    image.retired(), image.thread.context.rip,
                ));
            }
            let run_started = crate::ke::time::get_interrupt_time();
            let result = image.run_with_jit(8192, &mut services, &mut jit);
            run_elapsed = run_elapsed.saturating_add(
                crate::ke::time::get_interrupt_time().saturating_sub(run_started));
            if !matches!(result, ImageRunResult::Yielded) {
                crate::hal::serial::write_string_always(&format!(
                    "[PE] Translation {candidate}: native={} interpreted={}\r\n",
                    jit.retired(), image.retired().saturating_sub(jit.retired()),
                ));
                crate::hal::serial::write_string_always(&format!(
                    "[PE] Timing {candidate}: run_elapsed_ms={} wall_elapsed_ms={} batches={}\r\n",
                    run_elapsed / 10_000,
                    crate::ke::time::get_interrupt_time().saturating_sub(execution_started) / 10_000,
                    quantum + 1,
                ));
            }
            match result {
                ImageRunResult::Exited(code) => {
                    crate::hal::serial::write_string_always(&format!("[PE] Guest exited {candidate}: {code}\r\n"));
                    if code != 0 {
                        crate::hal::serial::write_string_always(&format!(
                            "[PE] Exit context {candidate}: {:?}\r\n", image.win32.exit_context,
                        ));
                        for (id, error, detail) in &image.win32.failures {
                            let name = crate::arch::common::btl::pe::win32::catalog::resolve(*id)
                                .map_or("unknown", |api| api.name);
                            crate::hal::serial::write_string_always(&format!(
                                "[PE] Recent API failure {name} ({id}): {error} {detail}\r\n",
                            ));
                        }
                    }
                    return Some(code);
                }
                ImageRunResult::Yielded => crate::ke::scheduler::yield_(),
                ImageRunResult::Fault(reason) => {
                    use crate::arch::common::btl::memory::GuestMemory;
                    let mut bytes = [0; 15];
                    let mut count = 0;
                    while count < bytes.len() && image.memory.fetch_virt(
                        image.thread.context.rip.wrapping_add(count as u64),
                        &mut bytes[count..count + 1],
                    ) == 1 { count += 1; }
                    let status = match reason {
                        StopReason::DataFault { .. } | StopReason::InstructionFault { .. } => 0xc0000005,
                        StopReason::DivideError { .. } => 0xc0000094,
                        StopReason::FloatingPointError { exceptions, .. } => {
                            if exceptions & 1 != 0 { 0xc0000090 }
                            else if exceptions & 2 != 0 { 0xc000008d }
                            else if exceptions & 4 != 0 { 0xc000008e }
                            else if exceptions & 8 != 0 { 0xc0000091 }
                            else if exceptions & 16 != 0 { 0xc0000093 }
                            else { 0xc000008f }
                        }
                        StopReason::InitializationFailed { .. } => 0xc0000142,
                        StopReason::UnsupportedApi { .. } => 0xc0000002,
                        _ => 0xc000001d,
                    };
                    let api = image
                        .win32
                        .last_api
                        .and_then(crate::arch::common::btl::pe::win32::catalog::resolve)
                        .map(|api| format!("{}!{}", api.dll, api.name))
                        .unwrap_or_default();
                    let diagnostic = format!(
                        "Guest execution fault in {candidate}: {reason:?}; API {api}; NTSTATUS 0x{status:08X}.\r\n"
                    );
                    crate::hal::serial::write_string_always("[PE] ");
                    crate::hal::serial::write_string_always(&diagnostic);
                    output(diagnostic.as_bytes());
                    if let Some((dll, symbol)) = crate::arch::common::btl::pe::win32::delay_load_fault(&mut image) {
                        crate::hal::serial::write_string_always(&format!(
                            "[PE] Delay-load fault in {candidate}: {dll}!{symbol}\r\n"
                        ));
                    }
                    crate::hal::serial::write_string_always(&format!(
                        "[PE] Fault details: retired={} bytes={:02X?} last_api_arguments={:X?} recent_api_failures={:?}\r\n",
                        image.retired(), &bytes[..count], image.win32.last_arguments, image.win32.failures,
                    ));
                    return Some(status);
                }
            }
        }
        let diagnostic = format!("[PE] Guest instruction budget exceeded in {candidate}; console invocation stopped.\r\n");
        crate::hal::serial::write_string_always(&diagnostic);
        output(diagnostic.as_bytes());
        return Some(1460);
    }
    None
}
