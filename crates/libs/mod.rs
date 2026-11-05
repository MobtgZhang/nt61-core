#[path = "advapi32/src/advapi32/mod.rs"]
pub mod advapi32;
#[path = "../userland/cmd/src/cmd/mod.rs"]
pub mod cmd;
#[path = "gdi32/src/gdi32/mod.rs"]
pub mod gdi32;
#[path = "kernel32/src/kernel32/mod.rs"]
pub mod kernel32;
#[path = "msvcrt/src/msvcrt/mod.rs"]
pub mod msvcrt;
#[path = "ntdll/src/ntdll/mod.rs"]
pub mod ntdll;
#[path = "rpcrt4/src/rpcrt4/mod.rs"]
pub mod rpcrt4;
#[path = "sechost/src/sechost/mod.rs"]
pub mod sechost;
#[path = "../system/common/src/server/mod.rs"]
pub mod server;
#[path = "../system/common/src/libs_smoke.rs"]
pub mod smoke;
#[path = "user32/src/user32/mod.rs"]
pub mod user32;
#[path = "../subsystems/win32k/src/win32k/mod.rs"]
pub mod win32k;
#[path = "winmm/src/winmm/mod.rs"]
pub mod winmm;
#[path = "../subsystems/wow64/src/wow64/mod.rs"]
pub mod wow64;
#[path = "../subsystems/wow64win/src/wow64win/mod.rs"]
pub mod wow64win;
#[path = "ws2_32/src/ws2_32/mod.rs"]
pub mod ws2_32;

pub fn init() {
    ntdll::init();
    kernel32::init();
    advapi32::init();
    msvcrt::init();
    rpcrt4::init();
    user32::init();
    gdi32::init();
    win32k::init();
    wow64::init();
    wow64win::init();
    sechost::init();
    server::init();
}

pub fn smoke_test() -> bool {
    smoke::smoke_test()
}
