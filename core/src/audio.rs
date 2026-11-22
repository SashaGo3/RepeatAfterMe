//! Audio as plain mono f32 samples: decoding lesson files, resampling, finding speech.

use anyhow::{anyhow, Context, Result};
use std::path::Path;
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

#[derive(Clone, Debug, Default)]
pub struct Clip {
    pub rate: u32,
    pub samples: Vec<f32>,
}

impl Clip {
    pub fn duration(&self) -> f64 {
        if self.rate == 0 { 0.0 } else { self.samples.len() as f64 / self.rate as f64 }
    }
    pub fn index(&self, t: f64) -> usize {
        ((t.max(0.0) * self.rate as f64) as usize).min(self.samples.len())
    }
    pub fn slice(&self, from: f64, to: f64) -> &[f32] {
        let (a, b) = (self.index(from), self.index(to));
        &self.samples[a..b.max(a)]
    }
}

/// Decode an audio file (mp3, m4a/aac/alac, wav, flac, ogg, …) to mono.
pub fn decode_file(path: &Path) -> Result<Clip> {
    let file = std::fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let mut format = symphonia::default::get_probe()
        .probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())
        .with_context(|| format!("{} is not an audio file I can read", path.display()))?;
    let track = format.default_track(TrackType::Audio).ok_or_else(|| anyhow!("no audio track"))?;
    let params = track.codec_params.as_ref().and_then(|p| p.audio()).ok_or_else(|| anyhow!("no audio parameters"))?;
    let mut decoder = symphonia::default::get_codecs().make_audio_decoder(params, &AudioDecoderOptions::default())?;
    let track_id = track.id;
    let mut rate = 0;
    let mut mono: Vec<f32> = Vec::new();
    let mut buf: Vec<f32> = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => break,
            Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(SymError::ResetRequired) => break,
            Err(e) => return Err(e.into()),
        };
        if packet.track_id != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(ab) => {
                let ch = ab.spec().channels().count().max(1);
                rate = ab.spec().rate();
                buf.clear();
                ab.copy_to_vec_interleaved(&mut buf);
                mono.reserve(buf.len() / ch);
                for frame in buf.chunks_exact(ch) {
                    mono.push(frame.iter().sum::<f32>() / ch as f32);
                }
            }
            Err(SymError::DecodeError(_)) => continue, // a damaged packet: skip it
            Err(e) => return Err(e.into()),
        }
    }
    if rate == 0 || mono.is_empty() {
        return Err(anyhow!("{} has no sound", path.display()));
    }
    Ok(Clip { rate, samples: mono })
}

/// Windowed-sinc resampling (with the low-pass that downsampling needs). The kernel is tabulated, so
/// a 7-minute lesson takes well under a second.
pub fn resample(x: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || x.is_empty() {
        return x.to_vec();
    }
    const ZEROS: f64 = 12.0; // zero crossings each side
    const PHASES: usize = 256; // table steps per input sample
    let ratio = to as f64 / from as f64;
    let cutoff = ratio.min(1.0) * 0.94;
    let half = ZEROS / cutoff; // window half-width in input samples
    let taps = half.ceil() as i64;
    // kernel at offsets d = i / PHASES for i in 0..=taps*PHASES (symmetric)
    let n_tab = (taps as usize + 1) * PHASES + 2;
    let table: Vec<f32> = (0..n_tab)
        .map(|i| {
            let d = i as f64 / PHASES as f64;
            if d >= half {
                return 0.0;
            }
            let z = d * cutoff;
            let sinc = if z < 1e-9 { 1.0 } else { (std::f64::consts::PI * z).sin() / (std::f64::consts::PI * z) };
            let win = 0.5 + 0.5 * (std::f64::consts::PI * d / half).cos();
            (sinc * win * cutoff) as f32
        })
        .collect();
    let k_at = |d: f64| -> f32 {
        let f = d.abs() * PHASES as f64;
        let i = f as usize;
        let t = (f - i as f64) as f32;
        table[i] * (1.0 - t) + table[i + 1] * t
    };
    let n_out = (x.len() as f64 * ratio).round() as usize;
    let last = x.len() as i64 - 1;
    let mut out = Vec::with_capacity(n_out);
    for o in 0..n_out {
        let t = o as f64 / ratio;
        let c = t.floor() as i64;
        let (lo, hi) = ((c - taps + 1).max(0), (c + taps).min(last));
        let mut acc = 0.0f32;
        for j in lo..=hi {
            acc += x[j as usize] * k_at(t - j as f64);
        }
        out.push(acc);
    }
    out
}

