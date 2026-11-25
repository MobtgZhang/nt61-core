use nt61_tools::fs::pe::{emit, ImportSymbol, PeImageSpec, SectionSpec};
use serde_json::Value;
use std::io::Write;
use std::process::{Command, Stdio};

const PROBE: &str = r"C:\muiprobe.exe";
const BOOT: &str = concat!(
    "[BOOTMGR] framebuffer mailbox: page=0x1000\r\n",
    "[WINLOAD] os_loader_run entered\n",
    "[KERNEL] boot mode start-session-manager\n",
    "[EXPLORER] desktop frame ready\n",
);

fn inspect(serial: &[u8], options: &[&str]) -> (i32, Value) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_nt61-debug"))
        .args(["-", "--json"])
        .args(options)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(serial).unwrap();
    let output = child.wait_with_output().unwrap();
    let report = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|_| panic!("{}", String::from_utf8_lossy(&output.stderr)));
    (output.status.code().unwrap(), report)
}

#[test]
fn accepts_complete_project_and_guest_records_interrupted_by_scheduler() {
    let serial = format!(
        "{BOOT}[PE] Loading {PROBE}\r\n[PE] Running {PROBE}\r\n\
         [PE] Translation {PROBE}: native=153 interpreted=279\r\n\
         [PE] Guest exited {PROBE}: [SCHED-SWITCH] n=0x0000000000000055 cur=0x000000021C70F360 \
         next=0x0000000000000005 nrsp=0x0000000000000030\r\n0\r\n"
    );
    let (status, report) = inspect(
        serial.as_bytes(),
        &[
            "--require-boot",
            "--require-desktop",
            "--require-exit",
            "c:/MUIPROBE.EXE",
            "--require-jit",
            PROBE,
            "--deny-panic",
        ],
    );
    assert_eq!(status, 0, "{report}");
    assert_eq!(report["scheduler_records_removed"], 1);
    assert_eq!(report["events"][2]["native"], 153);
    assert_eq!(report["events"][3]["line"], 8);
}

#[test]
fn reports_loader_and_runtime_faults_even_with_corrupted_guest_path() {
    let serial = concat!(
        "[PE] Cannot execute C:\\notepad32.exe: guest DLL user32.dll has no export Name(\"LoadImageW\"); NTSTATUS 0xC0000139.\n",
        "[PE] Guest execution fault in \0\0\x0b: InstructionFault { pc: 42 }; API user32.dll!EnableMenuItem; NTSTATUS 0xC0000005.\n",
        "[PE] Fault details: retired=156 bytes=[]\n",
        "[PE] Delay-load fault in C:\\test.exe: test.dll!#12\n",
        "KERNEL PANIC: panicked at crates/ntoskrnl/src/mm/jit_code.rs:84:9:\n",
    );
    let (status, report) = inspect(serial.as_bytes(), &["--deny-panic"]);
    assert_eq!(status, 1);
    assert_eq!(report["events"][0]["ntstatus"], 0xc0000139u32);
    assert!(report["events"][0]["detail"]
        .as_str()
        .unwrap()
        .contains("LoadImageW"));
    assert_eq!(report["events"][1]["ntstatus"], 0xc0000005u32);
    assert!(report["events"][1].get("guest").is_none());
    assert_eq!(report["events"][2]["kind"], "fault_details");
    assert_eq!(report["events"][3]["kind"], "delay_load_fault");
    assert_eq!(report["events"][3]["guest"], r"C:\test.exe");
    assert_eq!(report["events"][3]["dll"], "test.dll");
    assert_eq!(report["events"][3]["symbol"], "#12");
    assert_eq!(report["events"][4]["kind"], "kernel_panic");
}

#[test]
fn prior_pass_does_not_satisfy_an_incomplete_relaunch() {
    let serial = format!(
        "[PE] Loading {PROBE}\n[PE] Translation {PROBE}: native=1 interpreted=3\n\
         [PE] Guest exited {PROBE}: 0\n[PE] Loading {PROBE}\n\
         [PE] Guest exited {PROBE}: 0"
    );
    let (status, report) = inspect(
        serial.as_bytes(),
        &["--require-exit", PROBE, "--require-jit", PROBE],
    );
    assert_eq!(status, 1);
    assert_eq!(report["incomplete_tail"], true);
    assert_eq!(report["failures"].as_array().unwrap().len(), 2);
}

