//! Mounted-volume x86/x64 console PE execution using the portable backend.
//! This is a bounded command invocation, not a substitute for a full Win32
//! process (GUI dispatch, SEH and guest thread creation
//! remain separate). Normal imports link only to checked guest DLL code.
use crate::arch::common::btl::{
    io::{guest_file_path, FileLockKey, IoHost, IoServices, OpenedFile},
    ir::DecodeMode,
    nt61::{ChildProcess, ChildRequest, DcExtent, FileBasicInformation, SocketKind, VolumeAllocation},
    pe::{GuestImage, ImageRunResult},
    runtime::StopReason,
};
use alloc::{boxed::Box, format, string::String, sync::Arc, vec::Vec};
use crate::ps::impersonation::ImpersonationReference;
use core::fmt::Display;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

struct VolumeIo {
    attempt: GuestAttempt,
    identity: Option<ImpersonationReference>,
    output: fn(&[u8]),
    desktop_client: u32,
    desktop_windows:bool,
    application_directory: String,
}

struct ChildJob {
    attempt: GuestAttempt,
    path: String,
    image: GuestImage,
    services: IoServices<VolumeIo>,
    exit: Arc<AtomicU64>,
    thread_exit: Arc<AtomicU64>,
    _slot: ChildSlot,
}

static ACTIVE_CHILDREN: AtomicU32 = AtomicU32::new(0);

#[derive(Clone, Copy)]
struct GuestAttempt(u64);

impl GuestAttempt {
    fn start(path: &str) -> Self {
        static NEXT_ATTEMPT: AtomicU64 = AtomicU64::new(1);
        let attempt = Self(NEXT_ATTEMPT.fetch_add(1, Ordering::Relaxed));
        attempt.log(format_args!("Loading {path}"));
        attempt
    }

    fn log(self, event: impl Display) {
        crate::hal::serial::write_string_always(&format!("[PE] {event} [run={}]\r\n", self.0));
    }
}

fn report_progress_periodically(
    last: &mut u64,
    attempt: GuestAttempt,
    path: &str,
    image: &GuestImage,
) {
    let now = crate::ke::time::get_interrupt_time();
    if now.saturating_sub(*last) >= 20_000_000 {
        *last = now;
        report_guest_progress(attempt, path, image);
    }
}

struct ChildSlot;

impl ChildSlot {
    fn reserve() -> Result<Self, u32> {
        ACTIVE_CHILDREN.try_update(Ordering::Relaxed, Ordering::Relaxed, |active| {
            (active < 16).then_some(active + 1)
        }).map_err(|_| 89u32)?;
        Ok(Self)
    }
}

impl Drop for ChildSlot {
    fn drop(&mut self) {
        ACTIVE_CHILDREN.fetch_sub(1, Ordering::Relaxed);
    }
}

extern "C" fn run_child(context: u64) -> ! {
    let mut job = unsafe { Box::from_raw(context as *mut ChildJob) };
    let attempt = job.attempt;
    report_guest_context(attempt, &job.path, &job.image);
    attempt.log(format_args!("Running {}", job.path));
    let mut publisher = crate::mm::jit_code::KernelPublisher;
    let mut jit = crate::arch::common::btl::runtime::jit::Jit::new(&mut publisher);
    let mut status = 0xc000_0044;
    let mut finished = false;
    let mut last_progress = crate::ke::time::get_interrupt_time();
    for _ in 0..16384 {
        report_progress_periodically(&mut last_progress, attempt, &job.path, &job.image);
        let result = job.image.run_with_jit(8192, &mut job.services, &mut jit);
        if let Some(code) = job.image.primary_thread_exit_code() {
            job.thread_exit.store(u64::from(code), Ordering::Release);
        }
        match result {
            ImageRunResult::Yielded => crate::ke::scheduler::yield_(),
            ImageRunResult::Exited(code) => {
                status = code;
                attempt.log(format_args!("Guest exited {}: {code}", job.path));
                finished = true;
                break;
            }
            ImageRunResult::Fault(reason) => {
                status = 0xc000_0142;
                attempt.log(format_args!(
                    "Guest execution fault in {}: {reason:?}; NTSTATUS 0x{status:08X}.", job.path,
                ));
                finished = true;
                break;
            }
        }
    }
    if !finished {
        attempt.log(format_args!("Guest instruction budget exceeded in {}; child invocation stopped.", job.path));
    }
    drop(jit);
    let exit = Arc::clone(&job.exit);
    let thread_exit = Arc::clone(&job.thread_exit);
    drop(job);
    let _ = thread_exit.compare_exchange(u64::MAX, u64::from(status), Ordering::Release, Ordering::Relaxed);
    exit.store(u64::from(status), Ordering::Release);
    drop(thread_exit);
    drop(exit);
    crate::ps::thread::ps_exit_system_thread(status)
}

