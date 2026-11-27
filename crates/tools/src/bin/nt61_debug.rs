use clap::Parser;
use nt61_tools::fs::pe_read;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const BOOT_MARKERS: [&str; 4] = [
    "[BOOTMGR] framebuffer mailbox:",
    "[WINLOAD] os_loader_run entered",
    "[KERNEL] boot mode start-session-manager",
    "[EXPLORER] desktop frame ready",
];
const BOOT_CHAIN_LEN: usize = 3;

#[derive(Parser)]
#[command(about = "Check NT61 project serial captures and guest PE imports")]
struct Args {
    /// Serial log path, or - for standard input
    #[arg(required_unless_present = "pe_imports", conflicts_with = "pe_imports")]
    serial: Option<PathBuf>,
    /// Guest executable whose imports should be checked beside it and in the runtime directory
    #[arg(long, requires = "runtime_dir")]
    pe_imports: Option<PathBuf>,
    /// Windows/System32 or Windows/SysWOW64 from the project build
    #[arg(long, requires = "pe_imports")]
    runtime_dir: Option<PathBuf>,
    /// Project image manifest from scripts/test-original-notepad.py
    #[arg(long, requires = "serial")]
    project_image: Option<PathBuf>,
    /// Emit a machine-readable report
    #[arg(long)]
    json: bool,
    /// Require the ordered project startup markers through Session Manager
    #[arg(long, requires = "serial")]
    require_boot: bool,
    /// Require the boot chain and subsequent Explorer desktop frame
    #[arg(long, requires = "serial")]
    require_desktop: bool,
    /// Fail if the capture contains a kernel panic
    #[arg(long, requires = "serial")]
    deny_panic: bool,
    /// Require exit code 0 on the latest attempt for this full Windows path (repeatable)
    #[arg(long = "require-exit", requires = "serial")]
    require_exit: Vec<String>,
    /// Require an exit and both native and interpreted retirement on the latest attempt (repeatable)
    #[arg(long = "require-jit", requires = "serial")]
    require_jit: Vec<String>,
}

#[derive(Deserialize, Serialize)]
struct ImageManifest {
    path: String,
    boot_chain_verified: bool,
    source_sha256: String,
    expected_source_sha256: Option<String>,
    base_sha256: String,
    boot_disk_sha256: String,
}

impl ImageManifest {
    fn digest_metadata_consistent(&self) -> bool {
        let is_digest = |digest: &str| {
            digest.len() == 64 && digest.as_bytes().iter().all(u8::is_ascii_hexdigit)
        };
        is_digest(&self.source_sha256)
            && is_digest(&self.boot_disk_sha256)
            && self.source_sha256.eq_ignore_ascii_case(&self.base_sha256)
            && self
                .expected_source_sha256
                .as_ref()
                .is_none_or(|expected| expected.eq_ignore_ascii_case(&self.source_sha256))
    }
}

