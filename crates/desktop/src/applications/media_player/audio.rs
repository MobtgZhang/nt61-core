//! Bounded pure-Rust WAV/MP3 decoder. Compressed input is shared, PCM is streamed.
use alloc::{boxed::Box, collections::VecDeque, rc::Rc, vec::Vec};
use nanomp3::{Decoder, MAX_SAMPLES_PER_FRAME};

#[derive(Clone, Debug, PartialEq)]
pub struct Wave {
    pub start: usize,
    pub length: usize,
    pub rate: u32,
    pub channels: usize,
    pub bits: usize,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub offset: usize,
    pub sample: u64,
    pub count: usize,
}
#[derive(Clone)]
pub struct Stream {
    bytes: Rc<Vec<u8>>,
    wave: Option<Wave>,
    frames: Vec<Frame>,
    decoder: Box<Decoder>,
    offset: usize,
    decoded: u64,
    skip: u64,
    pending: VecDeque<i16>,
    pub position: u64,
    pub length: u64,
    pub rate: u32,
}
impl core::fmt::Debug for Stream {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Stream")
            .field("bytes", &self.bytes.len())
            .field("position", &self.position)
            .field("length", &self.length)
            .finish()
    }
}
impl PartialEq for Stream {
    fn eq(&self, o: &Self) -> bool {
        Rc::ptr_eq(&self.bytes, &o.bytes)
            && self.position == o.position
            && self.offset == o.offset
            && self.skip == o.skip
            && self.pending == o.pending
    }
}
fn u16_at(b: &[u8], n: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(n..n + 2)?.try_into().ok()?))
}
fn u32_at(b: &[u8], n: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(n..n + 4)?.try_into().ok()?))
}
impl Stream {
    pub fn open(bytes: Vec<u8>) -> Result<Self, &'static str> {
        if bytes.len() > 32 * 1024 * 1024 {
            return Err("This media file is too large.");
        }
        let mut wave = None;
        let mut frames = Vec::new();
        let rate;
        let length;
        if bytes.starts_with(b"RIFF") {
            if bytes.get(8..12) != Some(b"WAVE") {
                return Err("Unsupported RIFF media format.");
            }
            let end = usize::try_from(u32_at(&bytes, 4).ok_or("Invalid WAV header.")?)
                .unwrap()
                .checked_add(8)
                .ok_or("Invalid WAV size.")?;
            if end > bytes.len() || end < 12 {
                return Err("Truncated WAV file.");
            }
            let mut pos = 12;
            let mut fmt = None;
            let mut data = None;
            while pos + 8 <= end {
                let size = u32_at(&bytes, pos + 4).ok_or("Invalid WAV chunk.")? as usize;
                let start = pos + 8;
                let next = start
                    .checked_add(size)
                    .filter(|n| *n <= end)
                    .ok_or("Truncated WAV chunk.")?;
                match &bytes[pos..pos + 4] {
                    b"fmt " if size >= 16 => {
                        fmt = Some((
                            u16_at(&bytes, start).unwrap(),
                            u16_at(&bytes, start + 2).unwrap(),
                            u32_at(&bytes, start + 4).unwrap(),
                            u16_at(&bytes, start + 12).unwrap(),
                            u16_at(&bytes, start + 14).unwrap(),
                        ));
                    }
                    b"data" if data.is_none() => data = Some((start, size)),
                    _ => {}
                }
                pos = next + (size & 1);
            }
            let (kind, ch, hz, align, bits) = fmt.ok_or("Missing WAV format.")?;
            if kind != 1
                || !(ch == 1 || ch == 2)
                || !(bits == 8 || bits == 16)
                || !(8000..=192000).contains(&hz)
                || align != ch * bits / 8
            {
                return Err("Windows Media Player cannot play this WAV encoding. PCM 8/16 bit is supported.");
            }
            let (start, size) = data.ok_or("Missing WAV audio.")?;
            if size == 0 || size % align as usize != 0 {
                return Err("Invalid WAV audio length.");
            }
            rate = hz;
            length = (size / align as usize) as u64 * 48000 / u64::from(hz);
            wave = Some(Wave {
                start,
                length: size,
                rate: hz,
                channels: ch as usize,
                bits: bits as usize,
            });
        } else {
            let mut samples = 0u64;
            let mut first = None;
            for frame in nanomp3::frames(&bytes) {
                if frame.layer != 3 {
                    return Err("Unsupported MPEG audio layer.");
                }
                if first.is_some_and(|r| r != frame.sample_rate) {
                    return Err("Changing MP3 sample rates are not supported.");
                }
                first = Some(frame.sample_rate);
                frames.push(Frame {
                    offset: frame.offset,
                    sample: samples,
                    count: frame.samples,
                });
                samples += frame.samples as u64;
                if frames.len() > 500_000 {
                    return Err("Too many MPEG frames.");
                }
            }
            rate=first.ok_or("Windows Media Player cannot play this file. The file format is not supported or the file is damaged.")?;
            length = samples * 48000 / u64::from(rate);
        }
        Ok(Self {
            bytes: Rc::new(bytes),
            wave,
            frames,
            decoder: Box::new(Decoder::new()),
            offset: 0,
            decoded: 0,
            skip: 0,
            pending: VecDeque::new(),
            position: 0,
            length,
            rate,
        })
    }
    pub fn duration_ms(&self) -> u64 {
        self.length * 1000 / 48000
    }
    pub fn seek(&mut self, frame: u64) {
        self.position = frame.min(self.length);
        self.pending.clear();
        self.decoder = Box::new(Decoder::new());
        if self.wave.is_some() {
            return;
        }
        let target = self.position * u64::from(self.rate) / 48000;
        let i = self
            .frames
            .partition_point(|f| f.sample <= target)
            .saturating_sub(1)
            .saturating_sub(10);
        if let Some(f) = self.frames.get(i) {
            self.offset = f.offset;
            self.decoded = f.sample;
            self.skip = target.saturating_sub(f.sample);
        } else {
            self.offset = self.bytes.len();
        }
    }
    fn wave_sample(&self, w: &Wave, frame: u64, ch: usize) -> i32 {
        let frames = w.length / (w.channels * w.bits / 8);
        let frame = (frame as usize).min(frames - 1);
        let offset = w.start + (frame * w.channels + ch.min(w.channels - 1)) * w.bits / 8;
        if w.bits == 8 {
            (i32::from(self.bytes[offset]) - 128) * 256
        } else {
            i32::from(i16::from_le_bytes([
                self.bytes[offset],
                self.bytes[offset + 1],
            ]))
        }
    }
    pub fn read(&mut self, max_frames: usize) -> Vec<i16> {
        let count = max_frames
            .min(4800)
            .min(self.length.saturating_sub(self.position) as usize);
        let mut out = Vec::with_capacity(count * 2);
        if let Some(w) = &self.wave {
            for i in 0..count {
                let at = (self.position + i as u64) * u64::from(w.rate);
                let frame = at / 48000;
                let mix = (at % 48000) as i64;
                for ch in 0..2 {
                    let a = i64::from(self.wave_sample(w, frame, ch));
                    let b = i64::from(self.wave_sample(w, frame + 1, ch));
                    out.push((a + (b - a) * mix / 48000) as i16);
                }
            }
        } else {
            let mut pcm = [0i16; MAX_SAMPLES_PER_FRAME];
            while self.pending.len() < count * 2 && self.offset < self.bytes.len() {
                let (used, result) = self.decoder.decode(&self.bytes[self.offset..], &mut pcm);
                if used == 0 {
                    break;
                }
                self.offset += used;
                let info = match result {
                    Ok(info) => info,
                    Err(_) => {
                        // A missing bit reservoir after seek still occupies its original timeline.
                        if let Some(frame) =
                            self.frames.iter().rev().find(|f| f.offset < self.offset)
                        {
                            let n = frame.count as u64;
                            let skip = self.skip.min(n);
                            self.skip -= skip;
                            let output = ((self.decoded + n) * 48000 / u64::from(self.rate))
                                .saturating_sub(
                                    (self.decoded + skip) * 48000 / u64::from(self.rate),
                                );
                            self.pending
                                .extend(core::iter::repeat_n(0, output as usize * 2));
                            self.decoded += n;
                        }
                        continue;
                    }
                };
                let n = info.samples_produced as u64;
                let skip = self.skip.min(n);
                self.skip -= skip;
                let start = (self.decoded + skip) * 48000 / u64::from(self.rate);
                let end = (self.decoded + n) * 48000 / u64::from(self.rate);
                for j in start..end {
                    let at = j * u64::from(self.rate);
                    let src = (at / 48000)
                        .saturating_sub(self.decoded)
                        .min(n.saturating_sub(1)) as usize;
                    let mix = (at % 48000) as i64;
                    let next = (src + 1).min(n.saturating_sub(1) as usize);
                    let channels = info.channels.num() as usize;
                    for ch in 0..2 {
                        let ch = ch.min(channels - 1);
                        let a = i64::from(pcm[src * channels + ch]);
                        let b = i64::from(pcm[next * channels + ch]);
                        self.pending.push_back((a + (b - a) * mix / 48000) as i16);
                    }
                }
                self.decoded += n;
            }
            for _ in 0..count * 2 {
                if let Some(s) = self.pending.pop_front() {
                    out.push(s);
                } else {
                    break;
                }
            }
        }
        self.position += out.len() as u64 / 2;
        out
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    pub fn wave() -> Vec<u8> {
        let mut b = b"RIFF".to_vec();
        b.extend(40u32.to_le_bytes());
        b.extend(b"WAVEfmt ");
        b.extend(16u32.to_le_bytes());
        b.extend(1u16.to_le_bytes());
        b.extend(1u16.to_le_bytes());
        b.extend(48000u32.to_le_bytes());
        b.extend(96000u32.to_le_bytes());
        b.extend(2u16.to_le_bytes());
        b.extend(16u16.to_le_bytes());
        b.extend(b"data");
        b.extend(4u32.to_le_bytes());
        b.extend(1234i16.to_le_bytes());
        b.extend((-2345i16).to_le_bytes());
        b
    }
    #[test]
    fn wav_stereo_conversion_and_seek() {
        let mut s = Stream::open(wave()).unwrap();
        assert_eq!(s.read(1), alloc::vec![1234, 1234]);
        assert_eq!(s.read(1), alloc::vec![-2345, -2345]);
        assert!(s.read(1).is_empty());
        s.seek(0);
        assert_eq!(s.read(1), alloc::vec![1234, 1234]);
    }
    #[test]
    fn malformed_wave_never_reaches_output() {
        let b = wave();
        for i in 0..b.len() {
            assert!(Stream::open(b[..i].to_vec()).is_err());
        }
        let mut b = b;
        b[32] = 7;
        assert!(Stream::open(b).is_err());
        assert!(Stream::open(alloc::vec![0;200]).is_err());
    }
    #[test]
    fn generated_mp3_decodes_and_seek_reproduces_the_timeline() {
        let bytes = include_bytes!("../../../../../scripts/fixtures/media/tone-440hz.mp3").to_vec();
        let mut s = Stream::open(bytes).unwrap();
        assert_eq!(s.rate, 48000);
        assert!(s.duration_ms() >= 300);
        let mut pcm = Vec::new();
        loop {
            let next = s.read(4800);
            if next.is_empty() {
                break;
            }
            pcm.extend(next);
        }
        assert!(pcm.iter().filter(|v| v.unsigned_abs() > 100).count() > 10000);
        assert_eq!(pcm.len() as u64 / 2, s.length);
        s.seek(6000);
        assert_eq!(s.read(1000), pcm[12000..14000]);
        s.seek(s.length);
        assert!(s.read(4800).is_empty());
        s.seek(0);
        assert_eq!(s.read(1200), pcm[..2400]);
    }
}
