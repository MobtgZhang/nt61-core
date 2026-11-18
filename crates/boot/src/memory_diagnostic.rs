//! Windows 7 style pre-OS diagnostic controller, UI and owned physical RAM.
use crate::memory_result::{self, ResultRecord};
use crate::memory_test::{self, Cache, Memory, OwnedRegion, Report, TestMix};
use crate::{
    font::BitmapFont,
    input::Keyboard,
    renderer::{Color, Framebuffer},
};
use alloc::{format, vec::Vec};
use core::cell::Cell;
use uefi::{
    boot::{AllocateType, MemoryType},
    mem::memory_map::MemoryMap,
    proto::console::text::{Key, ScanCode},
    runtime::{VariableAttributes, VariableVendor},
};

const VENDOR: VariableVendor = VariableVendor(uefi::guid!("9d0f7e8a-8d21-4f7f-987f-3c9d0c5f4b61"));
const NAME: &uefi::CStr16 = uefi::cstr16!("NT61MemoryDiagnostic");
const BLUE: Color = Color(0x001c_007b);
const GRAY: Color = Color(0x00c0_c0c0);
const YELLOW: Color = Color(0x00ff_ff55);

fn marker(s: &str) {
    crate::arch::current::write_com1_marker(s.as_bytes());
}
fn read(input: &mut Option<&mut Keyboard>) -> Option<Key> {
    input.as_mut().and_then(|i| i.read_key())
}
fn stall() {
    uefi::boot::stall(core::time::Duration::from_millis(10));
}

#[derive(Clone, Copy)]
struct Config {
    mix: TestMix,
    cache: Cache,
    passes: u32,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            mix: TestMix::Standard,
            cache: Cache::Default,
            passes: 2,
        }
    }
}

/// Keep allocations owned until EVERY test phase is finished. Reserving and
/// releasing each chunk separately would miss cross-chunk address aliases.
struct Ram {
    regions: Vec<(OwnedRegion, core::ptr::NonNull<u8>, usize, usize)>,
    words: usize,
    current: Cell<usize>,
    available: u64,
    cpu: CpuCache,
}
impl Ram {
    fn reserve() -> Option<Self> {
        let map = uefi::boot::memory_map(MemoryType::LOADER_DATA).ok()?;
        let ranges: Vec<_> = map
            .entries()
            .filter(|d| d.ty == MemoryType::CONVENTIONAL && d.page_count > 0)
            .map(|d| (d.phys_start, d.page_count))
            .collect();
        drop(map);
        let available: u64 = ranges.iter().map(|(_, p)| p * 4096).sum();
        // Preallocate metadata BEFORE reserving RAM. Leave headroom for firmware
        // input, glyph rendering, NVRAM and memory-map changes while testing.
        let capacity = ranges
            .iter()
            .map(|(_, p)| p.div_ceil(256) as usize + 256)
            .sum();
        let mut ram = Self {
            regions: Vec::with_capacity(capacity),
            words: 0,
            current: Cell::new(0),
            available,
            cpu: CpuCache::default(),
        };
        let reserve_limit = available.saturating_sub(16 * 1024 * 1024);
        for (base, pages) in ranges {
            let mut page = 0;
            while page < pages && (ram.words as u64 * 8) < reserve_limit {
                let remaining = (reserve_limit - ram.words as u64 * 8) / 4096;
                let mut count = (pages - page).min(256).min(remaining) as usize;
                if count == 0 || ram.regions.len() == ram.regions.capacity() {
                    break;
                }
                let address = base + page * 4096;
                // A pool allocation may have claimed part of this descriptor
                // since the snapshot. Never write without successful reservation.
                let pointer = match uefi::boot::allocate_pages(
                    AllocateType::Address(address),
                    MemoryType::LOADER_DATA,
                    count,
                ) {
                    Ok(p) => Some(p),
                    Err(_) => {
                        count = 1;
                        uefi::boot::allocate_pages(
                            AllocateType::Address(address),
                            MemoryType::LOADER_DATA,
                            count,
                        )
                        .ok()
                    }
                };
                page += count as u64;
                if let Some(pointer) = pointer {
                    // SAFETY: AllocatePages reserves mapped, aligned RAM for us.
                    let region = unsafe { OwnedRegion::new(pointer.as_ptr().cast(), count * 512) };
                    ram.regions.push((region, pointer, count, ram.words));
                    ram.words += count * 512;
                }
            }
        }
        Some(ram)
    }
    fn region(&self, index: usize) -> usize {
        let current = self.current.get();
        if let Some((r, _, _, start)) = self.regions.get(current) {
            if index >= *start && index - *start < r.words() {
                return current;
            }
        }
        let next = self
            .regions
            .partition_point(|(_, _, _, start)| *start <= index)
            .saturating_sub(1);
        self.current.set(next);
        next
    }
}
impl Memory for Ram {
    fn words(&self) -> usize {
        self.words
    }
    fn address(&self, i: usize) -> u64 {
        let r = self.region(i);
        self.regions[r].0.address(i - self.regions[r].3)
    }
    fn write(&mut self, i: usize, value: u64) {
        let r = self.region(i);
        let start = self.regions[r].3;
        self.regions[r].0.write(i - start, value);
    }
    fn read(&mut self, i: usize) -> u64 {
        let r = self.region(i);
        let start = self.regions[r].3;
        self.regions[r].0.read(i - start)
    }
    fn set_cache(&mut self, enabled: bool) {
        self.cpu.set(enabled);
    }
    fn restore_cache(&mut self) {
        self.cpu.restore();
    }
}
impl Drop for Ram {
    fn drop(&mut self) {
        self.cpu.restore();
        for (_, pointer, count, _) in self.regions.drain(..) {
            // SAFETY: these exact allocations are ours and no test retains pointers.
            if unsafe { uefi::boot::free_pages(pointer, count) }.is_err() {
                marker("[MEMORY] release failed\r\n");
            }
        }
    }
}