#[derive(Debug, Serialize)]
struct Event {
    line: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    run: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    guest: Option<String>,
    #[serde(flatten)]
    kind: EventKind,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum EventKind {
    Loading,
    Image {
        pe_machine: u16,
        guest_abi: String,
        host_arch: String,
        backend: String,
    },
    Context {
        pe_machine: u16,
        guest_abi: String,
        host_arch: String,
        backend: String,
        pid: u32,
        tid: u32,
    },
    Running,
    Progress {
        retired: u64,
        pc: u64,
        threads: Option<(u32, u32, u32)>,
        #[serde(skip_serializing_if = "Option::is_none")]
        tid: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        module: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        rva: Option<u64>,
    },
    Translation {
        native: u64,
        interpreted: u64,
    },
    Exit {
        code: u32,
    },
    LoadFailure {
        ntstatus: u32,
        detail: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        missing_import: Option<String>,
    },
    ExecutionFault {
        ntstatus: u32,
        detail: String,
    },
    BudgetExceeded,
    FaultDetails {
        detail: String,
    },
    DelayLoadFault {
        dll: String,
        symbol: String,
    },
    KernelPanic {
        detail: String,
    },
    NativeHandoffUnavailable {
        detail: String,
    },
}

#[derive(Default, Serialize)]
struct Report {
    boot_markers: Vec<(&'static str, usize)>,
    boot_chain_observed: bool,
    desktop_observed: bool,
    scheduler_records_removed: usize,
    incomplete_tail: bool,
    events: Vec<Event>,
    project_image: Option<ImageManifest>,
    failures: Vec<String>,
}

fn extract_scheduler_record(line: &[u8]) -> Option<usize> {
    let line = std::str::from_utf8(line).ok()?;
    let mut fields = line.strip_prefix("[SCHED-SWITCH] ")?.split(' ');
    for field in ["n=", "cur=", "next=", "nrsp="] {
        let hex = fields.next()?.strip_prefix(field)?.strip_prefix("0x")?;
        if hex.len() != 16 || !hex.as_bytes().iter().all(u8::is_ascii_hexdigit) {
            return None;
        }
    }
    fields.next().is_none().then_some(line.len())
}

fn guest_path(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    (bytes.first().is_some_and(u8::is_ascii_alphabetic)
        && matches!(bytes.get(1..3), Some(b":\\" | b":/"))
        && !value
            .chars()
            .any(|character| character.is_control() || character == '\u{fffd}'))
    .then(|| value.to_owned())
}

fn parse_event(line: &str, line_number: usize) -> Option<Event> {
    let (line, run) = if line.starts_with("[PE] ") {
        line.rsplit_once(" [run=")
            .and_then(|(record, suffix)| Some((record, Some(suffix.strip_suffix(']')?.parse().ok()?))))
            .unwrap_or((line, None))
    } else {
        (line, None)
    };
    let event = |path: &str, kind: EventKind| Event {
        line: line_number,
        run,
        guest: guest_path(path),
        kind,
    };
    if line.starts_with("KERNEL PANIC:") {
        return Some(event(
            "",
            EventKind::KernelPanic {
                detail: line.into(),
            },
        ));
    }
    if line.contains("native loader parameter block handoff unavailable") {
        return Some(event(
            "",
            EventKind::NativeHandoffUnavailable {
                detail: line.into(),
            },
        ));
    }
    let record = line.strip_prefix("[PE] ")?;
    if let Some(path) = record.strip_prefix("Loading ") {
        return Some(event(path, EventKind::Loading));
    }
    if let Some(record) = record.strip_prefix("Image ") {
        let (path, fields) = record.split_once(": pe_machine=")?;
        let (machine, fields) = fields.split_once(" guest_abi=")?;
        let (abi, fields) = fields.split_once(" host_arch=")?;
        let (host_arch, backend) = fields.split_once(" backend=")?;
        return Some(event(path, EventKind::Image {
            pe_machine: u16::from_str_radix(machine, 16).ok()?,
            guest_abi: abi.into(),
            host_arch: host_arch.into(),
            backend: backend.into(),
        }));
    }
    if let Some(record) = record.strip_prefix("Context ") {
        let (path, fields) = record.split_once(": pe_machine=")?;
        let (machine, fields) = fields.split_once(" guest_abi=")?;
        let (abi, fields) = fields.split_once(" host_arch=")?;
        let (host_arch, fields) = fields.split_once(" backend=")?;
        let (backend, fields) = fields.split_once(" pid=")?;
        let (pid, tid) = fields.split_once(" tid=")?;
        return Some(event(
            path,
            EventKind::Context {
                pe_machine: u16::from_str_radix(machine, 16).ok()?,
                guest_abi: abi.into(),
                host_arch: host_arch.into(),
                backend: backend.into(),
                pid: pid.parse().ok()?,
                tid: tid.parse().ok()?,
            },
        ));
    }
    if let Some(path) = record.strip_prefix("Running ") {
        return Some(event(path, EventKind::Running));
    }
    if let Some(record) = record.strip_prefix("Progress ") {
        let (path, counters) = record.split_once(": retired=")?;
        let (retired, pc) = counters.split_once(" pc=0x")?;
        let (pc, tid, module, rva) = match pc.split_once(" tid=") {
            Some((pc, location)) => match location.split_once(" module=") {
                Some((tid, module)) => {
                    let (module, rva) = module.rsplit_once(" rva=0x")?;
                    (pc, Some(tid.parse().ok()?), Some(module.into()), Some(u64::from_str_radix(rva, 16).ok()?))
                }
                None => (pc, Some(location.parse().ok()?), None, None),
            },
            None => (pc, None, None, None),
        };
        let (pc, threads) = match pc.split_once(" runnable=") {
            Some((pc, counts)) => {
                let (runnable, counts) = counts.split_once(" waiting=")?;
                let (waiting, suspended) = counts.split_once(" suspended=")?;
                (pc, Some((runnable.parse().ok()?, waiting.parse().ok()?, suspended.parse().ok()?)))
            }
            None => (pc, None),
        };
        return Some(event(
            path,
            EventKind::Progress {
                retired: retired.parse().ok()?,
                pc: u64::from_str_radix(pc, 16).ok()?,
                threads,
                tid,
                module,
                rva,
            },
        ));
    }
    if let Some(record) = record.strip_prefix("Translation ") {
        let (path, counters) = record.split_once(": native=")?;
        let (native, interpreted) = counters.split_once(" interpreted=")?;
        return Some(event(
            path,
            EventKind::Translation {
                native: native.parse().ok()?,
                interpreted: interpreted.parse().ok()?,
            },
        ));
    }
    if let Some(record) = record.strip_prefix("Guest exited ") {
        let (path, code) = record.split_once(": ")?;
        return Some(event(
            path,
            EventKind::Exit {
                code: code.parse().ok()?,
            },
        ));
    }
    if let Some(details) = record.strip_prefix("Fault details: ") {
        return Some(event(
            "",
            EventKind::FaultDetails {
                detail: details.into(),
            },
        ));
    }
    if let Some(record) = record.strip_prefix("Delay-load fault in ") {
        let (path, target) = record.split_once(": ")?;
        let (dll, symbol) = target.split_once('!')?;
        if dll.is_empty() || symbol.is_empty() {
            return None;
        }
        return Some(event(
            path,
            EventKind::DelayLoadFault {
                dll: dll.into(),
                symbol: symbol.into(),
            },
        ));
    }
    if let Some(path) = record
        .strip_prefix("Guest instruction budget exceeded in ")
        .and_then(|path| path.strip_suffix("; console invocation stopped."))
    {
        return Some(event(path, EventKind::BudgetExceeded));
    }
    let (diagnostic, fault) = if let Some(diagnostic) = record.strip_prefix("Cannot execute ") {
        (diagnostic, false)
    } else {
        (record.strip_prefix("Guest execution fault in ")?, true)
    };
    let (path, details) = diagnostic.split_once(": ")?;
    let (detail, status) = details.rsplit_once("; NTSTATUS 0x")?;
    let status = status.strip_suffix('.')?;
    if status.len() != 8 {
        return None;
    }
    let ntstatus = u32::from_str_radix(status, 16).ok()?;
    let detail = detail.to_owned();
    let kind = if fault {
        EventKind::ExecutionFault { ntstatus, detail }
    } else {
        let missing_import = detail
            .strip_prefix("guest DLL ")
            .and_then(|record| record.split_once(" has no export "))
            .and_then(|(dll, symbol)| {
                if let Some(name) = symbol
                    .strip_prefix("Name(\"")
                    .and_then(|name| name.strip_suffix("\")"))
                {
                    return Some(format!("{dll}!{name}"));
                }
                let ordinal = symbol
                    .strip_prefix("Ordinal(")?
                    .strip_suffix(')')?
                    .parse::<u16>()
                    .ok()?;
                Some(format!("{dll}!#{ordinal}"))
            });
        EventKind::LoadFailure {
            ntstatus,
            detail,
            missing_import,
        }
    };
    Some(event(path, kind))
}

fn inspect(mut input: impl BufRead) -> io::Result<Report> {
    const SCHED_PREFIX: &[u8] = b"[SCHED-SWITCH] ";
    let mut report = Report::default();
    let mut fragment = Vec::new();
    let mut buffer = Vec::new();
    let mut line_number = 1;
    let mut record_start = 1;
    while input.read_until(b'\n', &mut buffer)? != 0 {
        let Some(line) = buffer.strip_suffix(b"\n") else {
            report.incomplete_tail = true;
            break;
        };
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let scheduler_at = line
            .windows(SCHED_PREFIX.len())
            .rposition(|window| window == SCHED_PREFIX)
            .filter(|&start| extract_scheduler_record(&line[start..]).is_some());
        if let Some(start) = scheduler_at {
            fragment.extend_from_slice(&line[..start]);
            report.scheduler_records_removed += 1;
        } else {
            fragment.extend_from_slice(line);
            let record = String::from_utf8_lossy(&fragment);
            if let Some(&marker) = BOOT_MARKERS.get(report.boot_markers.len()) {
                if record.starts_with(marker) {
                    report.boot_markers.push((marker, record_start));
                }
            }
            if let Some(event) = parse_event(&record, record_start) {
                report.events.push(event);
            }
            fragment.clear();
        }
        line_number += 1;
        if fragment.is_empty() {
            record_start = line_number;
        }
        buffer.clear();
    }
    report.incomplete_tail |= !fragment.is_empty();
    report.boot_chain_observed = report.boot_markers.len() >= BOOT_CHAIN_LEN;
    report.desktop_observed = report.boot_markers.len() == BOOT_MARKERS.len();
    Ok(report)
}

impl Report {
    fn latest_guest_records<'a>(&'a self, path: &'a str) -> impl Iterator<Item = &'a EventKind> {
        let path = path.replace('/', "\\");
        let matches_path = move |event: &&Event| {
            event.guest.as_ref().is_some_and(|guest| path.eq_ignore_ascii_case(&guest.replace('/', "\\")))
        };
        let records = self.events.iter().rev().filter(matches_path);
        let latest = records.clone().find(|event| event.kind == EventKind::Loading);
        records
            .take_while(move |event| latest.is_none_or(|start| event.line > start.line))
            .filter(move |event| latest.is_none_or(|start| event.run == start.run))
            .map(|event| &event.kind)
    }

