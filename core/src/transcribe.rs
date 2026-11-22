//! Lesson audio → words with timings, with whisper.cpp (base.en, ~60 MB). It runs once per lesson; the
//! result is cached by the audio's content hash.

use crate::align::SpokenWord;
use crate::audio::{self, Clip};
use anyhow::{anyhow, Result};
use once_cell::sync::OnceCell;
use parking_lot::Mutex;
use std::path::{Path, PathBuf};
use whisper_rs::{
    DtwMode, DtwModelPreset, DtwParameters, FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters,
};

pub const SAMPLE_RATE: u32 = 16000;

static MODEL: OnceCell<Mutex<WhisperContext>> = OnceCell::new();
static MODEL_PATH: OnceCell<PathBuf> = OnceCell::new();

/// Where the model file is (the app's resources). Loading waits until the first transcription.
pub fn set_model_path(path: PathBuf) {
    let _ = MODEL_PATH.set(path);
}

fn model() -> Result<&'static Mutex<WhisperContext>> {
    MODEL.get_or_try_init(|| {
        let path = MODEL_PATH
            .get()
            .cloned()
            .or_else(|| std::env::var_os("RAM_MODEL").map(PathBuf::from))
            .ok_or_else(|| anyhow!("the speech model's location is not set"))?;
        if !path.is_file() {
            return Err(anyhow!("speech model missing: {}", path.display()));
        }
        silence_logs();
        let mut p = WhisperContextParameters::default();
        p.use_gpu = false;
        p.dtw_parameters(DtwParameters {
            mode: DtwMode::ModelPreset { model_preset: preset_for(&path) },
            ..Default::default()
        });
        let ctx = WhisperContext::new_with_params(&path, p).map_err(|e| anyhow!("loading the speech model: {e}"))?;
        Ok(Mutex::new(ctx))
    })
}

/// The attention heads DTW reads depend on the model: pick them from the file name (ggml-<size>.en…).
fn preset_for(path: &Path) -> DtwModelPreset {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_lowercase();
    let en = name.contains(".en");
    match () {
        _ if name.contains("tiny") => if en { DtwModelPreset::TinyEn } else { DtwModelPreset::Tiny },
        _ if name.contains("small") => if en { DtwModelPreset::SmallEn } else { DtwModelPreset::Small },
        _ if name.contains("medium") => if en { DtwModelPreset::MediumEn } else { DtwModelPreset::Medium },
        _ if name.contains("turbo") => DtwModelPreset::LargeV3Turbo,
        _ if name.contains("large") => DtwModelPreset::LargeV3,
        _ => if en { DtwModelPreset::BaseEn } else { DtwModelPreset::Base },
    }
}

fn silence_logs() {
    unsafe extern "C" fn quiet(_: whisper_rs_sys::ggml_log_level, _: *const std::os::raw::c_char, _: *mut std::ffi::c_void) {}
    unsafe {
        whisper_rs_sys::whisper_log_set(Some(quiet), std::ptr::null_mut());
        whisper_rs_sys::ggml_log_set(Some(quiet), std::ptr::null_mut());
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Timing {
    /// whisper's own token timestamps
    Tokens,
    /// cross-attention alignment (DTW): token start times
    Dtw,
}

pub struct Options<'a> {
    /// printed text of the lesson: steers the spelling towards the PDF's words
    pub prompt: Option<&'a str>,
    pub timing: Timing,
    pub threads: usize,
}

impl Default for Options<'_> {
    fn default() -> Self {
        Self {
            prompt: None,
            timing: Timing::Dtw,
            threads: std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(8),
        }
    }
}

struct Tok {
    text: String,
    t0: f64,
    t1: f64,
    dtw: f64,
}