/// x86 firmware executes privileged on the boot processor. APs remain parked.
/// Cache-disabled batches mask interrupts; original CR0/IF are restored before
/// ANY firmware call, input polling, allocation or painting, including cancel.
#[derive(Default)]
struct CpuCache {
    #[cfg(target_arch = "x86_64")]
    saved: Option<(u64, u64)>,
}
impl CpuCache {
    fn set(&mut self, enabled: bool) {
        #[cfg(target_arch = "x86_64")]
        if self.saved.is_none() {
            let cr0: u64;
            let flags: u64;
            // SAFETY: UEFI runs at CPL0. Read CR0 without changing firmware state.
            unsafe {
                core::arch::asm!("mov {}, cr0", out(reg) cr0, options(nostack));
            }
            let desired = if enabled {
                cr0 & !((1 << 30) | (1 << 29))
            } else {
                (cr0 | (1 << 30)) & !(1 << 29)
            };
            if desired == cr0 {
                return;
            }
            // SAFETY: no firmware calls occur until restore; APs remain parked.
            unsafe {
                core::arch::asm!("pushfq", "pop {}", "cli", out(reg) flags);
                core::arch::asm!("wbinvd", "mov cr0, {}", "wbinvd", in(reg) desired, options(nostack));
            }
            self.saved = Some((cr0, flags));
        }
        #[cfg(not(target_arch = "x86_64"))]
        let _ = enabled;
    }
    fn restore(&mut self) {
        #[cfg(target_arch = "x86_64")]
        if let Some((cr0, flags)) = self.saved.take() {
            // SAFETY: restore exactly the state saved by set, before firmware runs.
            unsafe {
                core::arch::asm!("wbinvd", "mov cr0, {}", "wbinvd", in(reg) cr0, options(nostack));
                if flags & (1 << 9) != 0 {
                    core::arch::asm!("sti", options(nostack));
                }
            }
        }
    }
}
impl Drop for CpuCache {
    fn drop(&mut self) {
        self.restore();
    }
}

