//! Service control Manager (SCM)
//
//! Implements Windows Service control Manager
//! Manages Windows services defined in the registry

// SCM uses the WDK driver naming convention
// (SERVICE_* types, SERVICE_STATUS, ...).

use crate::ke::sync::Spinlock;
use core::sync::atomic::{AtomicUsize, Ordering};

extern crate alloc;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// Backslash character constant for path construction.
const BS: &str = "\\";

/// Service types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum ServiceType {
    KernelDriver = 0x01,
    FileSystemDriver = 0x02,
    Adapter = 0x04,
    RecognizerDriver = 0x08,
    DriverOwnProcess = 0x10,
    ShareProcess = 0x20,
    InteractiveProcess = 0x100,
    InteractiveOwnProcess = 0x110,
    InteractiveShareProcess = 0x120,
}

/// Service start types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum ServiceStartType {
    BootStart = 0,
    SystemStart = 1,
    AutoStart = 2,
    DemandStart = 3,
    Disabled = 4,
}

/// Service error control
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum ServiceErrorControl {
    Ignore = 0,
    Normal = 1,
    Severe = 2,
    Critical = 3,
}

/// Service state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum ServiceState {
    Stopped = 0x01,
    StartPending = 0x02,
    StopPending = 0x03,
    Running = 0x04,
    ContinuePending = 0x05,
    PausePending = 0x06,
    Paused = 0x07,
}

/// Service control codes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum ServiceControl {
    Start = 0x00,
    Stop = 0x01,
    Pause = 0x02,
    Continue = 0x03,
    Interrogate = 0x04,
    Shutdown = 0x05,
    ParamChange = 0x06,
    NetBindAdd = 0x07,
    NetBindRemove = 0x08,
    NetBindEnable = 0x09,
    NetBindDisable = 0x0A,
}

/// Service status
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ServiceStatus {
    pub service_type: ServiceType,
    pub current_state: ServiceState,
    pub controls_accepted: u32,
    pub win32_exit_code: u32,
    pub service_exit_code: u32,
    pub checkpoint: u32,
    pub wait_hint: u32,
}

impl ServiceStatus {
    pub const fn new() -> Self {
        Self {
            service_type: ServiceType::ShareProcess,
            current_state: ServiceState::Stopped,
            controls_accepted: 0,
            win32_exit_code: 0,
            service_exit_code: 0,
            checkpoint: 0,
            wait_hint: 0,
        }
    }
}

impl Default for ServiceStatus {
    fn default() -> Self {
        Self::new()
    }
}

/// Service database entry
#[derive(Clone)]
pub struct ServiceEntry {
    pub name: [u16; 256],
    pub display_name: [u16; 256],
    pub service_type: ServiceType,
    pub start_type: ServiceStartType,
    pub error_control: ServiceErrorControl,
    pub image_path: [u16; 260],
    pub load_order_group: [u16; 64],
    pub tag_id: u32,
    pub dependencies: [[u16; 64]; 8],
    pub dependency_groups: [[u16; 64]; 4],
    pub dependency_group_count: usize,
    pub service_dll: [u16; 260],
    pub delayed_auto_start: bool,
    pub dependency_count: usize,
    pub service_start_name: [u16; 256],
    pub status: ServiceStatus,
    /// Hosting process id (0 while the service has not been started).
    pub process_id: u32,
}

impl ServiceEntry {
    pub const fn new() -> Self {
        Self {
            name: [0; 256],
            display_name: [0; 256],
            service_type: ServiceType::ShareProcess,
            start_type: ServiceStartType::DemandStart,
            error_control: ServiceErrorControl::Normal,
            image_path: [0; 260],
            load_order_group: [0; 64],
            tag_id: 0,
            dependencies: [[0; 64]; 8],
            dependency_groups: [[0; 64]; 4],
            dependency_group_count: 0,
            service_dll: [0; 260],
            delayed_auto_start: false,
            dependency_count: 0,
            service_start_name: [0; 256],
            status: ServiceStatus::new(),
            process_id: 0,
        }
    }
}

impl Default for ServiceEntry {
    fn default() -> Self {
        Self::new()
    }
}

/// SCM state
pub struct ScmState {
    pub running: bool,
    pub debug: bool,
    pub services: [ServiceEntry; 256],
    pub service_count: usize,
}

impl ScmState {
    pub const fn new() -> Self {
        Self {
            running: false,
            debug: false,
            services: [const { ServiceEntry::new() }; 256],
            service_count: 0,
        }
    }
}

impl Default for ScmState {
    fn default() -> Self {
        Self::new()
    }
}

/// Global SCM state
pub static SCM_STATE: Spinlock<ScmState> = Spinlock::new(ScmState::new());

/// Initialize SCM
pub fn init() {
    // Force an early lock to flush any junk in the BSS
    // area used by the static `SCM_STATE`. On some
    // firmware the BSS arrays aren't reliably zeroed.
    {
        let state = SCM_STATE.lock();
        let _ = state.running;
    }
    let mut state = SCM_STATE.lock();
    state.running = true;
    state.debug = false;
    drop(state);

    // Register the audited Windows 7 SP1 installation-image configuration.
    register_default_services();
    install_backends();

    // kprintln!("    Service control Manager initialized");
}