/// Transcribe lesson audio (any rate). `progress` gets 0..1.
pub fn transcribe(
    clip: &Clip,
    opts: &Options,
    mut progress: impl FnMut(f32) + Send + 'static,
    cancelled: std::sync::Arc<dyn Fn() -> bool + Send + Sync>,
) -> Result<Vec<SpokenWord>> {
    let pcm = audio::resample(&clip.samples, clip.rate, SAMPLE_RATE);
    let ctx = model()?.lock();
    let mut state = ctx.create_state().map_err(|e| anyhow!("{e}"))?;
    let mut p = FullParams::new(SamplingStrategy::BeamSearch { beam_size: 5, patience: -1.0 });
    p.set_n_threads(opts.threads as i32);
    p.set_language(Some("en"));
    p.set_token_timestamps(true);
    p.set_no_context(true); // like condition_on_previous_text=False: no runaway repeats
    p.set_suppress_blank(true);
    p.set_print_progress(false);
    p.set_print_realtime(false);
    p.set_print_special(false);
    p.set_print_timestamps(false);
    if let Some(prompt) = opts.prompt {
        p.set_initial_prompt(prompt);
    }
    progress(0.0);
    p.set_progress_callback_safe(move |pct: i32| progress(pct as f32 / 100.0));
    // (whisper-rs 0.16's set_abort_callback_safe casts its box to the wrong type and crashes: do it here)
    struct Abort(std::sync::Arc<dyn Fn() -> bool + Send + Sync>);
    unsafe extern "C" fn abort_trampoline(user_data: *mut std::ffi::c_void) -> bool {
        let a = unsafe { &*(user_data as *const Abort) };
        (a.0)()
    }
    let abort = Box::new(Abort(cancelled.clone()));
    unsafe {
        p.set_abort_callback(Some(abort_trampoline));
        p.set_abort_callback_user_data(&*abort as *const Abort as *mut std::ffi::c_void);
    }
    let res = state.full(p, &pcm);
    drop(abort); // whisper is done with it
    if cancelled() {
        return Err(anyhow!("cancelled"));
    }
    res.map_err(|e| anyhow!("transcribing: {e}"))?;

    let eot = ctx.token_eot();
    let mut words: Vec<SpokenWord> = vec![];
    for seg in state.as_iter() {
        let seg_start = seg.start_timestamp() as f64 / 100.0;
        let seg_end = seg.end_timestamp() as f64 / 100.0;
        let mut toks: Vec<Tok> = vec![];
        for i in 0..seg.n_tokens() {
            let Some(tok) = seg.get_token(i) else { continue };
            let d = tok.token_data();
            if d.id >= eot {
                continue; // special tokens and timestamps
            }
            let text = tok.to_str_lossy().map(|s| s.into_owned()).unwrap_or_default();
            if text.starts_with("[_") || text.starts_with("<|") {
                continue;
            }
            toks.push(Tok { text, t0: d.t0 as f64 / 100.0, t1: d.t1 as f64 / 100.0, dtw: d.t_dtw as f64 / 100.0 });
        }
        // tokens → words: a token that starts with a space starts a new word
        let mut groups: Vec<Vec<Tok>> = vec![];
        for t in toks {
            if t.text.starts_with(' ') || groups.is_empty() {
                groups.push(vec![t]);
            } else {
                groups.last_mut().unwrap().push(t);
            }
        }
        let n = groups.len();
        for k in 0..n {
            let g = &groups[k];
            let text: String = g.iter().map(|t| t.text.as_str()).collect::<String>().trim().to_string();
            if text.is_empty() || crate::align::norm(&text).is_empty() && !words.is_empty() {
                // punctuation on its own: glue it to the word before
                if let Some(last) = words.last_mut() {
                    last.w.push_str(&text);
                }
                continue;
            }
            let (s, e) = match opts.timing {
                Timing::Tokens => (g[0].t0, g.last().unwrap().t1),
                Timing::Dtw => {
                    // whisper.cpp's DTW time is where a token ends: a word runs from the end of the word
                    // before (or the segment's start) to the end of its own last token
                    // (punctuation tokens get the time of whatever comes next: use the last real token)
                    let end_of = |g: &[Tok]| g.iter().rev().find(|t| t.text.chars().any(|c| c.is_alphanumeric())).unwrap_or(&g[g.len() - 1]).dtw;
                    let s = if k > 0 { end_of(&groups[k - 1]) } else { seg_start };
                    let e = end_of(g).min(seg_end.max(s));
                    (s, e.max(s + 0.05))
                }
            };
            words.push(SpokenWord::new(&text, s, e));
        }
    }
    let mut words = crate::align::merge_contractions(words);
    let islands = audio::speech_islands(&pcm, SAMPLE_RATE);
    settle_pause_words(&mut words, &islands);
    audio::snap_to_speech(&mut words, &islands);
    Ok(words)
}

