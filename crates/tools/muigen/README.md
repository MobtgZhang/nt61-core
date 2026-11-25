# muigen

`muigen` uses the Microsoft resource tools to create Win32 language-neutral modules and their `.mui` satellites. Build the CLI with `cargo build -p muigen --release`. Run it on Windows with the relevant Microsoft tools on `PATH`, or pass their executable paths explicitly.

## Split a compiled module

```powershell
muigen split --input "build\My App.dll" --config "resources.rcconfig" --lang zh-CN --out dist
```

`muirct.exe` writes `dist\My App.dll` and `dist\zh-CN\My App.dll.mui`. Use `--muirct PATH` to select a specific executable. The XML configuration defines which resources are localized. Its optional `language` attribute must match `--lang`.

## Package multiple editions

```powershell
muigen batch --input "zh\My App.dll" --config zh.rcconfig --lang zh-CN `
    --input "en\My App.dll" --config en.rcconfig --lang en-US --out dist
```

Use the same module filename in each edition. The first edition supplies the neutral module and its `.rcconfig` must declare that edition's language as its external ultimate fallback. `muirct.exe` synchronizes each later satellite's checksums with the neutral module. Deploy both language directories alongside the main DLL.

## Build from a resource script

```powershell
muigen build --input app.rc --config resources.rcconfig --module "My App.dll" `
    --lang zh-CN --machine x64 --out dist
```

`rc.exe` compiles the resource script and `link.exe` produces the satellite. Link `dist\My App.dll.res` into the application as part of its normal link step and keep `dist\zh-CN\My App.dll.mui` beside the resulting main module in the `zh-CN` subdirectory. Use `--rc PATH` and `--link PATH` to select the tools. `--machine` accepts `x86`, `x64` and `arm64` with a linker that supports the chosen target.

All commands accept `zh-CN` and `en-US`. Choose a new output directory. The CLI publishes it after every expected file is present and forwards Microsoft tool diagnostics and exit codes on failure.

## Verify on Windows

From an x64 Visual Studio Developer PowerShell with the Windows SDK and `muirct.exe` available, run the fixture. It calls `LoadStringW` on the generated main DLL and checks exact text for the split, batch and build packages. It also checks that removing the split satellite makes its string unavailable.

```powershell
& .\crates\tools\muigen\fixtures\verify.ps1 `
    -Muigen .\target\release\muigen.exe -Muirct muirct.exe
```

## Verify on the NT61 project image

Pass `-ProjectFixtures <new-directory>` to `verify.ps1` to export its SDK-generated x64 main modules and satellites. Use the same SDK toolchain as the Windows fixture. Transfer the resulting directory to the host that runs QEMU.

Build the CRT-free probe with an x64 MinGW-w64 compiler. Run it against the directory containing `disk.img` from the NT61 x86-64 NTFS build.

```bash
x86_64-w64-mingw32-gcc -O1 -fno-stack-protector -nostdlib \
  -Wl,--entry=mui_probe -Wl,--subsystem=console \
  crates/tools/muigen/fixtures/verify-project.c -o /tmp/muiprobe.exe -lkernel32 -luser32
python3 scripts/test-original-notepad.py --mui-only \
  --images-dir /path/to/project/images \
  --sdk-mui-fixtures /path/to/project-fixtures --sdk-mui-probe /tmp/muiprobe.exe \
  --output /tmp/nt61-mui-test
```

The runner copies the project disk and verifies its SHA-256 before injecting the fixtures. Pass `--expected-image-sha256` with the digest from the project build to pin the input. It checks the project Bootmgr, Winload and kernel markers before launching the probe from the project desktop. `x86_64/mui-runtime.json` records the result and the digest of the disk passed to QEMU after injection. Exit `0` confirms the missing-satellite control, Chinese and English on the same main module, and the Chinese satellite produced by `build`. `--kvm` selects hardware acceleration when available. Set `TMPDIR` to a directory with space for the disposable copy and a short path for the QMP socket if the default temporary volume is too small.