    fn check(&mut self, args: &Args) {
        if args.require_boot && !self.boot_chain_observed {
            self.failures
                .push("ordered project boot chain is incomplete".into());
        }
        if args.require_desktop && !self.desktop_observed {
            self.failures
                .push("project desktop milestone is missing or out of order".into());
        }
        if args.deny_panic
            && self
                .events
                .iter()
                .any(|event| matches!(event.kind, EventKind::KernelPanic { .. }))
        {
            self.failures.push("kernel panic observed".into());
        }
        for path in &args.require_exit {
            let completed = matches!(
                self.latest_guest_records(path).next(),
                Some(EventKind::Exit { code: 0 })
            );
            if !completed {
                self.failures
                    .push(format!("{path}: latest attempt has no successful exit"));
            }
        }
        for path in &args.require_jit {
            let mut records = self.latest_guest_records(path);
            let completed = matches!(records.next(), Some(EventKind::Exit { .. }));
            let native = records
                .take_while(|kind| **kind != EventKind::Running)
                .any(|kind| matches!(kind, EventKind::Translation { native, interpreted } if *native > 0 && *interpreted > 0));
            if !(completed && native) {
                self.failures.push(format!(
                    "{path}: latest attempt has no completed mixed native/interpreted translation"
                ));
            }
        }
        if let Some(image) = &self.project_image {
            if !image.digest_metadata_consistent() {
                self.failures
                    .push("project image manifest has inconsistent SHA-256 metadata".into());
            }
            if image.boot_chain_verified != self.boot_chain_observed {
                self.failures.push(
                    "project image manifest disagrees with the capture's boot markers".into(),
                );
            }
        }
    }