/// A second-order high-pass filter (RBJ biquad, Q = 0.707), kept across calls.
pub struct HighPass {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl HighPass {
    pub fn new(rate: u32, cutoff: f32) -> Self {
        let w = std::f32::consts::TAU * cutoff / rate as f32;
        let alpha = w.sin() / (2.0 * std::f32::consts::FRAC_1_SQRT_2);
        let c = w.cos();
        let a0 = 1.0 + alpha;
        Self {
            b0: (1.0 + c) / 2.0 / a0,
            b1: -(1.0 + c) / a0,
            b2: (1.0 + c) / 2.0 / a0,
            a1: -2.0 * c / a0,
            a2: (1.0 - alpha) / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }
    pub fn run(&mut self, x: &mut [f32]) {
        for v in x.iter_mut() {
            let y = self.b0 * *v + self.b1 * self.x1 + self.b2 * self.x2 - self.a1 * self.y1 - self.a2 * self.y2;
            self.x2 = self.x1;
            self.x1 = *v;
            self.y2 = self.y1;
            self.y1 = y;
            *v = y;
        }
    }
}

/// Loudness per 10 ms frame, in dB.
pub fn frame_db(x: &[f32], rate: u32) -> Vec<f32> {
    let hop = (rate / 100).max(1) as usize;
    x.chunks(hop)
        .map(|c| {
            let ms = c.iter().map(|v| v * v).sum::<f32>() / c.len() as f32;
            10.0 * (ms + 1e-10).log10()
        })
        .collect()
}

fn percentile(v: &[f32], p: f32) -> f32 {
    if v.is_empty() {
        return -100.0;
    }
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    s[((s.len() - 1) as f32 * p / 100.0).round() as usize]
}

/// Stretches of speech, in seconds: louder than the room by 15 dB, holes < 250 ms closed, ≥ 50 ms long.
pub fn speech_islands(x: &[f32], rate: u32) -> Vec<(f64, f64)> {
    let db = frame_db(x, rate);
    if db.is_empty() {
        return vec![];
    }
    let thr = (percentile(&db, 10.0) + 15.0).max(-55.0);
    let mut speech: Vec<bool> = db.iter().map(|&d| d > thr).collect();
    let idx: Vec<usize> = speech.iter().enumerate().filter(|(_, s)| **s).map(|(i, _)| i).collect();
    for w in idx.windows(2) {
        if w[1] - w[0] > 1 && w[1] - w[0] <= 25 {
            for s in &mut speech[w[0]..w[1]] {
                *s = true;
            }
        }
    }
    let mut out = vec![];
    let mut start: Option<usize> = None;
    for (i, &s) in speech.iter().chain(std::iter::once(&false)).enumerate() {
        match (s, start) {
            (true, None) => start = Some(i),
            (false, Some(a)) => {
                if i - a >= 5 {
                    out.push((a as f64 / 100.0, i as f64 / 100.0));
                }
                start = None;
            }
            _ => {}
        }
    }
    out
}

/// Word timings from Whisper often start back in the previous utterance and stretch over the pause, but
/// their ends are good: clip each word to the speech island that holds its end.
pub fn snap_to_speech(words: &mut [crate::align::SpokenWord], islands: &[(f64, f64)]) {
    if islands.is_empty() {
        return;
    }
    let starts: Vec<f64> = islands.iter().map(|i| i.0).collect();
    for w in words.iter_mut() {
        let k = starts.partition_point(|&s| s <= w.e - 0.05);
        if k == 0 {
            continue;
        }
        let (a, b) = islands[k - 1];
        if b < w.s {
            continue; // the word lies entirely in a pause: leave it
        }
        w.s = w.s.max(a);
        if w.e > a {
            w.e = w.e.min(b);
        }
        if w.e <= w.s {
            w.e = w.s + 0.05;
        }
        w.s = (w.s * 1000.0).round() / 1000.0;
        w.e = (w.e * 1000.0).round() / 1000.0;
    }
}

/// Save mono samples as a 16-bit WAV (for debugging).
pub fn write_wav(path: &Path, x: &[f32], rate: u32) -> std::io::Result<()> {
    let mut d = Vec::with_capacity(44 + x.len() * 2);
    let data_len = (x.len() * 2) as u32;
    d.extend_from_slice(b"RIFF");
    d.extend_from_slice(&(36 + data_len).to_le_bytes());
    d.extend_from_slice(b"WAVEfmt ");
    d.extend_from_slice(&16u32.to_le_bytes());
    d.extend_from_slice(&1u16.to_le_bytes());
    d.extend_from_slice(&1u16.to_le_bytes());
    d.extend_from_slice(&rate.to_le_bytes());
    d.extend_from_slice(&(rate * 2).to_le_bytes());
    d.extend_from_slice(&2u16.to_le_bytes());
    d.extend_from_slice(&16u16.to_le_bytes());
    d.extend_from_slice(b"data");
    d.extend_from_slice(&data_len.to_le_bytes());
    for v in x {
        d.extend_from_slice(&((v.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    std::fs::write(path, d)
}

/// Cut silence off both ends of a take, keeping a little air (120 ms before, 200 ms after).
pub fn trim(x: &[f32], rate: u32) -> &[f32] {
    let isl = speech_islands(x, rate);
    let (Some(first), Some(last)) = (isl.first(), isl.last()) else { return x };
    let a = ((first.0 - 0.12).max(0.0) * rate as f64) as usize;
    let b = (((last.1 + 0.2) * rate as f64) as usize).min(x.len());
    &x[a.min(b)..b]
}

#[cfg(test)]
pub(crate) fn tone(rate: u32, secs: f64, freq: f32, amp: f32) -> Vec<f32> {
    (0..(rate as f64 * secs) as usize).map(|i| amp * (i as f32 * freq * std::f32::consts::TAU / rate as f32).sin()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noise(n: usize, amp: f32) -> Vec<f32> {
        let mut s = 12345u32;
        (0..n)
            .map(|_| {
                s ^= s << 13;
                s ^= s >> 17;
                s ^= s << 5;
                amp * ((s as f32 / u32::MAX as f32) - 0.5)
            })
            .collect()
    }

    #[test]
    fn resample_keeps_tone_and_length() {
        let x = tone(44100, 1.0, 440.0, 0.5);
        let y = resample(&x, 44100, 16000);
        assert_eq!(y.len(), 16000);
        let rms = (y.iter().map(|v| v * v).sum::<f32>() / y.len() as f32).sqrt();
        assert!((rms - 0.5 / 2f32.sqrt()).abs() < 0.02, "rms {rms}");
        // a tone above the new Nyquist is removed
        let hi = resample(&tone(48000, 1.0, 12000.0, 0.5), 48000, 16000);
        let rms = (hi.iter().map(|v| v * v).sum::<f32>() / hi.len() as f32).sqrt();
        assert!(rms < 0.05, "aliased rms {rms}");
    }

    #[test]
    fn islands_and_trim() {
        let r = 16000;
        let mut x = noise(r as usize / 2, 0.002); // 0.5 s room
        x.extend(tone(r, 0.4, 200.0, 0.3)); // 0.5-0.9 speech
        x.extend(noise(r as usize / 10, 0.002)); // 100 ms hole: closed
        x.extend(tone(r, 0.3, 200.0, 0.3)); // 1.0-1.3
        x.extend(noise(r as usize, 0.002)); // 1 s room
        x.extend(tone(r, 0.3, 300.0, 0.3)); // 2.3-2.6
        x.extend(noise(r as usize / 2, 0.002));
        let isl = speech_islands(&x, r);
        assert_eq!(isl.len(), 2, "{isl:?}");
        assert!((isl[0].0 - 0.5).abs() < 0.02 && (isl[0].1 - 1.3).abs() < 0.02, "{isl:?}");
        assert!((isl[1].0 - 2.3).abs() < 0.02, "{isl:?}");
        let t = trim(&x, r);
        let secs = t.len() as f64 / r as f64;
        assert!((secs - (2.6 + 0.2 - 0.38)).abs() < 0.05, "{secs}");
    }

    #[test]
    fn high_pass_cuts_rumble_keeps_voice() {
        let rms = |x: &[f32]| (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt();
        let mut hum = tone(48000, 0.5, 60.0, 0.5);
        HighPass::new(48000, 200.0).run(&mut hum);
        let mut voice = tone(48000, 0.5, 1000.0, 0.5);
        HighPass::new(48000, 200.0).run(&mut voice);
        assert!(rms(&hum[4800..]) < 0.05, "60 Hz hum mostly gone: {}", rms(&hum[4800..]));
        assert!(rms(&voice[4800..]) > 0.33, "1 kHz kept: {}", rms(&voice[4800..]));
    }

    #[test]
    fn snap_clips_words_to_speech() {
        use crate::align::SpokenWord;
        let isl = vec![(1.0, 1.5), (3.0, 3.6)];
        let mut w = vec![SpokenWord::new("a", 0.2, 1.4), SpokenWord::new("b", 1.6, 3.5)];
        snap_to_speech(&mut w, &isl);
        assert_eq!((w[0].s, w[0].e), (1.0, 1.4));
        assert_eq!((w[1].s, w[1].e), (3.0, 3.5));
    }
}

#[cfg(test)]
mod speed {
    #[test]
    fn resample_is_fast_enough() {
        // a 7-minute lesson at 44.1 kHz → 48 kHz (debug builds are ~20x slower; only check in release)
        let x = super::tone(44100, 60.0, 440.0, 0.3);
        let t = std::time::Instant::now();
        let y = super::resample(&x, 44100, 48000);
        let el = t.elapsed().as_secs_f64() * 7.0;
        assert_eq!(y.len(), 48000 * 60);
        if !cfg!(debug_assertions) {
            // shared build machines (CI) are slower and busier: a looser limit there
            let limit = if std::env::var_os("CI").is_some() { 5.0 } else { 1.0 };
            assert!(el < limit, "7 minutes would take {el:.2}s");
        }
    }
}