/// Logical 640x480 layout from the Win7 diagnostic, scaled to the active GOP.
/// Text-console fallback prints the same states and accepts the same keys.
struct Screen<'a> {
    fb: &'a mut Framebuffer,
    font: &'a mut BitmapFont,
}
impl Screen<'_> {
    fn x(&self, x: u32) -> u32 {
        x * self.fb.width() / 640
    }
    fn y(&self, y: u32) -> u32 {
        y * self.fb.height() / 480
    }
    fn rect(&mut self, x: u32, y: u32, w: u32, h: u32, color: Color) {
        self.fb
            .fill_rect_fast(self.x(x), self.y(y), self.x(w), self.y(h), color);
    }
    fn text(&mut self, text: &str, x: u32, y: u32, color: Color) {
        if self.fb.is_text_console() {
            uefi::println!("{}", text);
            return;
        }
        self.font.draw_text(
            self.fb,
            text,
            self.x(x) as i32,
            self.y(y) as i32,
            color,
            None,
        );
    }
    fn base(&mut self, options: bool) {
        if self.fb.is_text_console() {
            uefi::println!("\nWindows Memory Diagnostics Tool");
            return;
        }
        self.font.set_size((13 * self.fb.height() / 480).max(10));
        self.fb
            .fill_rect_fast(0, 0, self.fb.width(), self.fb.height(), BLUE);
        self.rect(20, 3, 600, 19, GRAY);
        let title_y = self.y(3);
        self.font.draw_text_centered(
            self.fb,
            "Windows Memory Diagnostics Tool",
            (self.fb.width() / 2) as i32,
            title_y as i32,
            BLUE,
            None,
        );
        self.rect(20, 454, 600, 19, GRAY);
        if options {
            self.text("TAB=Select   F10=Apply", 27, 455, BLUE);
            self.text("ESC=Cancel", 532, 455, BLUE);
        } else {
            self.text("F1=Options", 27, 455, BLUE);
            self.text("ESC=Exit", 548, 455, BLUE);
        }
    }
    fn running(&mut self, config: Config, pass: u32, percent: u32, overall: u32, report: Report) {
        self.base(false);
        self.text("Windows is checking for memory problems...", 20, 64, GRAY);
        self.text("This might take several minutes.", 20, 83, GRAY);
        let count = if config.passes == 0 {
            format!("Running test pass {}: {:02}% complete", pass, percent)
        } else {
            format!(
                "Running test pass {} of {}: {:02}% complete",
                pass, config.passes, percent
            )
        };
        self.text(&count, 20, 120, GRAY);
        self.text(
            &format!("Overall test status: {:02}% complete", overall),
            20,
            139,
            GRAY,
        );
        self.rect(20, 155, 600 * overall.min(100) / 100, 18, GRAY);
        self.text("Status:", 20, 214, GRAY);
        self.text(
            if report.errors == 0 {
                "No problems have been detected yet."
            } else {
                "Hardware problems were detected. To identify and repair these"
            },
            20,
            233,
            YELLOW,
        );
        if report.errors > 0 {
            self.text(
                "problems, you will need to contact the computer manufacturer.",
                20,
                252,
                YELLOW,
            );
        }
        self.text(
            "Although the test may appear inactive at times, it is still running. Please",
            20,
            271,
            YELLOW,
        );
        self.text("wait until testing is complete...", 20, 290, YELLOW);
        self.text(
            "Windows will restart the computer automatically. Test results will be",
            20,
            328,
            GRAY,
        );
        self.text("displayed again after you log on.", 20, 347, GRAY);
        self.fb.present();
    }
    fn notice(&mut self, message: &str) {
        self.base(false);
        self.text(message, 20, 64, YELLOW);
        self.text(
            "Press ESC to return to Windows Boot Manager.",
            20,
            120,
            GRAY,
        );
        self.fb.present();
    }
    fn options(&mut self, draft: Config, field: usize, pass_text: &str, invalid: bool) {
        self.base(true);
        self.text("Options", 20, 45, GRAY);
        self.text("Test mix:", 20, 74, GRAY);
        self.text("Choose the type of test to run.", 20, 93, GRAY);
        for (n, (mix, description)) in [
            (TestMix::Basic, "Basic     MATS+, INVC, SCHCKR"),
            (
                TestMix::Standard,
                "Standard  Basic plus LRAND, Stride6, CHCKR3, WMATS+, WINVC",
            ),
            (
                TestMix::Extended,
                "Extended  Standard tests plus all extended tests",
            ),
        ]
        .iter()
        .enumerate()
        {
            self.text(
                &format!(
                    "{} {}",
                    if draft.mix == *mix { "(*)" } else { "( )" },
                    description
                ),
                30,
                120 + n as u32 * 19,
                if field == 0 { YELLOW } else { GRAY },
            );
        }
        self.text("Cache:", 20, 195, GRAY);
        self.text("Choose the cache setting for each test.", 20, 214, GRAY);
        self.text(
            &format!(
                "{} Default   {} On   {} Off",
                if draft.cache == Cache::Default {
                    "(*)"
                } else {
                    "( )"
                },
                if draft.cache == Cache::On {
                    "(*)"
                } else {
                    "( )"
                },
                if draft.cache == Cache::Off {
                    "(*)"
                } else {
                    "( )"
                }
            ),
            30,
            241,
            if field == 1 { YELLOW } else { GRAY },
        );
        self.text("Pass count:", 20, 289, GRAY);
        self.text(
            "Enter the number of times you want to repeat the test.",
            20,
            308,
            GRAY,
        );
        self.text(
            "Enter 0 to run the test indefinitely (0-99).",
            20,
            327,
            GRAY,
        );
        self.text(
            &format!("[ {:2} ]", pass_text),
            30,
            354,
            if field == 2 { YELLOW } else { GRAY },
        );
        if invalid {
            self.text("Enter a pass count from 0 to 99.", 20, 393, YELLOW);
        }
        self.fb.present();
    }
}