/// Start SCM
pub fn start() {
    // kprintln!("[SCM] Starting Service control Manager...");

    // Phase 1: Initialize
    phase1_init();

    // Phase 2: Load services
    phase2_load_services();

    // Phase 3: Start auto-start services
    phase3_start_services();

    // kprintln!("[SCM] Service control Manager started");
}

/// Phase 1: Initialize SCM
fn phase1_init() {
    // kprintln!("[SCM] Phase 1: Initializing...");
}

/// Phase 2: Load service definitions from registry
fn phase2_load_services() {
    // kprintln!("[SCM] Phase 2: Loading service definitions...");

    // Load services from:
    // HKLM\SYSTEM\CurrentControlSet\Services

    // Query service definitions from the registry
    load_services_from_registry();

    // Also add default services as fallback
    add_default_services();

    let _state = SCM_STATE.lock();
    // kprintln!("[SCM] Loaded service definitions");
}

/// Load service definitions from the registry
fn load_services_from_registry() {
    use crate::registry::cm::{enumerate_subkeys, query_dword, query_string, query_value};

    // Query the list of services from the registry
    let service_keys =
        match enumerate_subkeys("\\Registry\\Machine\\SYSTEM\\CurrentControlSet\\Services") {
            Some(keys) => keys,
            None => {
                // kprintln!("[SCM] No services found in registry (CM not available)");
                return;
            }
        };

    // kprintln!("[SCM] Found {} service definitions in registry", service_keys.len());

    // For each service, read its configuration
    for service_name in &service_keys {
        let service_path = format!(
            "\\Registry\\Machine\\SYSTEM\\CurrentControlSet\\Services\\{}",
            service_name
        );

        // Services also contains configuration-only subkeys. Missing required
        // values must not manufacture a demand-start Win32 service.
        let (Some(reg_type), Some(reg_start)) = (
            query_dword(&service_path, "Type"),
            query_dword(&service_path, "Start"),
        ) else {
            continue;
        };

        // read ErrorControl
        let reg_error = query_dword(&service_path, "ErrorControl").unwrap_or(1); // Normal (1)

        // read ImagePath and DisplayName (optional)
        let image_path = query_string(&service_path, "ImagePath");
        let display_name =
            query_string(&service_path, "DisplayName").unwrap_or_else(|| service_name.clone());

        // Map the registry values onto the SCM entry types. Unknown
        // bits fall back to the closest Win32 meaning instead of being
        // silently dropped.
        let start_type = match reg_start {
            0 => ServiceStartType::BootStart,
            1 => ServiceStartType::SystemStart,
            2 => ServiceStartType::AutoStart,
            4 => ServiceStartType::Disabled,
            _ => ServiceStartType::DemandStart,
        };
        let service_type = if reg_type & 0x01 != 0 {
            ServiceType::KernelDriver
        } else if reg_type & 0x02 != 0 {
            ServiceType::FileSystemDriver
        } else if reg_type == 0x110 {
            ServiceType::InteractiveOwnProcess
        } else if reg_type == 0x120 {
            ServiceType::InteractiveShareProcess
        } else if reg_type & 0x10 != 0 {
            ServiceType::DriverOwnProcess
        } else {
            ServiceType::ShareProcess
        };
        let error_control = match reg_error {
            0 => ServiceErrorControl::Ignore,
            2 => ServiceErrorControl::Severe,
            3 => ServiceErrorControl::Critical,
            _ => ServiceErrorControl::Normal,
        };

        let group = query_string(&service_path, "Group").unwrap_or_default();
        let account = query_string(&service_path, "ObjectName").unwrap_or_default();
        let read_multi = |name| {
            query_value(&service_path, name).map(|v| {
                let units: Vec<u16> = v.data.chunks_exact(2)
                    .map(|b| u16::from_le_bytes([b[0], b[1]])).collect();
                units.split(|c| *c == 0).filter(|v| !v.is_empty())
                    .map(String::from_utf16_lossy).collect::<Vec<_>>()
            }).unwrap_or_default()
        };
        let dependencies = read_multi("DependOnService");
        let dependency_groups = read_multi("DependOnGroup");
        let service_dll = query_string(&format!("{service_path}\\Parameters"), "ServiceDll").unwrap_or_default();
        let delayed = query_dword(&service_path, "DelayedAutoStart").unwrap_or(0) != 0;
        let mut state = SCM_STATE.lock();
        let existing = find_service(&state, service_name);
        let idx = existing.unwrap_or(state.service_count);
        if idx >= state.services.len() {
            break;
        }
        if state.services[idx].status.current_state != ServiceState::Stopped {
            continue;
        }
        let svc = &mut state.services[idx];
        copy_str_utf16(&mut svc.name, service_name);
        let resolved = if display_name.starts_with('@') {
            WIN7_SERVICES.iter().find(|b|b.name.eq_ignore_ascii_case(service_name) && b.display_name.eq_ignore_ascii_case(&display_name))
                .map(|b|b.resolved_name).unwrap_or(&display_name)
        } else { &display_name };
        copy_str_utf16(&mut svc.display_name, resolved);
        copy_str_utf16(&mut svc.image_path, image_path.as_deref().unwrap_or_default());
        svc.service_type = service_type;
        svc.start_type = start_type;
        svc.error_control = error_control;
        svc.status.service_type = service_type;
        copy_str_utf16(&mut svc.load_order_group, &group);
        copy_str_utf16(&mut svc.service_start_name, &account);
        svc.dependencies.fill([0; 64]);
        svc.dependency_groups.fill([0; 64]);
        svc.dependency_count = dependencies.len().min(svc.dependencies.len());
        svc.dependency_group_count = dependency_groups.len().min(svc.dependency_groups.len());
        for (target, dependency) in svc.dependencies.iter_mut().zip(&dependencies) {
            copy_str_utf16(target, dependency);
        }
        for (target, group) in svc.dependency_groups.iter_mut().zip(&dependency_groups) {
            copy_str_utf16(target, group);
        }
        copy_str_utf16(&mut svc.service_dll, &service_dll);
        svc.delayed_auto_start = delayed;
        // Never silently bypass a required dependency that cannot be represented.
        if dependencies.len() > svc.dependencies.len() || dependency_groups.len() > svc.dependency_groups.len()
            || dependencies.iter().chain(&dependency_groups).any(|d| d.encode_utf16().count() >= 64) {
            svc.start_type = ServiceStartType::Disabled;
        }
        if existing.is_none() {
            state.service_count = idx + 1;
        }
    }
}