    fn write_text(&self, mut out: impl Write) -> io::Result<()> {
        writeln!(
            out,
            "Project boot markers: {}/{}",
            self.boot_markers.len(),
            BOOT_MARKERS.len()
        )?;
        for (marker, line) in &self.boot_markers {
            writeln!(out, "  {line}: {marker}")?;
        }
        if let Some(image) = &self.project_image {
            writeln!(
                out,
                "Manifest project image: {} (source SHA-256 {})",
                image.path, image.source_sha256
            )?;
        }
        for event in &self.events {
            let guest = event.guest.as_deref().unwrap_or("<unattributed>");
            write!(out, "{}: ", event.line)?;
            match &event.kind {
                EventKind::Loading => writeln!(out, "{guest} loading")?,
                EventKind::Image { pe_machine, guest_abi, host_arch, backend } => writeln!(
                    out, "{guest} machine={pe_machine:04X} abi={guest_abi} host={host_arch} backend={backend}"
                )?,
                EventKind::Context { pe_machine, guest_abi, host_arch, backend, pid, tid } => writeln!(
                    out, "{guest} machine={pe_machine:04X} abi={guest_abi} host={host_arch} backend={backend} pid={pid} tid={tid}"
                )?,
                EventKind::Running => writeln!(out, "{guest} running")?,
                EventKind::Progress { retired, pc, threads, tid, module, rva } => {
                    write!(out, "{guest} retired={retired} pc=0x{pc:X}")?;
                    if let Some((runnable, waiting, suspended)) = threads {
                        write!(out, " runnable={runnable} waiting={waiting} suspended={suspended}")?;
                    }
                    if let Some(tid) = tid {
                        write!(out, " tid={tid}")?;
                    }
                    if let (Some(module), Some(rva)) = (module, rva) {
                        write!(out, " module={module} rva=0x{rva:X}")?;
                    }
                    writeln!(out)?;
                }
                EventKind::Translation {
                    native,
                    interpreted,
                } => writeln!(
                    out,
                    "{guest} translated native={native} interpreted={interpreted}"
                )?,
                EventKind::Exit { code } => writeln!(out, "{guest} exited {code}")?,
                EventKind::LoadFailure { ntstatus, detail, .. } => {
                    writeln!(out, "{guest} could not load (0x{ntstatus:08X}): {detail}")?
                }
                EventKind::ExecutionFault { ntstatus, detail } => {
                    writeln!(out, "{guest} execution fault (0x{ntstatus:08X}): {detail}")?
                }
                EventKind::BudgetExceeded => writeln!(out, "{guest} instruction budget exceeded")?,
                EventKind::FaultDetails { detail } => {
                    writeln!(out, "Guest fault details: {detail}")?
                }
                EventKind::DelayLoadFault { dll, symbol } => {
                    writeln!(out, "{guest} delay-load fault on {dll}!{symbol}")?
                }
                EventKind::KernelPanic { detail }
                | EventKind::NativeHandoffUnavailable { detail } => writeln!(out, "{detail}")?,
            }
        }
        if self.incomplete_tail {
            writeln!(out, "Ignored incomplete final serial record")?;
        }
        for failure in &self.failures {
            writeln!(out, "FAIL: {failure}")?;
        }
        Ok(())
    }
}

#[derive(Serialize)]
struct ImportReport {
    machine: u16,
    failures: Vec<ImportFailure>,
}

#[derive(Serialize)]
struct ImportFailure {
    dll: String,
    symbol: Option<String>,
    problem: &'static str,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    forwarders: Vec<String>,
}

struct RuntimeLibraries {
    machine: u16,
    paths: BTreeMap<String, PathBuf>,
    images: BTreeMap<String, Result<pe_read::PeImage, &'static str>>,
}

fn module_name(name: &str) -> Option<String> {
    if name.is_empty()
        || name.len() > 128
        || name.starts_with(['.', ' '])
        || name.ends_with(' ')
        || name.contains("..")
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._- ".contains(&byte))
    {
        return None;
    }
    let mut name = name.to_ascii_lowercase();
    if !name.contains('.') {
        name.push_str(".dll");
    }
    Some(name)
}

impl RuntimeLibraries {
    fn image(&mut self, module: &str) -> Result<&pe_read::PeImage, &'static str> {
        self.images
            .entry(module.to_owned())
            .or_insert_with(|| {
                let path = self.paths.get(module).ok_or("missing_module")?;
                let bytes = std::fs::read(path).map_err(|_| "unreadable_module")?;
                let image = pe_read::parse(&bytes).ok_or("invalid_module")?;
                if image.machine != self.machine {
                    return Err("wrong_machine");
                }
                Ok(image)
            })
            .as_ref()
            .map_err(|problem| *problem)
    }

