//! The speakers and the microphone (cpal: CoreAudio, WASAPI, ALSA/PulseAudio/PipeWire).
//! Streams are created and dropped on the thread that owns them (they are not `Send` everywhere).

use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, Stream, StreamConfig};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

/// What the speakers play: a slice of a shared buffer (at the device's rate).
#[derive(Default)]
struct Playing {
    buf: Arc<Vec<f32>>,
    pos: usize,
    end: usize,
    /// a short fade at the start and the end, so cuts inside a lesson don't click
    fade: usize,
    start: usize,
}

pub struct Player {
    _stream: Stream,
    pub rate: u32,
    state: Arc<Mutex<Playing>>,
    /// samples played so far in the current buffer (read by the UI clock)
    pos: Arc<AtomicU64>,
    busy: Arc<AtomicBool>,
    /// the device went away (unplugged, default changed): open a new player
    pub broken: Arc<AtomicBool>,
}

fn write_out<T: SizedSample + FromSample<f32>>(data: &mut [T], channels: usize, st: &Mutex<Playing>, pos: &AtomicU64, busy: &AtomicBool) {
    let mut guard = st.try_lock();
    for frame in data.chunks_mut(channels) {
        let mut v = 0.0f32;
        if let Some(p) = guard.as_deref_mut() {
            if p.pos < p.end {
                let x = p.buf[p.pos];
                let from_start = p.pos - p.start;
                let to_end = p.end - p.pos;
                let g = if p.fade > 0 { (from_start.min(to_end) as f32 / p.fade as f32).min(1.0) } else { 1.0 };
                v = x * g;
                p.pos += 1;
                pos.store(p.pos as u64, Ordering::Relaxed);
                if p.pos >= p.end {
                    busy.store(false, Ordering::Release);
                }
            }
        }
        let s = T::from_sample(v);
        for out in frame.iter_mut() {
            *out = s;
        }
    }
}

fn name_of(d: &cpal::Device) -> Option<String> {
    d.description().ok().map(|x| x.name().to_string())
}

/// Speakers/headphones by name (for settings). Two devices with one name (two identical monitors) are
/// told apart as "Name" and "Name (2)".
pub fn output_names() -> Vec<String> {
    label(cpal::default_host().output_devices().map(|it| it.filter_map(|d| name_of(&d)).collect()).unwrap_or_default())
}

/// Microphones by name (for settings).
pub fn input_names() -> Vec<String> {
    label(cpal::default_host().input_devices().map(|it| it.filter_map(|d| name_of(&d)).collect()).unwrap_or_default())
}

fn label(names: Vec<String>) -> Vec<String> {
    let mut count = std::collections::HashMap::<String, usize>::new();
    names
        .into_iter()
        .map(|n| {
            let k = count.entry(n.clone()).or_insert(0);
            *k += 1;
            if *k == 1 { n } else { format!("{n} ({k})") }
        })
        .collect()
}

/// "Name (2)" → ("Name", 2); anything else → (it, 1).
fn unlabel(label: &str) -> (&str, usize) {
    if let Some(open) = label.rfind(" (") {
        if let Some(n) = label[open + 2..].strip_suffix(')').and_then(|d| d.parse::<usize>().ok()) {
            if n >= 2 {
                return (&label[..open], n);
            }
        }
    }
    (label, 1)
}

/// The device with this label, else the system's default (a saved device may be unplugged).
fn pick(input: bool, name: Option<&str>) -> Option<cpal::Device> {
    let host = cpal::default_host();
    if let Some(l) = name {
        let (base, nth) = unlabel(l);
        let list: Option<Vec<cpal::Device>> = if input { host.input_devices().ok().map(|i| i.collect()) } else { host.output_devices().ok().map(|i| i.collect()) };
        let mut same = list.unwrap_or_default().into_iter().filter(|d| name_of(d).as_deref() == Some(base));
        // the exact one; or, if a label like "Mic (2)" is really a device's own name, that
        if let Some(d) = same.nth(nth - 1) {
            return Some(d);
        }
        let list: Option<Vec<cpal::Device>> = if input { host.input_devices().ok().map(|i| i.collect()) } else { host.output_devices().ok().map(|i| i.collect()) };
        if let Some(d) = list.unwrap_or_default().into_iter().find(|d| name_of(d).as_deref() == Some(l)) {
            return Some(d);
        }
    }
    if input { host.default_input_device() } else { host.default_output_device() }
}

#[cfg(test)]
mod tests {
    #[test]
    fn same_named_devices_get_numbers() {
        let v = super::label(vec!["BenQ RD280U".into(), "BenQ RD280U".into(), "Razer".into()]);
        assert_eq!(v, vec!["BenQ RD280U", "BenQ RD280U (2)", "Razer"]);
        assert_eq!(super::unlabel("BenQ RD280U (2)"), ("BenQ RD280U", 2));
        assert_eq!(super::unlabel("Razer"), ("Razer", 1));
        assert_eq!(super::unlabel("Mic (USB)"), ("Mic (USB)", 1));
    }
}