fn options(
    screen: &mut Screen<'_>,
    input: &mut Option<&mut Keyboard>,
    config: Config,
) -> Option<Config> {
    marker("[MEMORY] options\r\n");
    let mut draft = config;
    let mut field = 0usize;
    let mut pass_text = format!("{}", draft.passes);
    let mut invalid = false;
    loop {
        screen.options(draft, field, &pass_text, invalid);
        let key = loop {
            if let Some(k) = read(input) {
                break k;
            }
            stall();
        };
        match key {
            Key::Special(ScanCode::ESCAPE) => {
                marker("[MEMORY] resumed\r\n");
                return None;
            }
            Key::Special(ScanCode::FUNCTION_10) => {
                if let Ok(passes) = pass_text.parse::<u32>() {
                    if passes <= 99 {
                        draft.passes = passes;
                        // Other architectures still perform real memory sweeps;
                        // do not silently label cached reads as uncached testing.
                        if cfg!(target_arch = "x86_64")
                            || (draft.cache != Cache::Off
                                && !(draft.cache == Cache::Default
                                    && draft.mix == TestMix::Extended))
                        {
                            return Some(draft);
                        }
                        screen.notice("Cache Off is unavailable on this processor.");
                        while !matches!(read(input), Some(Key::Special(ScanCode::ESCAPE))) {
                            stall();
                        }
                    }
                }
                invalid = true;
            }
            Key::Special(code)
                if [
                    ScanCode::UP,
                    ScanCode::DOWN,
                    ScanCode::LEFT,
                    ScanCode::RIGHT,
                ]
                .contains(&code) =>
            {
                let previous = code == ScanCode::UP || code == ScanCode::LEFT;
                if field == 0 {
                    draft.mix = if previous {
                        draft.mix.previous()
                    } else {
                        draft.mix.next()
                    };
                }
                if field == 1 {
                    draft.cache = if previous {
                        draft.cache.previous()
                    } else {
                        draft.cache.next()
                    };
                }
                if field == 2 {
                    let p = pass_text.parse::<u32>().unwrap_or(0);
                    pass_text = format!(
                        "{}",
                        if previous {
                            (p + 99) % 100
                        } else {
                            (p + 1) % 100
                        }
                    );
                }
            }
            Key::Printable(c) => match u16::from(c) {
                9 => field = (field + 1) % 3,
                8 if field == 2 => {
                    pass_text.pop();
                    invalid = false;
                }
                n @ 48..=57 if field == 2 && pass_text.len() < 2 => {
                    pass_text.push(char::from(n as u8));
                    invalid = false;
                }
                _ => {}
            },
            _ => {}
        }
    }
}
fn save(record: ResultRecord) -> bool {
    match uefi::runtime::set_variable(
        NAME,
        &VENDOR,
        VariableAttributes::NON_VOLATILE
            | VariableAttributes::BOOTSERVICE_ACCESS
            | VariableAttributes::RUNTIME_ACCESS,
        &record.encode(),
    ) {
        Ok(()) => {
            marker(&format!(
                "[MEMORY] saved result status={}\r\n",
                record.status
            ));
            true
        }
        Err(e) => {
            marker(&format!("[MEMORY] save failed {:?}\r\n", e.status()));
            false
        }
    }
}