/// Phase 3: Start auto-start services
fn phase3_start_services() {
    // Collect the names of auto-start services under the lock,
    // then start them one at a time through the public
    // `start_service` entry point (which re-acquires the lock
    // internally) to avoid a re-entrant deadlock.
    let names: alloc::vec::Vec<alloc::string::String> = {
        let state = SCM_STATE.lock();
        (0..state.service_count)
            .filter_map(|i| {
                let svc = &state.services[i];
                if svc.start_type == ServiceStartType::AutoStart
                    || svc.start_type == ServiceStartType::SystemStart
                    || svc.start_type == ServiceStartType::BootStart
                {
                    Some(utf16_to_string(&svc.name))
                } else {
                    None
                }
            })
            .collect()
    };
    for name in names {
        start_service(&name);
    }
}

/// Add default services
fn add_default_services() {
    register_default_services();
}

struct BaselineService {
    name: &'static str, display_name: &'static str, resolved_name: &'static str, image_path: &'static str,
    account: &'static str, group: &'static str, service_dll: &'static str,
    service_type: u32, start: u32, error: u32, delayed: bool,
    dependencies: &'static [&'static str], dependency_groups: &'static [&'static str],
}
include!("win7_baseline.rs");

fn register_default_services() {
    let mut state = SCM_STATE.lock();
    for baseline in WIN7_SERVICES {
        if find_service(&state, baseline.name).is_some() { continue; }
        let idx = state.service_count;
        if idx >= state.services.len() { break; }
        let svc = &mut state.services[idx];
        copy_str_utf16(&mut svc.name, baseline.name);
        copy_str_utf16(&mut svc.display_name, if baseline.display_name.starts_with('@') { baseline.resolved_name } else { baseline.display_name });
        copy_str_utf16(&mut svc.image_path, baseline.image_path);
        copy_str_utf16(&mut svc.service_start_name, baseline.account);
        copy_str_utf16(&mut svc.load_order_group, baseline.group);
        copy_str_utf16(&mut svc.service_dll, baseline.service_dll);
        svc.service_type = match baseline.service_type {
            1 => ServiceType::KernelDriver, 2 => ServiceType::FileSystemDriver,
            0x110 => ServiceType::InteractiveOwnProcess,
            0x120 => ServiceType::InteractiveShareProcess,
            16 => ServiceType::DriverOwnProcess, _ => ServiceType::ShareProcess,
        };
        svc.start_type = match baseline.start {
            0 => ServiceStartType::BootStart, 1 => ServiceStartType::SystemStart,
            2 => ServiceStartType::AutoStart, 4 => ServiceStartType::Disabled,
            _ => ServiceStartType::DemandStart,
        };
        svc.error_control = match baseline.error {
            0 => ServiceErrorControl::Ignore, 2 => ServiceErrorControl::Severe,
            3 => ServiceErrorControl::Critical, _ => ServiceErrorControl::Normal,
        };
        svc.delayed_auto_start = baseline.delayed;
        svc.dependency_count = baseline.dependencies.len();
        assert!(svc.dependency_count <= svc.dependencies.len());
        for (target, dep) in svc.dependencies.iter_mut().zip(baseline.dependencies) {
            copy_str_utf16(target, dep);
        }
        svc.dependency_group_count = baseline.dependency_groups.len();
        assert!(svc.dependency_group_count <= svc.dependency_groups.len());
        for (target, group) in svc.dependency_groups.iter_mut().zip(baseline.dependency_groups) {
            copy_str_utf16(target, group);
        }
        svc.status.service_type = svc.service_type;
        state.service_count += 1;
    }
}