impl Player {
    pub fn open(name: Option<&str>) -> Result<Self> {
        let dev = pick(false, name).ok_or_else(|| anyhow!("no speakers or headphones found"))?;
        let sup = dev.default_output_config()?;
        let fmt = sup.sample_format();
        let cfg: StreamConfig = sup.into();
        let channels = cfg.channels as usize;
        let state = Arc::new(Mutex::new(Playing::default()));
        let pos = Arc::new(AtomicU64::new(0));
        let busy = Arc::new(AtomicBool::new(false));
        let broken = Arc::new(AtomicBool::new(false));
        macro_rules! build {
            ($t:ty) => {{
                let (st, p, b, br) = (state.clone(), pos.clone(), busy.clone(), broken.clone());
                dev.build_output_stream(
                    cfg.clone(),
                    move |d: &mut [$t], _: &cpal::OutputCallbackInfo| write_out(d, channels, &st, &p, &b),
                    move |e| {
                        log::warn!("speaker stream: {e}");
                        if !matches!(e.kind(), cpal::ErrorKind::Xrun | cpal::ErrorKind::RealtimeDenied) {
                            br.store(true, Ordering::Release);
                        }
                    },
                    None,
                )?
            }};
        }
        let stream = match fmt {
            SampleFormat::F32 => build!(f32),
            SampleFormat::I16 => build!(i16),
            SampleFormat::I32 => build!(i32),
            SampleFormat::U16 => build!(u16),
            SampleFormat::F64 => build!(f64),
            f => return Err(anyhow!("unsupported speaker format {f}")),
        };
        stream.play()?;
        Ok(Self { _stream: stream, rate: cfg.sample_rate, state, pos, busy, broken })
    }

    /// Play `buf[from..to]` (samples at `self.rate`).
    pub fn play(&self, buf: Arc<Vec<f32>>, from: usize, to: usize) {
        let to = to.min(buf.len());
        let from = from.min(to);
        let mut p = self.state.lock();
        *p = Playing { buf, pos: from, end: to, fade: (self.rate / 200) as usize, start: from };
        self.pos.store(from as u64, Ordering::Relaxed);
        self.busy.store(to > from, Ordering::Release);
    }

    pub fn stop(&self) {
        let mut p = self.state.lock();
        // fade out over 5 ms instead of cutting
        let fade = (self.rate / 200) as usize;
        if p.pos < p.end {
            p.end = (p.pos + fade).min(p.end);
            p.fade = fade;
        }
    }

    pub fn is_playing(&self) -> bool {
        self.busy.load(Ordering::Acquire)
    }

    /// Where playback is, in samples of the current buffer.
    pub fn position(&self) -> usize {
        self.pos.load(Ordering::Relaxed) as usize
    }
}

/// The microphone: mono samples collected while it is open.
pub struct Mic {
    _stream: Stream,
    pub rate: u32,
    buf: Arc<Mutex<Vec<f32>>>,
}

fn read_in<T: SizedSample + cpal::Sample>(data: &[T], channels: usize, buf: &Mutex<Vec<f32>>)
where
    f32: FromSample<T>,
{
    let mut b = buf.lock();
    for frame in data.chunks(channels) {
        let s: f32 = frame.iter().map(|&x| <f32 as FromSample<T>>::from_sample_(x)).sum::<f32>() / channels as f32;
        b.push(s);
    }
}

impl Mic {
    pub fn open(name: Option<&str>) -> Result<Self> {
        let dev = pick(true, name).ok_or_else(|| anyhow!("no microphone found"))?;
        let sup = dev.default_input_config()?;
        let fmt = sup.sample_format();
        let cfg: StreamConfig = sup.into();
        let channels = cfg.channels as usize;
        let buf = Arc::new(Mutex::new(Vec::with_capacity(cfg.sample_rate as usize * 30)));
        macro_rules! build {
            ($t:ty) => {{
                let b = buf.clone();
                dev.build_input_stream(
                    cfg.clone(),
                    move |d: &[$t], _: &cpal::InputCallbackInfo| read_in(d, channels, &b),
                    |e| log::warn!("microphone stream: {e}"),
                    None,
                )?
            }};
        }
        let stream = match fmt {
            SampleFormat::F32 => build!(f32),
            SampleFormat::I16 => build!(i16),
            SampleFormat::I32 => build!(i32),
            SampleFormat::U16 => build!(u16),
            SampleFormat::F64 => build!(f64),
            f => return Err(anyhow!("unsupported microphone format {f}")),
        };
        stream.play()?;
        Ok(Self { _stream: stream, rate: cfg.sample_rate, buf })
    }

    /// Everything heard since the last call.
    pub fn take(&self) -> Vec<f32> {
        std::mem::take(&mut *self.buf.lock())
    }
}

/// Can we hear the microphone? Opening it also makes the system ask for permission the first time.
#[derive(Clone, Debug, serde::Serialize, PartialEq)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum MicState {
    /// sound comes in
    Ok { device: String },
    /// the system gives only digital silence (exact zeros): access is blocked, or the prompt is still open
    Blocked { device: String },
    /// no microphone, or it can't be opened
    Missing { error: String },
}

/// Listen for up to `wait` to see whether real sound arrives (any real mic hears a little room noise).
pub fn probe_mic(name: Option<&str>, wait: std::time::Duration) -> MicState {
    let device = pick(true, name).and_then(|d| name_of(&d)).unwrap_or_else(|| "Microphone".into());
    let mic = match Mic::open(name) {
        Ok(m) => m,
        Err(e) => return MicState::Missing { error: e.to_string() },
    };
    let t = std::time::Instant::now();
    let mut got = 0usize;
    while t.elapsed() < wait {
        std::thread::sleep(std::time::Duration::from_millis(50));
        let x = mic.take();
        got += x.len();
        if x.iter().any(|v| *v != 0.0) {
            return MicState::Ok { device };
        }
    }
    let _ = got;
    MicState::Blocked { device }
}
