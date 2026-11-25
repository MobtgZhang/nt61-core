//! Windows default sound scheme. Decode the installed PCM WAV, then submit
//! actual samples to the detected output device. Missing assets are errors.
extern crate alloc;
use alloc::vec::Vec;

pub const EVENTS: &[(&str, &str)] = &[
    ("startup", "Windows Startup.wav"),
    ("shutdown", "Windows Shutdown.wav"),
    ("logon", "Windows Logon Sound.wav"),
    ("logoff", "Windows Logoff Sound.wav"),
    ("default", "Windows Default.wav"),
    ("error", "Windows Critical Stop.wav"),
    ("exclamation", "Windows Exclamation.wav"),
    ("information", "Windows Ding.wav"),
    ("notify", "Windows Notify.wav"),
    ("device-connect", "Windows Hardware Insert.wav"),
    ("device-disconnect", "Windows Hardware Remove.wav"),
    ("device-fail", "Windows Hardware Fail.wav"),
    ("navigate", "Windows Navigation Start.wav"),
    ("recycle", "Windows Recycle.wav"),
    ("print", "Windows Print complete.wav"),
    ("uac", "Windows User Account Control.wav"),
];

/// Strict, bounded RIFF PCM reader. Unknown padded chunks are skipped.
/// Output is always interleaved 48 kHz stereo signed PCM16.
pub fn decode(data: &[u8]) -> Option<Vec<i16>> {
    let u16_at = |s: &[u8], i| Some(u16::from_le_bytes(s.get(i..i + 2)?.try_into().ok()?));
    let u32_at = |s: &[u8], i| Some(u32::from_le_bytes(s.get(i..i + 4)?.try_into().ok()?));
    if data.get(..4)? != b"RIFF" || data.get(8..12)? != b"WAVE" {
        return None;
    }
    let end = (u32_at(data, 4)? as usize).checked_add(8)?;
    if end > data.len() || end < 12 {
        return None;
    }
    let (mut format, mut pcm) = (None, None);
    let mut cursor = 12usize;
    while cursor < end {
        let header = data.get(cursor..cursor.checked_add(8)?)?;
        let length = u32_at(header, 4)? as usize;
        let start = cursor + 8;
        let next = start.checked_add(length)?;
        if next > end {
            return None;
        }
        let chunk = &data[start..next];
        match &header[..4] {
            b"fmt " => {
                if format.is_some() || chunk.len() < 16 || u16_at(chunk, 0)? != 1 {
                    return None;
                }
                let channels = u16_at(chunk, 2)?;
                let rate = u32_at(chunk, 4)?;
                let bits = u16_at(chunk, 14)?;
                let align = u16_at(chunk, 12)?;
                if !(channels == 1 || channels == 2)
                    || !(8000..=96000).contains(&rate)
                    || !(bits == 8 || bits == 16)
                    || align != channels * (bits / 8)
                    || u32_at(chunk, 8)? != rate.checked_mul(u32::from(align))?
                {
                    return None;
                }
                format = Some((channels as usize, rate, bits, align as usize));
            }
            b"data" => {
                if pcm.is_some() {
                    return None;
                }
                pcm = Some(chunk);
            }
            _ => {}
        }
        cursor = next.checked_add(length & 1)?;
        if cursor > end {
            return None;
        }
    }
    let (channels, rate, bits, align) = format?;
    let pcm = pcm?;
    if pcm.is_empty() || pcm.len() % align != 0 {
        return None;
    }
    let frames = pcm.len() / align;
    if frames > rate as usize * 10 {
        return None;
    }
    let output_frames = frames.checked_mul(48000)? / rate as usize;
    let mut output = Vec::with_capacity(output_frames * 2);
    let sample = |frame: usize, channel: usize| -> i32 {
        let offset = frame * align + channel.min(channels - 1) * (bits as usize / 8);
        if bits == 8 {
            (i32::from(pcm[offset]) - 128) * 256
        } else {
            i32::from(i16::from_le_bytes([pcm[offset], pcm[offset + 1]]))
        }
    };
    for frame in 0..output_frames {
        let position = frame as u64 * u64::from(rate);
        let index = (position / 48000) as usize;
        let fraction = (position % 48000) as i64;
        for channel in 0..2 {
            let a = sample(index, channel);
            let b = sample((index + 1).min(frames - 1), channel);
            output.push((i64::from(a) + (i64::from(b - a) * fraction) / 48000) as i16);
        }
    }
    Some(output)
}