#[test]
fn manifest_cannot_replace_ordered_serial_evidence() {
    let digest = "a".repeat(64);
    let manifest = tempfile::NamedTempFile::new().unwrap();
    serde_json::to_writer(
        manifest.as_file(),
        &serde_json::json!({
            "path": "/build/disk.img", "boot_chain_verified": true,
            "source_sha256": digest, "base_sha256": "b".repeat(64),
            "expected_source_sha256": digest, "boot_disk_sha256": digest,
        }),
    )
    .unwrap();
    let (status, report) = inspect(
        b"[WINLOAD] os_loader_run entered\n[BOOTMGR] framebuffer mailbox:\n[KERNEL] boot mode start-session-manager\n",
        &["--require-boot", "--project-image", manifest.path().to_str().unwrap()],
    );
    assert_eq!(status, 1);
    assert_eq!(report["boot_chain_observed"], false);
    assert_eq!(report["failures"].as_array().unwrap().len(), 3);
}

#[test]
fn follows_ordinal_forwarders_and_rejects_holes_and_cycles() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path();
    let mut application = PeImageSpec::new(0x140000000, 3, false, 0x8664);
    application.sections.push(SectionSpec::rdata(Vec::new()));
    application.import(
        "SOURCE.DLL",
        vec![
            ImportSymbol::ByOrdinal(4),
            ImportSymbol::ByOrdinal(3),
            ImportSymbol::ByOrdinal(6),
        ],
    );
    std::fs::write(path.join("guest.exe"), emit(&application).0).unwrap();

    let mut source = PeImageSpec::new(0x180000000, 3, true, 0x8664);
    source.sections.push(SectionSpec::code(vec![0xc3]));
    source.sections.push(SectionSpec::rdata(Vec::new()));
    source.export_forwarder("Indirect", 4, "Target.drv.#7");
    source.export_forwarder("Cycle", 6, "Source.#6");
    source.export_ordinal(2, 0x1000);
    std::fs::write(path.join("source.dll"), emit(&source).0).unwrap();

    let mut target = PeImageSpec::new(0x190000000, 3, true, 0x8664);
    target.sections.push(SectionSpec::code(vec![0xc3]));
    target.sections.push(SectionSpec::rdata(Vec::new()));
    target.export_ordinal(7, 0x1000);
    std::fs::write(path.join("target.drv"), emit(&target).0).unwrap();

    let scan = || {
        Command::new(env!("CARGO_BIN_EXE_nt61-debug"))
            .args(["--json", "--pe-imports"])
            .arg(path.join("guest.exe"))
            .arg("--runtime-dir")
            .arg(path)
            .output()
            .unwrap()
    };
    let output = scan();
    assert_eq!(output.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    let failures = report["failures"].as_array().unwrap();
    assert_eq!(failures.len(), 2);
    assert_eq!(failures[0]["symbol"], "#3");
    assert_eq!(failures[0]["problem"], "missing_export");
    assert_eq!(failures[1]["symbol"], "#6");
    assert_eq!(failures[1]["problem"], "forwarder_cycle");

    std::fs::remove_file(path.join("target.drv")).unwrap();
    let output = scan();
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["failures"][0]["symbol"], "#4");
    assert_eq!(report["failures"][0]["problem"], "missing_module");
    assert_eq!(report["failures"][0]["forwarders"][0], "Target.drv.#7");
}

#[test]
fn resolves_adjacent_application_libraries_before_the_runtime_directory() {
    let directory = tempfile::tempdir().unwrap();
    let runtime = directory.path().join("runtime");
    std::fs::create_dir(&runtime).unwrap();
    let guest = directory.path().join("guest.exe");
    let mut application = PeImageSpec::new(0x400000, 3, false, 0x14c);
    application.sections.push(SectionSpec::rdata(Vec::new()));
    application.import("private.dll", vec![ImportSymbol::ByOrdinal(7)]);
    std::fs::write(&guest, emit(&application).0).unwrap();

    let mut library = PeImageSpec::new(0x10000000, 3, true, 0x14c);
    library.sections.push(SectionSpec::code(vec![0xc3]));
    library.sections.push(SectionSpec::rdata(Vec::new()));
    library.export_forwarder("Entry", 7, "Runtime.#5");
    std::fs::write(directory.path().join("Private.DLL"), emit(&library).0).unwrap();
    std::fs::write(runtime.join("private.dll"), b"invalid PE").unwrap();

    let mut fallback = PeImageSpec::new(0x20000000, 3, true, 0x14c);
    fallback.sections.push(SectionSpec::code(vec![0xc3]));
    fallback.sections.push(SectionSpec::rdata(Vec::new()));
    fallback.export_ordinal(5, 0x1000);
    std::fs::write(runtime.join("runtime.dll"), emit(&fallback).0).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_nt61-debug"))
        .args(["--json", "--pe-imports"])
        .arg(guest)
        .arg("--runtime-dir")
        .arg(runtime)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
}