    fn resolve(
        &mut self,
        dll: &str,
        symbol: &str,
        forwarders: &mut Vec<String>,
    ) -> Result<(), &'static str> {
        let mut module = module_name(dll).ok_or("invalid_module_name")?;
        let mut symbol = symbol.to_owned();
        let mut visited = BTreeSet::new();
        loop {
            if !visited.insert((module.clone(), symbol.clone())) {
                return Err("forwarder_cycle");
            }
            if visited.len() > 64 {
                return Err("forwarder_depth");
            }
            let image = self.image(&module)?;
            let rva = if let Some(ordinal) = symbol.strip_prefix('#') {
                let ordinal = ordinal.parse::<u16>().map_err(|_| "invalid_ordinal")?;
                image.exports_by_ordinal.get(&ordinal)
            } else {
                image.exports_by_name.get(&symbol)
            }
            .ok_or("missing_export")?;
            if let Some(target) = image.forwarders_by_rva.get(rva) {
                forwarders.push(target.clone());
                let (dll, next) = target.rsplit_once('.').ok_or("invalid_forwarder")?;
                module = module_name(dll).ok_or("invalid_forwarder")?;
                symbol = next.to_owned();
            } else {
                let (start, size) = image.data_directories.first().copied().unwrap_or_default();
                return if *rva >= start && *rva - start < size {
                    Err("invalid_forwarder")
                } else {
                    Ok(())
                };
            }
        }
    }
}