/// Backends report readiness only after their actual subsystem is available.
/// A backend owns cleanup; SCM must never free a live process's PID.
#[derive(Clone, Copy)]
pub struct BackendStarted {
    pub process_id: u32,
    pub controls_accepted: u32,
}
#[derive(Clone, Copy)]
pub struct ServiceBackend {
    pub name: &'static str,
    pub start: fn() -> Result<BackendStarted, u32>,
    pub control: fn(ServiceControl) -> Result<(), u32>,
}
static BACKENDS: Spinlock<[Option<ServiceBackend>; 32]> = Spinlock::new([None; 32]);
pub fn register_backend(backend: ServiceBackend) -> bool {
    let mut entries = BACKENDS.lock();
    if entries
        .iter()
        .flatten()
        .any(|b| b.name.eq_ignore_ascii_case(backend.name))
    {
        return false;
    }
    let Some(slot) = entries.iter_mut().find(|b| b.is_none()) else {
        return false;
    };
    *slot = Some(backend);
    true
}
fn backend(name: &str) -> Option<ServiceBackend> {
    BACKENDS
        .lock()
        .iter()
        .flatten()
        .find(|b| b.name.eq_ignore_ascii_case(name))
        .copied()
}
fn dns_start() -> Result<BackendStarted, u32> {
    crate::netstack::dns::set_cache_service_enabled(true);
    Ok(BackendStarted {
        process_id: 0,
        controls_accepted: 1,
    })
}
fn dns_control(control: ServiceControl) -> Result<(), u32> {
    match control {
        ServiceControl::Stop | ServiceControl::Shutdown => {
            crate::netstack::dns::set_cache_service_enabled(false);
            Ok(())
        }
        ServiceControl::Interrogate => Ok(()),
        _ => Err(1052),
    }
}
fn eventlog_start() -> Result<BackendStarted, u32> {
    crate::rtl::eventlog::ensure_initialized();
    Ok(BackendStarted {
        process_id: 0,
        controls_accepted: 0,
    })
}
fn eventlog_control(control: ServiceControl) -> Result<(), u32> {
    if control == ServiceControl::Interrogate {
        Ok(())
    } else {
        Err(1052)
    }
}
fn install_backends() {
    register_backend(ServiceBackend {
        name: "Dnscache",
        start: dns_start,
        control: dns_control,
    });
    register_backend(ServiceBackend {
        name: "EventLog",
        start: eventlog_start,
        control: eventlog_control,
    });
}

/// Copy UTF-16 including the terminator, clearing any previous longer value.
fn copy_str_utf16(dest: &mut [u16], src: &str) {
    dest.fill(0);
    let max = dest.len().saturating_sub(1);
    for (cell, unit) in dest.iter_mut().take(max).zip(src.encode_utf16()) {
        *cell = unit;
    }
}
fn fail_start(idx: usize, error: u32) -> Result<(), u32> {
    let mut state = SCM_STATE.lock();
    let svc = &mut state.services[idx];
    svc.status.current_state = ServiceState::Stopped;
    svc.status.win32_exit_code = error;
    svc.status.service_exit_code = 0;
    svc.status.controls_accepted = 0;
    svc.status.checkpoint = 0;
    svc.status.wait_hint = 0;
    svc.process_id = 0;
    Err(error)
}
fn start_index(idx: usize, visiting: &mut Vec<usize>, dependency: bool) -> Result<(), u32> {
    if visiting.contains(&idx) {
        return Err(1059);
    } // circular dependency
    let svc = {
        let mut state = SCM_STATE.lock();
        if idx >= state.service_count {
            return Err(1060);
        }
        let svc = &mut state.services[idx];
        if svc.start_type == ServiceStartType::Disabled {
            return Err(1058);
        }
        match svc.status.current_state {
            ServiceState::Running if dependency => return Ok(()),
            ServiceState::Running | ServiceState::Paused => return Err(1056),
            ServiceState::Stopped => {}
            _ => return Err(1061),
        }
        svc.status.current_state = ServiceState::StartPending;
        svc.status.controls_accepted = 0;
        svc.status.win32_exit_code = 0;
        svc.status.service_exit_code = 0;
        svc.status.checkpoint = 1;
        svc.status.wait_hint = 30000;
        svc.clone()
    };
    visiting.push(idx);
    let deps = (|| {
        for dep in svc.dependencies.iter().take(svc.dependency_count) {
            let name = utf16_to_string(dep);
            if name.is_empty() {
                continue;
            }
            let dep_idx = {
                let state = SCM_STATE.lock();
                find_service(&state, &name)
            }
            .ok_or(1075u32)?;
            start_index(dep_idx, visiting, true).map_err(|error| -> u32 {
                if error == 1059 {
                    1059
                } else {
                    1068
                }
            })?;
        }
        // A load-order group dependency succeeds when at least one member
        // starts. Keep the current node in `visiting` through group traversal.
        for group in svc.dependency_groups.iter().take(svc.dependency_group_count) {
            let group = utf16_to_string(group);
            let members: Vec<usize> = {
                let state = SCM_STATE.lock();
                state.services[..state.service_count].iter().enumerate()
                    .filter(|(_, entry)| utf16_to_string(&entry.load_order_group).eq_ignore_ascii_case(&group))
                    .map(|(index, _)| index).collect()
            };
            let mut ready = false;
            for member in members {
                if start_index(member, visiting, true).is_ok() { ready = true; }
            }
            if !ready { return Err(1068); }
        }
        Ok::<(), u32>(())
    })();
    visiting.pop();
    if let Err(error) = deps {
        return fail_start(idx, error);
    }
    let name = utf16_to_string(&svc.name);
    // No SCM guard is held during subsystem, image or handler operations.
    let result = if let Some(b) = backend(&name) {
        (b.start)()
    } else if matches!(
        svc.service_type,
        ServiceType::KernelDriver | ServiceType::FileSystemDriver
    ) {
        let mut bytes = [0u8; 64];
        let len = name.len().min(63);
        bytes[..len].copy_from_slice(&name.as_bytes()[..len]);
        if crate::io::find_registered_driver(&bytes).is_null() {
            Err(2)
        } else {
            Ok(BackendStarted {
                process_id: 0,
                controls_accepted: 0,
            })
        }
    } else {
        Err(120)
    }; // No executable service dispatcher is installed for this service.
    let ready = match result {
        Ok(ready) => ready,
        Err(error) => return fail_start(idx, error),
    };
    let mut state = SCM_STATE.lock();
    let svc = &mut state.services[idx];
    svc.process_id = ready.process_id;
    svc.status.service_type = svc.service_type;
    svc.status.current_state = ServiceState::Running;
    svc.status.controls_accepted = ready.controls_accepted;
    svc.status.checkpoint = 0;
    svc.status.wait_hint = 0;
    Ok(())
}
/// Win32 error result; callers can distinguish disabled, missing, dependency and backend errors.
pub fn start_service_result(name: &str) -> Result<(), u32> {
    let idx = {
        let state = SCM_STATE.lock();
        find_service(&state, name)
    }
    .ok_or(1060u32)?;
    start_index(idx, &mut Vec::new(), false)
}
pub fn start_service(name: &str) -> bool {
    start_service_result(name).is_ok()
}