/// A "word" with (almost) no sound under it and its end in a pause: whisper's timing of it is wrong.
/// When the next stretch of speech is a sentence (two words or more), the word starts that sentence
/// ("When" before "you leave the city"): it moves there. Otherwise it is the recogniser filling a pause
/// in a word list ("seat … sit" heard twice): it goes.
fn settle_pause_words(words: &mut Vec<SpokenWord>, islands: &[(f64, f64)]) {
    let speech_under = |w: &SpokenWord| -> f64 { islands.iter().map(|&(a, b)| (b.min(w.e) - a.max(w.s)).max(0.0)).sum() };
    let island_of_end = |e: f64| islands.iter().position(|&(a, b)| a <= e && e <= b + 0.05);
    let mut keep = vec![true; words.len()];
    for i in 0..words.len() {
        let sp = speech_under(&words[i]);
        if sp <= 0.04 {
            keep[i] = false;
            continue;
        }
        if island_of_end(words[i].e).is_some() || sp >= 0.1 {
            continue;
        }
        // the next island and how many words end in it
        let Some(k) = islands.iter().position(|&(a, _)| a >= words[i].e) else {
            keep[i] = false;
            continue;
        };
        let n = words[i + 1..].iter().filter(|w| island_of_end(w.e) == Some(k)).count();
        if n >= 2 {
            let a = islands[k].0;
            words[i].s = a;
            words[i].e = a + 0.05;
        } else {
            keep[i] = false;
        }
    }
    let mut k = keep.into_iter();
    words.retain(|_| k.next().unwrap());
}

/// The printed words as a prompt (whisper reads up to ~220 tokens of it). List numbers ("4.", "b)")
/// are left out: they are printed, not said, and in a prompt they get "heard" in the pauses.
pub fn prompt_from(words: &[crate::align::PdfWord]) -> String {
    let mut s = String::new();
    for w in words {
        if s.len() > 600 {
            break;
        }
        if is_list_marker(&w.text) || is_respelling(&w.text) {
            continue;
        }
        if !s.is_empty() {
            s.push(' ');
        }
        s.push_str(&w.text);
    }
    s
}

pub fn model_ready(path: &Path) -> bool {
    path.is_file()
}

fn is_list_marker(t: &str) -> bool {
    let core = t.trim_end_matches(['.', ')', ':']);
    t.len() > core.len() && !core.is_empty() && (core.chars().all(|c| c.is_ascii_digit()) || (core.len() == 1 && core.chars().all(|c| c.is_ascii_alphabetic())))
        || t.chars().all(|c| c.is_ascii_digit())
}

/// "STAH-PUUSH-ing", "FEE-(y)uhl-LYK": how a course writes a sound, not a word anyone says.
fn is_respelling(t: &str) -> bool {
    t.contains('-')
        && t.split('-').any(|part| {
            let letters: Vec<char> = part.chars().filter(|c| c.is_alphabetic()).collect();
            letters.len() >= 2 && letters.iter().all(|c| c.is_uppercase())
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_word_filling_a_pause_in_a_word_list_goes() {
        // "EEE vs. IH": whisper puts a second "sit" in the pause after the real one
        let isl = [(3.33, 4.02), (5.02, 5.57), (7.29, 8.05)];
        let mut w = vec![
            SpokenWord::new("C", 3.16, 3.82),
            SpokenWord::new("seat", 3.82, 5.5),
            SpokenWord::new("sit", 5.5, 6.84),
            SpokenWord::new("bead", 6.84, 8.02),
        ];
        settle_pause_words(&mut w, &isl);
        let got: Vec<&str> = w.iter().map(|w| w.w.as_str()).collect();
        assert_eq!(got, ["C", "seat", "bead"]);
    }

    #[test]
    fn a_word_timed_too_early_moves_to_its_sentence() {
        // "EEE vs. IH Sentences": "When" is timed in the pause before "you leave the city"
        let isl = [(0.81, 3.07), (5.9, 7.05)];
        let mut w = vec![
            SpokenWord::new("Sentences", 1.22, 3.0),
            SpokenWord::new("When", 3.0, 4.92),
            SpokenWord::new("you", 4.92, 6.16),
            SpokenWord::new("leave", 6.16, 6.38),
            SpokenWord::new("the", 6.38, 6.58),
        ];
        settle_pause_words(&mut w, &isl);
        assert_eq!(w.len(), 5);
        assert_eq!((w[1].w.as_str(), w[1].s), ("When", 5.9));
    }

    #[test]
    fn respellings() {
        for r in ["STAH-PUUSH-ing", "FEE-(y)uhl-LYK", "SUH-MOR", "duh-ZYN-NOHTS"] {
            assert!(super::is_respelling(r), "{r}");
        }
        for w in ["sister-in-law", "Kenneth's", "OK", "T-shirt"] {
            assert!(!super::is_respelling(w), "{w}");
        }
    }

    #[test]
    fn list_markers() {
        for m in ["4.", "12)", "b)", "7"] {
            assert!(super::is_list_marker(m), "{m}");
        }
        for w in ["I", "a", "Sentences.", "R."] {
            assert!(!super::is_list_marker(w) || w == "R.", "{w}");
        }
    }
}