fn check_imports(path: &Path, runtime: &Path) -> Result<ImportReport, Box<dyn std::error::Error>> {
    let bytes = std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let image = pe_read::parse(&bytes).ok_or_else(|| format!("{}: invalid PE", path.display()))?;
    let mut report = ImportReport {
        machine: image.machine,
        failures: Vec::new(),
    };
    let mut libraries = RuntimeLibraries {
        machine: image.machine,
        paths: BTreeMap::new(),
        images: BTreeMap::new(),
    };
    let application = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    for directory in [runtime, application] {
        let mut entries = std::fs::read_dir(directory)?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_unstable_by_key(|entry| entry.file_name());
        for entry in entries {
            if entry.file_type()?.is_dir() {
                continue;
            }
            if let Some(name) = entry
                .file_name()
                .to_str()
                .filter(|name| name.contains('.'))
                .and_then(module_name)
            {
                libraries.paths.insert(name, entry.path());
            }
        }
    }
    for import in image.imports {
        let mut fail = |symbol, problem, forwarders| {
            report.failures.push(ImportFailure {
                dll: import.dll_name.clone(),
                symbol,
                problem,
                forwarders,
            })
        };
        let module = module_name(&import.dll_name).ok_or("invalid_module_name");
        if let Err(problem) = module.and_then(|module| libraries.image(&module).map(|_| ())) {
            fail(None, problem, Vec::new());
            continue;
        }
        for symbol in import.symbols.into_iter().flatten().chain(
            import
                .ordinals
                .into_iter()
                .map(|ordinal| format!("#{ordinal}")),
        ) {
            let mut forwarders = Vec::new();
            if let Err(problem) = libraries.resolve(&import.dll_name, &symbol, &mut forwarders) {
                fail(Some(symbol), problem, forwarders);
            }
        }
    }
    Ok(report)
}

fn run(args: Args) -> Result<ExitCode, Box<dyn std::error::Error>> {
    if let (Some(path), Some(runtime)) = (&args.pe_imports, &args.runtime_dir) {
        let report = check_imports(path, runtime)?;
        let mut out = io::stdout().lock();
        if args.json {
            serde_json::to_writer_pretty(&mut out, &report)?;
            writeln!(out)?;
        } else {
            writeln!(
                out,
                "PE machine 0x{:04X}, {} import failures",
                report.machine,
                report.failures.len()
            )?;
            for failure in &report.failures {
                write!(
                    out,
                    "{} {} {}",
                    failure.dll,
                    failure.symbol.as_deref().unwrap_or(""),
                    failure.problem
                )?;
                if !failure.forwarders.is_empty() {
                    write!(out, " via {}", failure.forwarders.join(" -> "))?;
                }
                writeln!(out)?;
            }
        }
        return Ok(if report.failures.is_empty() {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        });
    }
    let serial = args.serial.as_deref().ok_or("serial log path required")?;
    let mut report = if serial.as_os_str() == "-" {
        inspect(io::stdin().lock())?
    } else {
        let file = File::open(serial)
            .map_err(|error| format!("open serial log {}: {error}", serial.display()))?;
        inspect(BufReader::new(file))?
    };
    if let Some(path) = &args.project_image {
        report.project_image = Some(
            (|| -> Result<_, Box<dyn std::error::Error>> {
                Ok(serde_json::from_reader(BufReader::new(File::open(path)?))?)
            })()
            .map_err(|error| format!("read image manifest {}: {error}", path.display()))?,
        );
    }
    report.check(&args);
    let mut out = io::stdout().lock();
    if args.json {
        serde_json::to_writer_pretty(&mut out, &report)?;
        writeln!(out)?;
    } else {
        report.write_text(&mut out)?;
    }
    Ok(if report.failures.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

fn main() -> ExitCode {
    match run(Args::parse()) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("nt61-debug: {error}");
            ExitCode::from(2)
        }
    }
}