fn next_run_id() -> u64 {
    let name = uefi::cstr16!("NT61MemoryDiagnosticSequence");
    let mut bytes = [0u8; 8];
    let previous = uefi::runtime::get_variable(name, &VENDOR, &mut bytes)
        .ok().and_then(|(value, _)| <[u8; 8]>::try_from(value).ok())
        .map(u64::from_le_bytes).unwrap_or(0);
    let next = previous.wrapping_add(1).max(1);
    if uefi::runtime::set_variable(name, &VENDOR,
        VariableAttributes::NON_VOLATILE | VariableAttributes::BOOTSERVICE_ACCESS
            | VariableAttributes::RUNTIME_ACCESS, &next.to_le_bytes()).is_ok() {
        next
    } else {
        // Still test RAM; an unsequenced result is delivered without suppressing
        // a retry based on a potentially stale on-disk acknowledgement.
        marker("[MEMORY] sequence unavailable\r\n");
        0
    }
}

pub fn run(fb: &mut Framebuffer, font: &mut BitmapFont, mut input: Option<&mut Keyboard>) {
    let mut screen = Screen { fb, font };
    // UEFI starts a five-minute image watchdog. A real sweep (or an indefinite
    // run) can legitimately take longer without calling ExitBootServices.
    match uefi::boot::set_watchdog_timer(0, 0x1_0000, None) {
        Ok(()) => marker("[MEMORY] watchdog disabled\r\n"),
        Err(e) if e.status() == uefi::Status::UNSUPPORTED => {},
        Err(_) => {
            screen.notice("The firmware watchdog could not be disabled. Testing cannot start.");
            while !matches!(read(&mut input), Some(Key::Special(ScanCode::ESCAPE))) {
                stall();
            }
            return;
        }
    }
    let mut config = Config::default();
    let run_id = next_run_id();
    // Invalidate an earlier success BEFORE testing. Cancellation cannot revive it.
    let _ = uefi::runtime::delete_variable(NAME, &VENDOR);
    'restart: loop {
        marker(&format!(
            "[MEMORY] begin mix={} passes={} cache={}\r\n",
            config.mix.name(),
            config.passes,
            config.cache.name()
        ));
        screen.running(config, 1, 0, 0, Report::default());
        let Some(mut ram) = Ram::reserve() else {
            screen.notice("The memory map could not be read. Testing cannot continue.");
            while !matches!(read(&mut input), Some(Key::Special(ScanCode::ESCAPE))) {
                stall();
            }
            return;
        };
        let bytes = ram.words as u64 * 8;
        let mut record = ResultRecord {
            run_id,
            available: ram.available,
            excluded: ram.available.saturating_sub(bytes),
            ..ResultRecord::default()
        };
        marker(&format!(
            "[MEMORY] reserved bytes={} excluded={} regions={} first={:#x}\r\n",
            bytes,
            record.excluded,
            ram.regions.len(),
            ram.regions.first().map_or(0, |r| r.0.address(0))
        ));
        if bytes == 0 {
            record.status = memory_result::INCONCLUSIVE;
            save(record);
            screen.notice("No reservable memory was tested. The result is inconclusive.");
            while !matches!(read(&mut input), Some(Key::Special(ScanCode::ESCAPE))) {
                stall();
            }
            return;
        }
        let mut report = Report::default();
        let mut pass = 0u32;
        loop {
            pass = pass.saturating_add(1);
            let mut last_percent = 101;
            for (test_index, &kind) in config.mix.tests().iter().enumerate() {
                marker(&format!(
                    "[MEMORY] test pass={} name={} cache={}\r\n",
                    pass,
                    kind.name(),
                    if kind.cache_enabled(config.cache) {
                        "On"
                    } else {
                        "Off"
                    }
                ));
                let mut exit = false;
                let mut changed = None;
                let prior = report;
                let mut observed = report;
                let mut last_input = 0;
                let outcome = memory_test::run(&mut ram, kind, config.cache, |progress| {
                    let errors = prior.errors.saturating_add(progress.report.errors);
                    let current = Report {
                        errors,
                        first: prior.first.or(progress.report.first),
                    };
                    observed = current;
                    // Firmware consoles can probe several USB/serial devices per
                    // read. Poll every 4 MiB of work, rather than on every
                    // cache-restoration checkpoint; still poll at both endpoints.
                    let key = if progress.done == 0 || progress.done == progress.total
                        || progress.done.saturating_sub(last_input) >= 524288 {
                        last_input = progress.done;
                        read(&mut input)
                    } else { None };
                    if let Some(key) = key {
                        match key {
                            Key::Special(ScanCode::ESCAPE) => {
                                exit = true;
                                return false;
                            }
                            Key::Special(ScanCode::FUNCTION_1) => {
                                if let Some(next) = options(&mut screen, &mut input, config) {
                                    changed = Some(next);
                                    return false;
                                }
                                last_percent = 101;
                            }
                            _ => {}
                        }
                    }
                    let percent = ((test_index as u64 * 100
                        + progress.done * 100 / progress.total.max(1))
                        / config.mix.tests().len() as u64) as u32;
                    if percent != last_percent {
                        last_percent = percent;
                        let overall = if config.passes == 0 {
                            percent
                        } else {
                            ((u64::from(pass - 1) * 100 + u64::from(percent))
                                / u64::from(config.passes)) as u32
                        };
                        screen.running(config, pass, percent, overall, current);
                    }
                    true
                });
                if let Some(next) = changed {
                    config = next;
                    drop(ram);
                    continue 'restart;
                }
                if exit {
                    record.status = memory_result::CANCELLED;
                    record.errors = observed.errors;
                    if let Some(f) = observed.first {
                        record.address = f.address;
                        record.expected = f.expected;
                        record.actual = f.actual;
                    }
                    save(record);
                    marker("[MEMORY] cancelled\r\n");
                    return;
                }
                if let Ok(test_report) = outcome {
                    report.errors = report.errors.saturating_add(test_report.errors);
                    report.first = report.first.or(test_report.first);
                    if let Some(f) = test_report.first {
                        marker(&format!("[MEMORY] fault address={:#x} expected={:#x} actual={:#x} errors={}\r\n", f.address, f.expected, f.actual, test_report.errors));
                    }
                }
            }
            record.passes = pass;
            record.tested = bytes;
            if config.passes != 0 && pass >= config.passes {
                break;
            }
        }
        record.errors = report.errors;
        if let Some(f) = report.first {
            record.address = f.address;
            record.expected = f.expected;
            record.actual = f.actual;
        }
        record.status = if report.errors == 0 {
            memory_result::PASSED
        } else {
            memory_result::FAILED
        };
        screen.running(config, pass, 100, 100, report);
        drop(ram);
        marker(&format!(
            "[MEMORY] completed bytes={} passes={} errors={}\r\n",
            record.tested, record.passes, record.errors
        ));
        if !save(record) {
            screen.notice("Results could not be saved. Press ESC to return to the boot menu.");
            while !matches!(read(&mut input), Some(Key::Special(ScanCode::ESCAPE))) {
                stall();
            }
            return;
        }
        uefi::boot::stall(core::time::Duration::from_secs(2));
        marker("[MEMORY] restarting\r\n");
        uefi::runtime::reset(uefi::runtime::ResetType::COLD, uefi::Status::SUCCESS, None);
    }
}