/// Milliseconds accepted by the hardware queue, for orderly shutdown.
pub fn play(event: &str) -> Result<u64, u32> {
    let file = EVENTS
        .iter()
        .find(|(id, _)| *id == event)
        .ok_or(0xC000000Du32)?
        .1;
    if event == "startup" {
        load_startup();
    }
    let config = configuration();
    if (event == "startup" && !config.startup) || (event != "startup" && !config.enabled) {
        return Ok(0);
    }
    let data = crate::fs::system_volume::read(&alloc::format!("C:\\Windows\\Media\\{file}"))?;
    let pcm = decode(&data).ok_or(0xC000000Du32)?;
    if !super::hda_pcm::ready() {
        return Err(0xC00000BB);
    }
    if super::hda_pcm::submit(&pcm) != pcm.len() {
        return Err(0xC00000BB);
    }
    Ok(pcm.len() as u64 * 1000 / 96000)
}

pub fn message_beep(kind: u32) -> bool {
    let event = match kind {
        u32::MAX => None,
        0 | 0x20 => Some("default"),
        0x10 => Some("error"),
        0x30 => Some("exclamation"),
        0x40 => Some("information"),
        _ => return false,
    };
    if !configuration().enabled {
        return true;
    }
    if event.is_some_and(|event| play(event).is_ok() || (event != "default" && play("default").is_ok())) {
        return true;
    }
    if !super::hda_pcm::ready() {
        return false;
    }
    let mut tone = Vec::with_capacity(4800 * 2);
    for frame in 0..4800 {
        let amplitude = ((4800 - frame) * 2048 / 4800) as i16;
        let sample = if frame * 800 % 48000 < 24000 { amplitude } else { -amplitude };
        tone.extend_from_slice(&[sample, sample]);
    }
    super::hda_pcm::submit(&tone) == tone.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn wave() -> Vec<u8> {
        let mut b = b"RIFF".to_vec();
        b.extend_from_slice(&40u32.to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        for v in [1u16, 1] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b.extend_from_slice(&48000u32.to_le_bytes());
        b.extend_from_slice(&96000u32.to_le_bytes());
        for v in [2u16, 16] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b.extend_from_slice(b"data");
        b.extend_from_slice(&4u32.to_le_bytes());
        b.extend_from_slice(&[0, 128, 255, 127]);
        b
    }
    #[test]
    fn mono_pcm_is_duplicated_to_both_channels() {
        assert_eq!(decode(&wave()).unwrap(), [-32768, -32768, 32767, 32767]);
    }
    #[test]
    fn malformed_sizes_formats_and_alignment_are_rejected() {
        let b = wave();
        for n in 0..b.len() {
            assert!(decode(&b[..n]).is_none());
        }
        for (i, value) in [(20, 3), (22, 3), (32, 7), (34, 24), (40, 3)] {
            let mut b = wave();
            b[i] = value;
            assert!(decode(&b).is_none());
        }
    }
    #[test]
    fn configuration_roundtrips_and_rejects_invalid_storage() {
        for volume in [0, 50, 100] {
            for flags in 0..8 {
                let config = Configuration {
                    volume,
                    muted: flags & 1 != 0,
                    enabled: flags & 2 != 0,
                    startup: flags & 4 != 0,
                };
                assert_eq!(
                    Configuration::from_bytes(&config.bytes()).unwrap().bytes(),
                    config.bytes()
                );
            }
        }
        let bytes = Configuration::DEFAULT.bytes();
        for len in 0..bytes.len() {
            assert!(Configuration::from_bytes(&bytes[..len]).is_none());
        }
        for (offset, value) in [(0, 0), (4, 101), (5, 2), (6, 2), (7, 2)] {
            let mut invalid = bytes;
            invalid[offset] = value;
            assert!(Configuration::from_bytes(&invalid).is_none());
        }
    }
}

#[derive(Clone, Copy)]
pub struct Configuration {
    pub volume: u8,
    pub muted: bool,
    pub enabled: bool,
    pub startup: bool,
}
impl Configuration {
    pub const DEFAULT: Self = Self {
        volume: 75,
        muted: false,
        enabled: true,
        startup: true,
    };
    pub fn bytes(self) -> [u8; 8] {
        [
            b'S',
            b'N',
            b'D',
            b'1',
            self.volume,
            u8::from(self.muted),
            u8::from(self.enabled),
            u8::from(self.startup),
        ]
    }
    pub fn from_bytes(b: &[u8]) -> Option<Self> {
        if b.len() != 8 || &b[..4] != b"SND1" || b[4] > 100 || b[5..].iter().any(|v| *v > 1) {
            return None;
        }
        Some(Self {
            volume: b[4],
            muted: b[5] != 0,
            enabled: b[6] != 0,
            startup: b[7] != 0,
        })
    }
}
static CONFIG: crate::ke::sync::Spinlock<Configuration> =
    crate::ke::sync::Spinlock::new(Configuration::DEFAULT);
pub fn configuration() -> Configuration {
    *CONFIG.lock()
}
fn apply(config: Configuration) {
    *CONFIG.lock() = config;
    super::playback::set_master_volume(config.volume);
    super::playback::set_master_mute(config.muted);
}
fn settings_path(user: &str) -> alloc::string::String {
    alloc::format!("C:\\Users\\{user}\\AppData\\Roaming\\Microsoft\\Windows\\Themes\\NT61.soundcfg")
}
pub fn load_for(user: &str) {
    let mut config = crate::fs::system_volume::read(&settings_path(user))
        .ok()
        .and_then(|b| Configuration::from_bytes(&b))
        .unwrap_or(Configuration::DEFAULT);
    config.startup = startup_enabled();
    apply(config);
}
// Startup sound is a machine setting: LogonUI plays it before any user logs
// on. This private storage is used until persistent AppEvents registry writes
// are supported; it must never depend on the last authenticated profile.
const STARTUP_PATH: &str = "C:\\Windows\\System32\\config\\startup-sound.bin";
fn startup_enabled() -> bool {
    crate::fs::system_volume::read(STARTUP_PATH)
        .ok()
        .filter(|b| b.len() == 5 && &b[..4] == b"SST1" && b[4] <= 1)
        .map_or(true, |b| b[4] != 0)
}
pub fn load_startup() {
    CONFIG.lock().startup = startup_enabled();
}
pub fn save_startup(enabled: bool) -> Result<(), u32> {
    let mut bytes = *b"SST1\0";
    bytes[4] = u8::from(enabled);
    crate::fs::system_volume::write(STARTUP_PATH, &bytes)?;
    crate::fs::system_volume::flush()?;
    CONFIG.lock().startup = enabled;
    Ok(())
}
pub fn save_for(user: &str, config: Configuration) -> Result<(), u32> {
    if !crate::se::users::valid_account_name(user) {
        return Err(0xC000000D);
    }
    for suffix in [
        "AppData",
        "AppData\\Roaming",
        "AppData\\Roaming\\Microsoft",
        "AppData\\Roaming\\Microsoft\\Windows",
        "AppData\\Roaming\\Microsoft\\Windows\\Themes",
    ] {
        let path = alloc::format!("C:\\Users\\{user}\\{suffix}");
        if crate::fs::system_volume::list(&path).is_err() {
            crate::fs::system_volume::mkdir(&path)?;
        }
    }
    crate::fs::system_volume::write(&settings_path(user), &config.bytes())?;
    crate::fs::system_volume::flush()?;
    apply(config);
    Ok(())
}