impl VolumeIo {
    fn new(attempt: GuestAttempt, path: &str, directory: &str, output: fn(&[u8]),
        identity: Option<ImpersonationReference>) -> Self {
        static NEXT_DESKTOP_CLIENT: AtomicU32 = AtomicU32::new(1);
        Self {
            attempt,
            identity,
            output,
            desktop_client: NEXT_DESKTOP_CLIENT.fetch_add(1, Ordering::Relaxed),
            desktop_windows: false,
            application_directory: path.rsplit_once('\\').map_or(directory, |(parent, _)| parent).into(),
        }
    }

    fn file_identity(&self) -> Result<&crate::se::token::Token, u32> {
        use crate::libs::ntdll::status::{STATUS_ACCESS_DENIED, STATUS_BAD_IMPERSONATION_LEVEL};
        use crate::se::token::ImpersonationLevel;

        let identity = self.identity.as_ref().ok_or(STATUS_ACCESS_DENIED as u32)?;
        if matches!(identity.level, ImpersonationLevel::SecurityAnonymous | ImpersonationLevel::SecurityIdentification) {
            return Err(STATUS_BAD_IMPERSONATION_LEVEL as u32);
        }
        Ok(identity.token.token())
    }

    fn open_security_target(&self, path: &str, desired: u32) -> Result<(
        crate::fs::ntfs::security_target::FileSecurityTarget,
        crate::arch::common::btl::io::FileShareLease,
    ), u32> {
        use crate::libs::ntdll::status::STATUS_NOT_SUPPORTED;
        let token = self.file_identity()?;
        let (file, _, _, lease) = crate::fs::system_volume::open_native_file(path, 1, 0,
            |descriptor, access| crate::libs::ntdll::access_check::check_file_descriptor(descriptor, token, access)
                .map_err(|status| status as u32), desired, 7)?;
        Ok((file.security_target.ok_or(STATUS_NOT_SUPPORTED as u32)?, lease))
    }

    fn resolve_child(&self, request: &ChildRequest) -> Result<String, u32> {
        let line = request.command_line.as_deref().unwrap_or_default().trim_start();
        if let Some(name) = request.application.as_deref() {
            return self.find_executable(request, name, false)?.ok_or(2);
        }
        if let Some(quoted) = line.strip_prefix('"') {
            let name = quoted.split_once('"').ok_or(87u32)?.0;
            return self.find_executable(request, name, true)?.ok_or(2);
        }
        for end in line.match_indices([' ', '\t']).map(|(offset, _)| offset).chain([line.len()]) {
            let name = &line[..end];
            if name.ends_with([' ', '\t']) || name.contains('"') { continue; }
            if let Some(path) = self.find_executable(request, name, true)? { return Ok(path); }
        }
        Err(2)
    }

    fn find_executable(&self, request: &ChildRequest, name: &str, search: bool) -> Result<Option<String>, u32> {
        if name.is_empty() { return Err(2); }
        let directory = &request.parent_directory;
        let name = name.replace('/', "\\");
        let leaf = name.rsplit('\\').next().unwrap_or_default();
        let name = if search && !leaf.contains('.') { format!("{name}.exe") } else { name };
        let mut candidates = Vec::new();
        if name.contains(':') {
            candidates.push(name);
        } else if name.starts_with('\\') {
            candidates.push(format!("C:{name}"));
        } else if !search || name.contains('\\') {
            candidates.push(format!("{directory}\\{name}"));
        } else {
            for directory in [self.application_directory.as_str(), directory, "C:\\Windows\\System32", "C:\\Windows\\System", "C:\\Windows"]
                .into_iter().chain(request.search_path.as_deref().unwrap_or_default().split(';').filter(|entry| !entry.is_empty())) {
                candidates.push(format!("{directory}\\{name}"));
            }
        }
        for candidate in candidates {
            let path = guest_file_path(request.abi, &candidate, request.redirect)
                .map_err(file_launch_error)?;
            match crate::fs::system_volume::file_attributes(&path) {
                Ok(flags) if flags & 0x10 == 0 => return Ok(Some(path)),
                Ok(_) | Err(0xc000_000d | 0xc000_0034 | 0xc000_003a) => continue,
                Err(status) => {
                    crate::boot_println!("[BTL process] executable lookup {path}: NTSTATUS 0x{status:08X}");
                    return Err(file_launch_error(status));
                }
            }
        }
        Ok(None)
    }
}