/// Stop a specific service by name
pub fn stop_service(service_name: &str) -> bool {
    let idx = {
        let state = SCM_STATE.lock();
        match find_service(&state, service_name) {
            Some(idx) => idx,
            None => return false,
        }
    };
    control_service(idx, ServiceControl::Stop)
}

/// Query service status
pub fn query_service_status(service_name: &str) -> Option<ServiceStatus> {
    let state = SCM_STATE.lock();

    for i in 0..state.service_count {
        let name_str = String::from_utf16_lossy(&state.services[i].name);
        let name = name_str.trim_end_matches('\0');
        if name.eq_ignore_ascii_case(service_name) {
            return Some(state.services[i].status);
        }
    }
    None
}

/// List all registered services
pub fn list_services() -> Vec<(String, ServiceState)> {
    let state = SCM_STATE.lock();
    let mut result = Vec::new();

    for i in 0..state.service_count {
        let name = String::from_utf16_lossy(&state.services[i].name)
            .trim_end_matches('\0')
            .into();
        result.push((name, state.services[i].status.current_state));
    }
    result
}

// ============================================================================
// Service control API
// ============================================================================

/// Open a service handle (returns service entry index or -1)
pub fn open_service(service_name: &str) -> Option<usize> {
    let state = SCM_STATE.lock();

    for i in 0..state.service_count {
        let name_str = String::from_utf16_lossy(&state.services[i].name);
        let name = name_str.trim_end_matches('\0');
        if name.eq_ignore_ascii_case(service_name) {
            return Some(i);
        }
    }
    None
}

/// control a service (start, stop, pause, continue)
pub fn control_service(idx: usize, control: ServiceControl) -> bool {
    if control == ServiceControl::Start {
        return start_index(idx, &mut Vec::new(), false).is_ok();
    }
    control_index(idx, control).is_ok()
}
pub fn stop_service_result(name: &str) -> Result<(), u32> {
    let idx = {
        let state = SCM_STATE.lock();
        find_service(&state, name)
    }
    .ok_or(1060u32)?;
    control_index(idx, ServiceControl::Stop)
}
fn control_index(idx: usize, control: ServiceControl) -> Result<(), u32> {
    let (name, old, pending, target) = {
        let mut state = SCM_STATE.lock();
        if idx >= state.service_count {
            return Err(1060);
        }
        let svc = &state.services[idx];
        let name = utf16_to_string(&svc.name);
        let old = svc.status;
        if control == ServiceControl::Interrogate {
            return Ok(());
        }
        let (accepted, pending, target, required) = match control {
            ServiceControl::Stop => (1, ServiceState::StopPending, ServiceState::Stopped, None),
            ServiceControl::Shutdown => (4, ServiceState::StopPending, ServiceState::Stopped, None),
            ServiceControl::Pause => (
                2,
                ServiceState::PausePending,
                ServiceState::Paused,
                Some(ServiceState::Running),
            ),
            ServiceControl::Continue => (
                2,
                ServiceState::ContinuePending,
                ServiceState::Running,
                Some(ServiceState::Paused),
            ),
            _ => return Err(1052),
        };
        if old.current_state == ServiceState::Stopped {
            return Err(1062);
        }
        if !matches!(
            old.current_state,
            ServiceState::Running | ServiceState::Paused
        ) {
            return Err(1061);
        }
        if required.is_some_and(|r| r != old.current_state) {
            return Err(1061);
        }
        if old.controls_accepted & accepted == 0 {
            return Err(1052);
        }
        if target == ServiceState::Stopped
            && state.services[..state.service_count].iter().any(|d| {
                d.status.current_state != ServiceState::Stopped
                    && (d.dependencies
                        .iter()
                        .take(d.dependency_count)
                        .any(|dep| utf16_to_string(dep).eq_ignore_ascii_case(&name))
                        || d.dependency_groups.iter().take(d.dependency_group_count).any(|group| {
                            let group = utf16_to_string(group);
                            !group.is_empty()
                                && group.eq_ignore_ascii_case(&utf16_to_string(&svc.load_order_group))
                                && !state.services[..state.service_count].iter().enumerate().any(|(i, member)| {
                                    i != idx
                                        && member.status.current_state == ServiceState::Running
                                        && group.eq_ignore_ascii_case(&utf16_to_string(&member.load_order_group))
                                })
                        }))
            })
        {
            return Err(1051);
        }
        let svc = &mut state.services[idx];
        svc.status.current_state = pending;
        svc.status.controls_accepted = 0;
        svc.status.checkpoint = 1;
        svc.status.wait_hint = 30000;
        (name, old, pending, target)
    };
    let result = if let Some(b) = backend(&name) {
        (b.control)(control)
    } else {
        let utf16: Vec<u16> = name.encode_utf16().collect();
        let handler = {
            HANDLER_TABLE
                .lock()
                .iter()
                .flatten()
                .find(|e| {
                    utf16_to_string(&e.name[..e.name_len])
                        .eq_ignore_ascii_case(&utf16_to_string(&utf16))
                })
                .map(|e| e.handler)
        };
        match handler {
            Some(h) => match h(control as u32) {
                0 => Ok(()),
                e => Err(e),
            },
            None => Err(1052),
        }
    };
    let mut state = SCM_STATE.lock();
    let svc = &mut state.services[idx];
    if let Err(error) = result {
        svc.status = old;
        return Err(error);
    }
    if svc.status.current_state == pending {
        svc.status = old;
        svc.status.current_state = target;
        svc.status.checkpoint = 0;
        svc.status.wait_hint = 0;
        if target == ServiceState::Stopped {
            svc.status.controls_accepted = 0;
            svc.process_id = 0;
        }
    }
    Ok(())
}

