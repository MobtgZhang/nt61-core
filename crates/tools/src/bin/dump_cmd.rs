//! Dump the cmd.exe image produced by the build pipeline to disk.
//!
//! The in-binary `system_image` pipeline is gone: cmd.exe is now
//! produced from the freshly built `nt61-cmd.efi` artifact by
//! `nt61_tools::fs::build`. This tool extracts that image so it can
//! be embedded or inspected.
use std::fs;
use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let machine: u16 = if args.len() > 1 {
        u16::from_str_radix(args[1].trim_start_matches("0x"), 16).unwrap_or(0x8664)
    } else {
        0x8664
    };
    let out_path = if args.len() > 2 {
        PathBuf::from(&args[2])
    } else {
        PathBuf::from("src/resources/pe/cmd_x86_64.exe")
    };
    let arch = match machine {
        0xAA64 => "aarch64",
        0x5064 => "riscv64",
        0x6264 => "loongarch64",
        _ => "x86_64",
    };
    nt61_tools::fs::build::set_pe_arch(arch);
    let images = nt61_tools::fs::build::build_system_images();
    let img = images
        .iter()
        .find(|i| i.name == "cmd.exe")
        .expect("cmd.exe missing from build_system_images (build nt61-cmd first)");
    let bytes: &[u8] = &img.bytes;
    eprintln!("dump_cmd: produced {} bytes", bytes.len());
    eprintln!(
        "dump_cmd: first 32 bytes = {:02x?}",
        &bytes[..32.min(bytes.len())]
    );
    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent).expect("mkdir parent");
    }
    fs::write(&out_path, bytes).expect("write cmd.exe");
    println!("Wrote {} bytes to {}", bytes.len(), out_path.display());
}
