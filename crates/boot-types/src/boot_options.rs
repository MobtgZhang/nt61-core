//! Boot switches shared by bootmgr, winload and the kernel.
use crate::boot_types::BootMode;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BootOptions(pub u32);
impl BootOptions {
    pub const SAFE: u32 = 1;
    pub const NETWORK: u32 = 1 << 1;
    pub const SAFE_SHELL: u32 = 1 << 2;
    pub const DEBUG: u32 = 1 << 3;
    pub const BOOT_LOG: u32 = 1 << 4;
    pub const LOW_RES: u32 = 1 << 5;
    pub const LAST_GOOD: u32 = 1 << 6;
    pub const NO_RESTART: u32 = 1 << 7;
    pub const NO_SIGNATURE: u32 = 1 << 8;
    pub const DIRECTORY_REPAIR: u32 = 1 << 9;

    pub fn parse(text: &str) -> Self {
        let mut flags = 0;
        for token in text.split_ascii_whitespace() {
            if token.eq_ignore_ascii_case("/safeboot:minimal") {
                flags |= Self::SAFE;
            } else if token.eq_ignore_ascii_case("/safeboot:network") {
                flags |= Self::SAFE | Self::NETWORK;
            } else if token.eq_ignore_ascii_case("/safeboot:shell") {
                flags |= Self::SAFE | Self::SAFE_SHELL;
            } else if token.eq_ignore_ascii_case("/safeboot:dsrepair") {
                flags |= Self::SAFE | Self::DIRECTORY_REPAIR;
            } else if token.eq_ignore_ascii_case("/debug") {
                flags |= Self::DEBUG;
            } else if token.eq_ignore_ascii_case("/bootlog") {
                flags |= Self::BOOT_LOG;
            } else if token.eq_ignore_ascii_case("/lowres")
                || token.eq_ignore_ascii_case("/basevideo")
            {
                flags |= Self::LOW_RES;
            } else if token.eq_ignore_ascii_case("/lastknowngood") {
                flags |= Self::LAST_GOOD;
            } else if token.eq_ignore_ascii_case("/norebootoncrash") {
                flags |= Self::NO_RESTART;
            } else if token.eq_ignore_ascii_case("/disableintegritychecks") {
                flags |= Self::NO_SIGNATURE;
            }
        }
        Self(flags)
    }
    pub const fn has(self, flag: u32) -> bool {
        self.0 & flag != 0
    }
    pub const fn network_enabled(self) -> bool {
        !self.has(Self::SAFE) || self.has(Self::NETWORK)
    }
    /// F8 overrides CrashControl\\AutoReboot for this boot only.
    pub const fn automatic_restart(self, configured: bool) -> bool {
        configured && !self.has(Self::NO_RESTART)
    }
    pub fn mode(self, guid: &[u8; 16]) -> BootMode {
        if self.has(Self::SAFE) {
            return BootMode::SafeMode;
        }
        // Debugging the normal desktop does not select the separate Debug CMD shell.
        match *guid {
            [0xb2, 0x72, 0x1d, 0x66, 0x7d, 0xbf, 0x4e, 0x50, 0xae, 0x7c, 0xd2, 0x7f, 0x2d, 0x90, 0xce, 0x20] => {
                BootMode::Cmd
            }
            [0x51, 0x89, 0xb2, 0x5c, 0x55, 0x58, 0x4b, 0xf2, 0xbb, 0x0f, 0xcd, 0x5a, 0x4f, 0x8c, 0x7e, 0x20] => {
                BootMode::DebugCmd
            }
            [0xa3, 0x4c, 0x8e, 0xd1, 0x7b, 0xf2, 0x4a, 0x93, 0xbc, 0x1d, 0x6e, 0x5a, 0x3f, 0x8b, 0x7c, 0x20] => {
                BootMode::SafeMode
            }
            _ => BootMode::Desktop,
        }
    }
}

static ACTIVE: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
/// Install once on the BSP before starting drivers or secondary CPUs.
pub fn install(options: BootOptions) {
    ACTIVE.store(options.0, core::sync::atomic::Ordering::Release);
}
pub fn active() -> BootOptions {
    BootOptions(ACTIVE.load(core::sync::atomic::Ordering::Acquire))
}