/// Get service configuration
pub fn query_service_config(
    idx: usize,
) -> Option<(ServiceType, ServiceStartType, ServiceErrorControl)> {
    let state = SCM_STATE.lock();

    if idx >= state.service_count {
        return None;
    }

    let svc = &state.services[idx];
    Some((svc.service_type, svc.start_type, svc.error_control))
}

// ============================================================================
// Service Enumeration
// ============================================================================

/// Enumerate services by type
pub fn enumerate_services(svc_type: ServiceType) -> Vec<String> {
    let state = SCM_STATE.lock();
    let mut result = Vec::new();

    for i in 0..state.service_count {
        if state.services[i].service_type == svc_type {
            let name = String::from_utf16_lossy(&state.services[i].name)
                .trim_end_matches('\0')
                .into();
            result.push(name);
        }
    }
    result
}

/// Enumerate services by state
pub fn enumerate_services_by_state(state_filter: ServiceState) -> Vec<String> {
    let state = SCM_STATE.lock();
    let mut result = Vec::new();

    for i in 0..state.service_count {
        if state.services[i].status.current_state == state_filter {
            let name = String::from_utf16_lossy(&state.services[i].name)
                .trim_end_matches('\0')
                .into();
            result.push(name);
        }
    }
    result
}

// ============================================================================
// Service Database Lock
// ============================================================================

/// True while the service database is reserved by an external caller.
static SERVICE_DATABASE_LOCK: Spinlock<bool> = Spinlock::new(false);

/// Lock the service database for update
pub fn lock_service_database() -> bool {
    let mut reserved = SERVICE_DATABASE_LOCK.lock();
    if *reserved {
        return false;
    }
    let state = SCM_STATE.lock();
    if !state.running {
        return false;
    }
    *reserved = true;
    true
}

/// Unlock the service database
pub fn unlock_service_database() {
    let mut reserved = SERVICE_DATABASE_LOCK.lock();
    *reserved = false;
}

// ============================================================================
// Service control Handler (for services running in svchost)
// ============================================================================

/// Service control handler callback type
pub type ServiceControlHandler = fn(control: u32) -> u32;

/// Service control handler state
static SERVICE_CONTROL_HANDLER: Spinlock<Option<ServiceControlHandler>> = Spinlock::new(None);

/// Register a service control handler
pub fn register_control_handler(handler: ServiceControlHandler) {
    let mut h = SERVICE_CONTROL_HANDLER.lock();
    *h = Some(handler);
    // kprintln!("[SCM] Service control handler registered");
}

/// Dispatch control code to the registered handler
pub fn dispatch_control(control: u32) -> u32 {
    let handler = *SERVICE_CONTROL_HANDLER.lock();
    if let Some(handler) = handler {
        handler(control)
    } else {
        127 // ERROR_PROC_NOT_FOUND
    }
}

// (spinlock now centralised in ke::sync)

// ============================================================================
// Service main-thread API (used by sshd in SCM mode)
// ============================================================================

/// Maximum length of a service name in UTF-16 cells.
pub const SERVICE_NAME_MAX: usize = 256;

/// Maximum length of a service display name in UTF-16 cells.
pub const SERVICE_DISPLAY_NAME_MAX: usize = 256;

/// Maximum length of an image path in UTF-16 cells.
pub const SERVICE_IMAGE_PATH_MAX: usize = 260;

/// Per-service registered control handler entry. The handler is
/// invoked by `dispatch_service_control(name, code)` whenever the
/// SCM sends a control code to a running service.
#[derive(Clone, Copy)]
pub struct ServiceControlHandlerEntry {
    pub name: [u16; SERVICE_NAME_MAX],
    pub name_len: usize,
    pub handler: ServiceControlHandler,
    pub current_status: ServiceStatus,
    /// Last reported checkpoint (for hung-service detection).
    pub last_checkpoint_time_ms: u64,
}