fn file_launch_error(status: u32) -> u32 {
    match status {
        0xc000_000d => 87,
        0xc000_0017 => 8,
        0xc000_0022 => 5,
        0xc000_0034 => 2,
        0xc000_003a => 3,
        0xc000_0135 => 126,
        0xc000_0139 => 127,
        0xc000_007b => 193,
        _ => 1,
    }
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
    fn create_process(&mut self, request: ChildRequest) -> Result<ChildProcess, u32> {
        let slot = ChildSlot::reserve()?;
        let supplied = request.directory.replace('/', "\\");
        if supplied.is_empty() { return Err(267); }
        let directory = if supplied.contains(':') || supplied.starts_with('\\') {
            supplied
        } else { format!("{}\\{supplied}", request.parent_directory) };
        let directory = crate::fs::system_volume::normalize(&directory).map_err(file_launch_error)?;
        let directory = guest_file_path(request.abi, &directory, request.redirect).map_err(file_launch_error)?;
        let attributes = crate::fs::system_volume::file_attributes(&directory).map_err(|status| {
            crate::boot_println!("[BTL process] working directory {directory}: NTSTATUS 0x{status:08X}");
            file_launch_error(status)
        })?;
        if attributes & 0x10 == 0 {
            return Err(267);
        }
        let path = self.resolve_child(&request)?;
        let attempt = GuestAttempt::start(&path);
        self.attempt.log(format_args!("Child run={}", attempt.0));
        let command_line = request.command_line.as_deref().filter(|line| !line.is_empty());
        let default_line;
        let command_line = match command_line {
            Some(line) => line,
            None => { default_line = format!("\"{path}\""); &default_line }
        };
        let mut image = crate::userspace::loader::load_translated_image_with_environment(
            &path, command_line, &directory, request.environment.as_deref(),
            |machine, magic| report_guest_pe(attempt, &path, machine, magic),
        )
        .map_err(|error| {
            let status = error.ntstatus();
            attempt.log(format_args!(
                "Cannot execute {path}: {error}; NTSTATUS 0x{status:08X}.",
            ));
            file_launch_error(status)
        })?;
        image.assign_process_identity().map_err(|_| {
            attempt.log(format_args!("Cannot execute {path}: process identity allocation failed; NTSTATUS 0xC000009A."));
            89u32
        })?;
        let id = image.process_id();
        let thread_id = image.thread_id();
        let exit = Arc::new(AtomicU64::new(u64::MAX));
        let thread_exit = Arc::new(AtomicU64::new(u64::MAX));
        let services = IoServices::new(Self::new(attempt, &path, &directory, self.output, self.identity.clone()));
        let job = Box::into_raw(Box::new(ChildJob { attempt, path, image, services, exit: Arc::clone(&exit), thread_exit: Arc::clone(&thread_exit), _slot: slot }));
        let worker = crate::ps::thread::ps_create_system_thread(run_child as *const () as u64, job as u64);
        if !worker.success {
            let job = unsafe { Box::from_raw(job) };
            attempt.log(format_args!("Cannot execute {}: child worker allocation failed; NTSTATUS 0xC000009A.", job.path));
            return Err(8);
        }
        Ok(ChildProcess { process_id: id, thread_id, exit, thread_exit })
    }

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
        let token = self.file_identity()?;
        crate::fs::system_volume::mkdir_authorized(path, |descriptor, access| {
            crate::libs::ntdll::access_check::check_file_descriptor(descriptor, token, access)
                .map_err(|status| status as u32)
        })
    }
    fn file_attributes(&mut self, path: &str) -> Result<u32, u32> {
        crate::fs::system_volume::file_attributes(path)
    }
    fn file_basic_information(&mut self, path: &str) -> Result<FileBasicInformation, u32> {
        let (attributes, times, size) = crate::fs::system_volume::file_basic_information(path)?;
        Ok(FileBasicInformation { attributes, times, size })
    }
    fn set_file_attributes(&mut self, path: &str, attributes: u32) -> Result<(), u32> {
        crate::fs::system_volume::set_file_attributes(path, attributes)
    }
    fn set_file_times(&mut self, path: &str, times: [Option<u64>; 3]) -> Result<(), u32> {
        crate::fs::system_volume::set_file_times(path, times)
    }
    fn directory_entries(&mut self, path: &str) -> Result<Vec<crate::arch::common::btl::nt61::DirectoryEntry>, u32> {
        let token = self.file_identity()?;
        let entries = crate::fs::system_volume::directory_metadata(path, |descriptor, access| {
            crate::libs::ntdll::access_check::check_file_descriptor(descriptor, token, access)
                .map_err(|status| status as u32)
        })?;
        Ok(entries.into_iter().map(|entry| {
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
    fn open_file_with_access(&mut self, path: &str, disposition: u32, mut options: u32, desired: u32, share: u32) -> Result<OpenedFile, u32> {
        let token = self.file_identity()?;
        if options & (0x4000 | 1) == 0 { options |= 0x40; }
        crate::fs::system_volume::open_guest_file(
            path, disposition, options,
            |descriptor, access| crate::libs::ntdll::access_check::check_file_descriptor(descriptor, token, access)
                .map_err(|status| status as u32), desired, share,
        )
    }
    fn commit_file_copy(&mut self, path: &str, overwrite: bool, contents: &[u8]) -> Result<(), u32> {
        let token = self.file_identity()?;
        crate::fs::system_volume::commit_guest_file_copy(path, overwrite, contents, |descriptor, access| {
            crate::libs::ntdll::access_check::check_file_descriptor(descriptor, token, access)
                .map_err(|status| status as u32)
        })
    }
    fn retain_open_file(&mut self, _path: &str, identity: Option<u64>, _writable: bool) -> Result<(), u32> {
        identity.map_or(Ok(()), crate::fs::system_volume::retain_guest_file)
    }
    fn open_file_size(&mut self, path: &str, identity: Option<u64>) -> Result<u64, u32> {
        crate::fs::system_volume::open_file_size(path, identity)
    }
    fn query_open_file_security(&mut self, record: u64, granted: u32, information: u32) -> Result<Vec<u8>, u32> {
        use crate::libs::ntdll::security::file_object;

        let required = file_object::query_access(information).map_err(|status| status as u32)?;
        if granted & required != required { return Err(0xc000_0022); }
        let target = crate::fs::system_volume::guest_file_security_target(record)?;
        file_object::query(target, information).map_err(|status| status as u32)
    }
    fn check_set_open_file_security(&mut self, record: u64, granted: u32, information: u32) -> Result<(), u32> {
        use crate::libs::ntdll::security::file_object;

        let required = file_object::set_access(information).map_err(|status| status as u32)?;
        if granted & required != required { return Err(0xc000_0022); }
        crate::fs::system_volume::guest_file_security_target(record)?;
        Ok(())
    }
    fn set_open_file_security(&mut self, record: u64, information: u32, descriptor: &[u8]) -> Result<(), u32> {
        let target = crate::fs::system_volume::guest_file_security_target(record)?;
        crate::libs::ntdll::security::file_object::set(target, information, descriptor).map_err(|status| status as u32)
    }
    fn query_file_security(&mut self, path: &str, information: u32) -> Result<Vec<u8>, u32> {
        use crate::libs::ntdll::security::file_object;

        let access = file_object::query_access(information).map_err(|status| status as u32)?;
        let (target, _lease) = self.open_security_target(path, access)?;
        file_object::query(target, information).map_err(|status| status as u32)
    }
    fn set_file_security(&mut self, path: &str, information: u32, descriptor: &[u8]) -> Result<(), u32> {
        use crate::libs::ntdll::security::file_object;

        let access = file_object::set_access(information).map_err(|status| status as u32)?;
        let (target, _lease) = self.open_security_target(path, access)?;
        file_object::set(target, information, descriptor).map_err(|status| status as u32)
    }
    fn close_tracked_file(&mut self, identity: u64) {
        crate::fs::system_volume::close_guest_file(identity);
    }
    fn set_file_range_lock(&mut self, key: FileLockKey, offset: u64, length: u64, lock: bool) -> Result<(), u32> {
        crate::fs::system_volume::set_guest_file_range_lock(key, offset, length, lock)
    }
    fn release_file_range_locks(&mut self, key: FileLockKey) {
        crate::fs::system_volume::release_guest_file_range_locks(key);
    }
    fn delete_file(&mut self, path: &str) -> Result<(), u32> {
        crate::fs::system_volume::delete_guest_file(path)
    }
    fn remove_directory(&mut self, path: &str) -> Result<(), u32> {
        crate::fs::system_volume::remove_guest_directory(path)
    }
    fn move_file(&mut self, source: &str, destination: &str) -> Result<(), u32> {
        crate::fs::system_volume::move_guest_file(source, destination)
    }
    fn file_size(&mut self, path: &str) -> Result<u64, u32> {
        crate::fs::system_volume::file_size(path)
    }
    fn flush_file_buffers(&mut self) -> Result<(), u32> {
        crate::fs::system_volume::flush()
    }
    fn volume_allocation(&mut self, directory: &str) -> Result<VolumeAllocation, u32> {
        let (bytes_per_sector, sectors_per_cluster, total_clusters, free_clusters) =
            crate::fs::system_volume::allocation_units(directory)?;
        Ok(VolumeAllocation { bytes_per_sector, sectors_per_cluster, total_clusters, free_clusters })
    }
    fn read_at(&mut self, path: &str, offset: u64, len: usize) -> Result<Vec<u8>, u32> {
        crate::fs::system_volume::read_at(path, offset, len)
    }
    fn write_at(&mut self, path: &str, offset: u64, bytes: &[u8]) -> Result<usize, u32> {
        crate::fs::system_volume::write_at(path, offset, bytes)
    }
    fn read_open_file(&mut self, path: &str, key: Option<FileLockKey>, offset: u64, length: usize) -> Result<Vec<u8>, u32> {
        crate::fs::system_volume::read_open_file(path, key, offset, length)
    }
    fn write_open_file(&mut self, path: &str, key: Option<FileLockKey>, offset: u64, bytes: &[u8]) -> Result<usize, u32> {
        crate::fs::system_volume::write_open_file(path, key, offset, bytes)
    }
    fn read_mapped_file(&mut self, path: &str, record: Option<u64>, offset: u64, length: usize) -> Result<Vec<u8>, u32> {
        match record {
            Some(record) => crate::fs::system_volume::read_mapped_file(path, record, offset, length),
            None => crate::fs::system_volume::read_at(path, offset, length),
        }
    }
    fn write_mapped_file(&mut self, path: &str, record: Option<u64>, offset: u64, bytes: &[u8]) -> Result<usize, u32> {
        let record = record.ok_or(0xc000_0002u32)?;
        crate::fs::system_volume::write_mapped_file(path, record, offset, bytes)
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
        let attempt = GuestAttempt::start(&candidate);
        let mut image =
            match crate::userspace::loader::load_translated_image(&candidate, line, directory,
                |machine, magic| report_guest_pe(attempt, &candidate, machine, magic)) {
                Ok(image) => image,
                Err(error) => {
                    let status = error.ntstatus();
                    let diagnostic = format!(
                        "Cannot execute {candidate}: {error}; NTSTATUS 0x{status:08X}."
                    );
                    attempt.log(&diagnostic);
                    output(diagnostic.as_bytes());
                    output(b"\r\n");
                    return Some(status);
                }
            };
        if image.assign_process_identity().is_err() {
            attempt.log(format_args!(
                "Cannot execute {candidate}: process identity allocation failed; NTSTATUS 0xC000009A."
            ));
            return Some(0xc000_009a);
        }
        report_guest_context(attempt, &candidate, &image);
        attempt.log(format_args!("Running {candidate}"));
        let identity = crate::ps::process::get_current_thread_token().snapshot_for_worker().ok();
        let mut services = IoServices::new(VolumeIo::new(attempt, &candidate, directory, output, identity));
        // Bounded maintenance/desktop console execution. Every quantum lets
        // runnable kernel/desktop threads proceed; no global BTL lock is held.
        // Large SFX installers need more than 8M instructions just to inspect
        // their embedded archive. Keep a finite 128M-instruction cap and
        // retain a scheduler yield after every 8192-instruction quantum.
        let execution_started = crate::ke::time::get_interrupt_time();
        let mut last_progress = execution_started;
        let mut run_elapsed = 0u64;
        let mut publisher = crate::mm::jit_code::KernelPublisher;
        let mut jit = crate::arch::common::btl::runtime::jit::Jit::new(&mut publisher);
        for quantum in 0..16384 {
            report_progress_periodically(&mut last_progress, attempt, &candidate, &image);
            let run_started = crate::ke::time::get_interrupt_time();
            let result = image.run_with_jit(8192, &mut services, &mut jit);
            run_elapsed = run_elapsed.saturating_add(
                crate::ke::time::get_interrupt_time().saturating_sub(run_started));
            if !matches!(result, ImageRunResult::Yielded) {
                attempt.log(format_args!(
                    "Translation {candidate}: native={} interpreted={}",
                    jit.retired(), image.retired().saturating_sub(jit.retired()),
                ));
                attempt.log(format_args!(
                    "Timing {candidate}: run_elapsed_ms={} wall_elapsed_ms={} batches={}",
                    run_elapsed / 10_000,
                    crate::ke::time::get_interrupt_time().saturating_sub(execution_started) / 10_000,
                    quantum + 1,
                ));
            }
            match result {
                ImageRunResult::Exited(code) => {
                    attempt.log(format_args!("Guest exited {candidate}: {code}"));
                    if code != 0 {
                        for (id, error, _) in &image.win32.failures {
                            let name = crate::arch::common::btl::pe::win32::catalog::resolve(*id)
                                .map_or("unknown", |api| api.name);
                            attempt.log(format_args!(
                                "Recent API failure {name} ({id}): {error}",
                            ));
                        }
                    }
                    return Some(code);
                }
                ImageRunResult::Yielded => {
                    crate::ke::scheduler::yield_();
                }
                ImageRunResult::Fault(reason) => {
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
                        StopReason::MappedWritebackFailed { .. } => 0xc0000185,
                        _ => 0xc000001d,
                    };
                    let api = image
                        .win32
                        .last_api
                        .and_then(crate::arch::common::btl::pe::win32::catalog::resolve)
                        .map(|api| format!("{}!{}", api.dll, api.name))
                        .unwrap_or_default();
                    let diagnostic = format!(
                        "Guest execution fault in {candidate}: {reason:?}; API {api}; NTSTATUS 0x{status:08X}."
                    );
                    attempt.log(&diagnostic);
                    output(diagnostic.as_bytes());
                    output(b"\r\n");
                    if let Some((dll, symbol)) = crate::arch::common::btl::pe::win32::delay_load_fault(&mut image) {
                        attempt.log(format_args!(
                            "Delay-load fault in {candidate}: {dll}!{symbol}"
                        ));
                    }
                    report_guest_progress(attempt, &candidate, &image);
                    return Some(status);
                }
            }
        }
        let diagnostic = format!("Guest instruction budget exceeded in {candidate}; console invocation stopped.");
        report_guest_progress(attempt, &candidate, &image);
        attempt.log(&diagnostic);
        output(b"[PE] ");
        output(diagnostic.as_bytes());
        output(b"\r\n");
        return Some(1460);
    }
    None
}

fn report_guest_pe(attempt: GuestAttempt, path: &str, machine: u16, magic: u16) {
    let abi = match magic { 0x10b => "pe32", 0x20b => "pe32plus", _ => "unknown" };
    attempt.log(format_args!(
        "Image {path}: pe_machine={machine:04X} guest_abi={abi} host_arch={} backend=btl",
        host_arch(),
    ));
}

fn report_guest_context(attempt: GuestAttempt, path: &str, image: &GuestImage) {
    let (machine, abi) = match image.thread.context.mode {
        DecodeMode::X86 => (0x014c, "pe32"),
        DecodeMode::X64 => (0x8664, "pe32plus"),
    };
    attempt.log(format_args!(
        "Context {path}: pe_machine={machine:04X} guest_abi={abi} host_arch={} backend=btl pid={} tid={}",
        host_arch(), image.process_id(), image.thread_id(),
    ));
}

fn report_guest_progress(attempt: GuestAttempt, path: &str, image: &GuestImage) {
    let (runnable, waiting, suspended) = image.thread_counts();
    let location = image.mapped_instruction_location()
        .map(|(module, rva)| format!(" module={module} rva=0x{rva:X}"))
        .unwrap_or_default();
    attempt.log(format_args!(
        "Progress {path}: retired={} pc=0x{:X} runnable={runnable} waiting={waiting} suspended={suspended} tid={}{location}",
        image.retired(), image.thread.context.rip, image.thread_id(),
    ));
}

fn host_arch() -> &'static str {
    if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else if cfg!(target_arch = "loongarch64") {
        "loongarch64"
    } else if cfg!(target_arch = "riscv64") {
        "riscv64"
    } else {
        "unknown"
    }
}