impl ServiceControlHandlerEntry {
    pub const fn empty() -> Self {
        Self {
            name: [0u16; SERVICE_NAME_MAX],
            name_len: 0,
            handler: empty_handler,
            current_status: ServiceStatus::new(),
            last_checkpoint_time_ms: 0,
        }
    }
}

fn empty_handler(_control: u32) -> u32 {
    0
}

const MAX_HANDLER_ENTRIES: usize = 32;
static HANDLER_TABLE: Spinlock<[Option<ServiceControlHandlerEntry>; MAX_HANDLER_ENTRIES]> =
    Spinlock::new([const { None }; MAX_HANDLER_ENTRIES]);

/// `RegisterServiceCtrlHandlerExW`-equivalent. Registers a service
/// control handler for the running service. Returns the entry's
/// index, or `None` if the table is full or the name collides.
///
/// `service_name` is a UTF-16 slice (null-terminated not required).
pub fn register_service_ctrl_handler(
    service_name: &[u16],
    handler: ServiceControlHandler,
) -> Option<usize> {
    if service_name.is_empty() || service_name.len() > SERVICE_NAME_MAX || service_name.contains(&0)
    {
        return None;
    }
    let mut table = HANDLER_TABLE.lock();
    for slot in table.iter_mut() {
        if let Some(entry) = slot.as_ref() {
            if utf16_to_string(&entry.name[..entry.name_len])
                .eq_ignore_ascii_case(&utf16_to_string(service_name))
            {
                return None;
            }
        }
    }
    for (idx, slot) in table.iter_mut().enumerate() {
        if slot.is_none() {
            let mut entry = ServiceControlHandlerEntry::empty();
            entry.name_len = service_name.len();
            for (i, c) in service_name.iter().enumerate() {
                entry.name[i] = *c;
            }
            entry.handler = handler;
            entry.current_status = ServiceStatus::new();
            entry.current_status.current_state = ServiceState::StartPending;
            *slot = Some(entry);
            return Some(idx);
        }
    }
    None
}

/// `SetServiceStatus`-equivalent. Updates the cached status for a
/// service that previously registered a control handler.
pub fn set_service_status(idx: usize, status: ServiceStatus) -> bool {
    let name = {
        let mut table = HANDLER_TABLE.lock();
        let Some(Some(slot)) = table.get_mut(idx) else {
            return false;
        };
        slot.current_status = status;
        utf16_to_string(&slot.name[..slot.name_len])
    };
    let mut state = SCM_STATE.lock();
    if let Some(service) = find_service(&state, &name) {
        state.services[service].status = status;
        if status.current_state == ServiceState::Stopped {
            state.services[service].process_id = 0;
        }
    }
    true
}

/// Get the most recent status reported by `set_service_status`.
pub fn query_service_status_ex(idx: usize) -> Option<ServiceStatus> {
    let table = HANDLER_TABLE.lock();
    table
        .get(idx)
        .and_then(|s| s.as_ref().map(|e| e.current_status))
}

/// Dispatch a control code to a registered handler by name.
/// Returns the handler's return value, or `127` (ERROR_PROC_NOT_FOUND)
/// if no such service is registered.
pub fn dispatch_service_control(service_name: &[u16], control: u32) -> u32 {
    let table = HANDLER_TABLE.lock();
    for slot in table.iter() {
        if let Some(entry) = slot.as_ref() {
            if utf16_to_string(&entry.name[..entry.name_len])
                .eq_ignore_ascii_case(&utf16_to_string(service_name))
            {
                let h = entry.handler;
                drop(table);
                return h(control);
            }
        }
    }
    127
}

/// Convert a control code (raw u32) to a `ServiceControl` value.
pub fn control_from_code(code: u32) -> Option<ServiceControl> {
    match code {
        0x00 => Some(ServiceControl::Start),
        0x01 => Some(ServiceControl::Stop),
        0x02 => Some(ServiceControl::Pause),
        0x03 => Some(ServiceControl::Continue),
        0x04 => Some(ServiceControl::Interrogate),
        0x05 => Some(ServiceControl::Shutdown),
        0x06 => Some(ServiceControl::ParamChange),
        0x07 => Some(ServiceControl::NetBindAdd),
        0x08 => Some(ServiceControl::NetBindRemove),
        0x09 => Some(ServiceControl::NetBindEnable),
        0x0A => Some(ServiceControl::NetBindDisable),
        _ => None,
    }
}

// ============================================================================
// Service information, dependency and control pipe API
// ============================================================================

/// Snapshot of one service database entry.
#[derive(Clone)]
pub struct ServiceInfo {
    pub name: String,
    pub display_name: String,
    pub service_type: ServiceType,
    pub start_type: ServiceStartType,
    pub error_control: ServiceErrorControl,
    pub image_path: String,
    pub load_order_group: String,
    pub state: ServiceState,
    pub process_id: u32,
    pub dependencies: Vec<String>,
    pub dependency_groups: Vec<String>,
    pub service_dll: String,
    pub account: String,
    pub delayed_auto_start: bool,
}

fn utf16_to_string(value: &[u16]) -> String {
    String::from_utf16_lossy(value)
        .trim_end_matches('\0')
        .into()
}

fn find_service(state: &ScmState, service_name: &str) -> Option<usize> {
    (0..state.service_count)
        .find(|&i| utf16_to_string(&state.services[i].name).eq_ignore_ascii_case(service_name))
}

/// Query the full service record (`QueryServiceEx` / `QueryServiceLockStatus`).
pub fn query_service_info(service_name: &str) -> Option<ServiceInfo> {
    let state = SCM_STATE.lock();
    let idx = find_service(&state, service_name)?;
    let svc = &state.services[idx];

    let mut dependencies = Vec::new();
    for dep in svc.dependencies.iter().take(svc.dependency_count) {
        let name = utf16_to_string(dep);
        if !name.is_empty() {
            dependencies.push(name);
        }
    }

    Some(ServiceInfo {
        name: utf16_to_string(&svc.name),
        display_name: utf16_to_string(&svc.display_name),
        service_type: svc.service_type,
        start_type: svc.start_type,
        error_control: svc.error_control,
        image_path: utf16_to_string(&svc.image_path),
        load_order_group: utf16_to_string(&svc.load_order_group),
        state: svc.status.current_state,
        process_id: svc.process_id,
        dependencies,
        dependency_groups: svc.dependency_groups.iter().take(svc.dependency_group_count).map(|g| utf16_to_string(g)).collect(),
        service_dll: utf16_to_string(&svc.service_dll),
        account: utf16_to_string(&svc.service_start_name),
        delayed_auto_start: svc.delayed_auto_start,
    })
}

/// One coherent, lightweight snapshot; no repeated name lookup or entry cloning.
pub fn service_snapshot() -> Vec<(String, String, String, ServiceStatus, u32, ServiceStartType)> {
    let state = SCM_STATE.lock();
    state.services[..state.service_count].iter().filter(|svc|
        matches!(svc.service_type, ServiceType::DriverOwnProcess | ServiceType::ShareProcess
            | ServiceType::InteractiveProcess | ServiceType::InteractiveOwnProcess | ServiceType::InteractiveShareProcess))
        .map(|svc| (utf16_to_string(&svc.name), utf16_to_string(&svc.display_name),
            // Task Manager's Group column is the svchost group, not the load-order group.
            utf16_to_string(&svc.image_path).split(" -k ").nth(1).unwrap_or("").into(),
            svc.status, svc.process_id, svc.start_type)).collect()
}

/// Query every service: `(name, state, hosting process id)`.
pub fn query_all_services() -> Vec<(String, ServiceState, u32)> {
    let state = SCM_STATE.lock();
    let mut result = Vec::new();
    for i in 0..state.service_count {
        result.push((
            utf16_to_string(&state.services[i].name),
            state.services[i].status.current_state,
            state.services[i].process_id,
        ));
    }
    result
}

/// Check readiness: every named dependency and at least one member of every
/// required load-order group must actually be running. Startup policy alone
/// never counts as readiness.
pub fn check_dependencies(service_name: &str) -> bool {
    let state = SCM_STATE.lock();
    let idx = match find_service(&state, service_name) {
        Some(idx) => idx,
        None => return false,
    };
    let svc = &state.services[idx];

    for dep in svc.dependencies.iter().take(svc.dependency_count) {
        let dep_name = utf16_to_string(dep);
        if dep_name.is_empty() {
            continue;
        }
        let dep_idx = match find_service(&state, &dep_name) {
            Some(idx) => idx,
            None => return false,
        };
        let dep_svc = &state.services[dep_idx];
        let satisfied = dep_svc.status.current_state == ServiceState::Running;
        if !satisfied {
            return false;
        }
    }
    for group in svc.dependency_groups.iter().take(svc.dependency_group_count) {
        let group = utf16_to_string(group);
        if !state.services[..state.service_count].iter().any(|member| {
            member.status.current_state == ServiceState::Running
                && group.eq_ignore_ascii_case(&utf16_to_string(&member.load_order_group))
        }) {
            return false;
        }
    }
    true
}

static CONTROL_PIPE_COUNT: AtomicUsize = AtomicUsize::new(0);

/// create the SCM control pipe for a service (`CreateServicePipe`).
/// Returns the pipe handle, or `None` when the service is unknown.
pub fn create_service_pipe(service_name: &str) -> Option<usize> {
    let state = SCM_STATE.lock();
    find_service(&state, service_name)?;
    // Handles start at 1 so that 0 keeps meaning "no pipe".
    Some(CONTROL_PIPE_COUNT.fetch_add(1, Ordering::Relaxed) + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_code_round_trip() {
        assert_eq!(control_from_code(0x01), Some(ServiceControl::Stop));
        assert_eq!(control_from_code(0x05), Some(ServiceControl::Shutdown));
        assert_eq!(control_from_code(0xFF), None);
    }

    #[test]
    fn set_service_status_updates_state() {
        let name: [u16; 4] = ['t' as u16, 'e' as u16, 's' as u16, 't' as u16];
        let idx = register_service_ctrl_handler(&name, empty_handler).unwrap();
        let mut st = ServiceStatus::new();
        st.current_state = ServiceState::Running;
        assert!(set_service_status(idx, st));
        let got = query_service_status_ex(idx).unwrap();
        assert_eq!(got.current_state, ServiceState::Running);
    }

    #[test]
    fn dispatch_unknown_service_reports_missing_handler() {
        let bogus: [u16; 3] = ['x' as u16, 'y' as u16, 'z' as u16];
        assert_eq!(dispatch_service_control(&bogus, 0x01), 127);
    }
}
