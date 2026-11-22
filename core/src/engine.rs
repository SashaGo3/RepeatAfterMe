//! The running app without its window: the lesson library, progress, loading lessons (decode, PDF,
//! transcribe once, align) and the practice session on the real speakers and microphone.
//! One thread owns the audio devices and the session; the UI sends [`Cmd`]s and gets [`Out`] events.

use crate::align::{self, build_phrases, phrase_text, PdfWord, SpokenWord};
use crate::audio::{self, Clip};
use crate::device::{Mic, Player};
use crate::library::{content_id, Entry, Library, Unpaired};
use crate::pdf;
use crate::progress::{LessonProgress, Store};
use crate::session::{self, Audio, Behaviour, Cmd, Lesson, Session};
use crate::settings::Settings;
use crate::transcribe;
use anyhow::{anyhow, Result};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Bump when the transcript format or settings change: old caches are redone.
const CACHE_VERSION: u32 = 4;

pub struct Paths {
    pub data: PathBuf,
    pub model: PathBuf,
    pub pdfium_dir: Option<PathBuf>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LessonInfo {
    pub id: String,
    pub name: String,
    pub group: String,
    pub audio: PathBuf,
    pub pdf: PathBuf,
    pub missing: bool,
    pub done: usize,
    pub total: Option<usize>,
    /// seconds practised
    pub secs: f64,
    /// unix seconds last practised (0 = never)
    pub last: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WordView {
    pub page: usize,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    /// the phrase this printed word is said in, -1 = not said
    pub phrase: i32,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhraseView {
    pub text: String,
    pub start: f64,
    pub end: f64,
    pub in_pdf: bool,
    pub done: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LessonView {
    pub id: String,
    pub name: String,
    pub pages: Vec<pdf::PageSize>,
    pub words: Vec<WordView>,
    /// per spoken word: its printed word (-1 none)
    pub spoken: Vec<i32>,
    pub phrases: Vec<PhraseView>,
    /// printed words you marked for attention
    pub marked: Vec<usize>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Out {
    /// opening a lesson: what's happening and how far (0..1)
    Loading { id: String, stage: String, progress: f32 },
    Lesson { lesson: LessonView },
    Session { event: session::Event },
    /// a phrase's done mark changed
    Done { phrase: usize, done: bool },
    /// the microphone test in settings: loudness 0..1
    MicLevel { level: f32 },
    /// the words marked for attention changed
    Marks { marked: Vec<usize> },
    Error { message: String },
}

pub type Sink = Arc<dyn Fn(Out) + Send + Sync>;

enum Msg {
    Open(String),
    Loaded(u64, Result<Box<Loaded>>),
    Cmd(Cmd),
    /// start a lesson over (the open one or any other)
    ResetLesson(String),
    /// every lesson starts over, time practised included (marked words stay)
    ResetAll,
    /// back to a fresh install: close the lesson, delete what the app made (replies when done)
    ResetApp(Sender<()>),
    ClearMarks(String),
    /// merge a backup into the progress (and into the open lesson)
    Restore(std::collections::BTreeMap<String, LessonProgress>, Sender<Result<usize>>),
    Settings(Settings),
    MicTest(bool),
    /// mark/unmark printed words (all marked → unmark them, else mark them)
    Mark(Vec<usize>),
    /// mark/unmark the current phrase's words
    MarkPhrase,
    ExportAnswers(PathBuf, Sender<Result<usize>>),
    TestSound,
    Quit,
}

struct Loaded {
    id: String,
    lesson_name: String,
    clip: Clip,
    words: Vec<SpokenWord>,
    pdf: Vec<PdfWord>,
    pages: Vec<pdf::PageSize>,
}

// ---------------------------------------------------------------- the real speakers and mic

struct Devices {
    player: Option<Player>,
    mic: Option<Mic>,
    input: Option<String>,
    output: Option<String>,
    /// the lesson at the speaker rate is made for this rate
    rate: u32,
    /// set when `rate` changed and the teacher needs resampling
    rate_changed: bool,
}

impl Devices {
    fn new(s: &Settings) -> Self {
        let player = Player::open(s.output.as_deref()).map_err(|e| log::error!("speakers: {e}")).ok();
        let rate = player.as_ref().map(|p| p.rate).unwrap_or(48000);
        Self { player, mic: None, input: s.input.clone(), output: s.output.clone(), rate, rate_changed: false }
    }
    /// New devices chosen in settings: open them next time they're needed.
    fn choose(&mut self, s: &Settings) {
        if s.output != self.output {
            self.output = s.output.clone();
            if let Some(p) = &self.player {
                p.broken.store(true, Ordering::Release);
            }
            self.ensure_player();
        }
        if s.input != self.input {
            self.input = s.input.clone();
            self.mic = None;
        }
    }
    fn ensure_player(&mut self) {
        if self.player.as_ref().is_none_or(|p| p.broken.load(Ordering::Acquire)) {
            self.player = None;
            match Player::open(self.output.as_deref()) {
                Ok(p) => {
                    if p.rate != self.rate {
                        self.rate = p.rate;
                        self.rate_changed = true;
                    }
                    self.player = Some(p);
                }
                Err(e) => log::error!("speakers: {e}"),
            }
        }
    }
}

impl Audio for Devices {
    fn out_rate(&self) -> u32 {
        self.rate
    }
    fn play(&mut self, buf: Arc<Vec<f32>>, from: usize, to: usize) {
        self.ensure_player();
        if let Some(p) = &self.player {
            p.play(buf, from, to);
        }
    }
    fn stop(&mut self) {
        if let Some(p) = &self.player {
            p.stop();
        }
    }
    fn is_playing(&self) -> bool {
        self.player.as_ref().is_some_and(|p| p.is_playing() && !p.broken.load(Ordering::Acquire))
    }
    fn position(&self) -> usize {
        self.player.as_ref().map(|p| p.position()).unwrap_or(0)
    }
    fn mic_open(&mut self) -> Result<u32> {
        if self.mic.is_none() {
            self.mic = Some(Mic::open(self.input.as_deref())?);
        }
        Ok(self.mic.as_ref().unwrap().rate)
    }
    fn mic_close(&mut self) {
        self.mic = None;
    }
    fn mic_take(&mut self) -> Vec<f32> {
        self.mic.as_ref().map(|m| m.take()).unwrap_or_default()
    }
}

// ---------------------------------------------------------------- loading a lesson

#[derive(Serialize, Deserialize)]
struct Cached {
    version: u32,
    model: String,
    duration: f64,
    words: Vec<SpokenWord>,
}

fn load_lesson(
    entry: &Entry,
    id: &str,
    cache_dir: &Path,
    model: &Path,
    report: &dyn Fn(&str, f32),
    cancelled: Arc<dyn Fn() -> bool + Send + Sync>,
) -> Result<Loaded> {
    report("Reading the PDF", 0.0);
    let text = pdf::read_words(&entry.pdf)?;
    report("Reading the audio", 0.0);
    let clip = audio::decode_file(&entry.audio)?;
    let model_name = model.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let cache = cache_dir.join(format!("{id}.json"));
    let cached: Option<Cached> = std::fs::read_to_string(&cache)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .filter(|c: &Cached| c.version == CACHE_VERSION && c.model == model_name);
    let words = match cached {
        Some(c) => c.words,
        None => {
            report("Getting the lesson ready", 0.0);
            let prompt = transcribe::prompt_from(&text.words);
            let opts = transcribe::Options { prompt: Some(&prompt), ..Default::default() };
            let (ptx, prx) = channel::<f32>();
            let words = std::thread::scope(|s| {
                // whisper.cpp needs a big stack (the default 2 MB of a Rust thread overflows)
                let h = std::thread::Builder::new()
                    .name("whisper".into())
                    .stack_size(32 << 20)
                    .spawn_scoped(s, || transcribe::transcribe(&clip, &opts, move |p| { let _ = ptx.send(p); }, cancelled.clone()))?;
                // pass progress on while whisper works
                loop {
                    match prx.recv_timeout(Duration::from_millis(200)) {
                        Ok(p) => report("Getting the lesson ready", p),
                        Err(RecvTimeoutError::Timeout) => {
                            if h.is_finished() {
                                break;
                            }
                        }
                        Err(RecvTimeoutError::Disconnected) => break,
                    }
                }
                h.join().map_err(|_| anyhow!("the speech engine crashed"))?
            })?;
            let c = Cached { version: CACHE_VERSION, model: model_name, duration: clip.duration(), words };
            crate::progress::write_atomic(&cache, serde_json::to_string(&c)?.as_bytes())?;
            c.words
        }
    };
    let name = entry.audio.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    Ok(Loaded { id: id.to_string(), lesson_name: name, clip, words, pdf: text.words, pages: text.pages })
}

fn view(l: &Loaded, words: &[SpokenWord], pdf: &[PdfWord], phrases: &[align::Phrase], prog: &LessonProgress) -> LessonView {
    let mut phrase_of = vec![-1i32; pdf.len()];
    for (k, p) in phrases.iter().enumerate() {
        for w in &words[p.a..p.b] {
            if w.pdf >= 0 {
                phrase_of[w.pdf as usize] = k as i32;
            }
        }
    }
    LessonView {
        id: l.id.clone(),
        name: l.lesson_name.clone(),
        pages: l.pages.clone(),
        words: pdf
            .iter()
            .zip(&phrase_of)
            .map(|(w, &ph)| WordView { page: w.page, x: w.rect.x, y: w.rect.y, w: w.rect.w, h: w.rect.h, phrase: ph })
            .collect(),
        spoken: words.iter().map(|w| w.pdf).collect(),
        phrases: phrases
            .iter()
            .map(|p| PhraseView { text: phrase_text(p, words, pdf), start: p.start, end: p.end, in_pdf: p.in_pdf, done: prog.is_done(p) })
            .collect(),
        marked: vec![],
    }
}

// ---------------------------------------------------------------- the engine

pub struct Engine {
    tx: Sender<Msg>,
    pub library: Arc<Mutex<Library>>,
    pub progress: Arc<Store>,
    data: PathBuf,
    ids: Arc<Mutex<std::collections::HashMap<PathBuf, (u64, String)>>>,
    settings: Arc<Mutex<Settings>>,
}

#[derive(Clone, Debug, Serialize)]
pub struct DeviceList {
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
}

impl Engine {
    pub fn start(paths: Paths, sink: Sink) -> Result<Self> {
        pdf::init(paths.pdfium_dir.as_deref())?;
        transcribe::set_model_path(paths.model.clone());
        std::fs::create_dir_all(&paths.data)?;
        let library = Arc::new(Mutex::new(Library::open(paths.data.join("library.json"))));
        let progress = Arc::new(Store::open(paths.data.join("progress.json")));
        let (tx, rx) = channel();
        let ids = Arc::new(Mutex::new(Default::default()));
        let settings = Arc::new(Mutex::new(Settings::load(&Settings::path(&paths.data))));
        let worker = Worker {
            settings: settings.lock().clone(),
            rx,
            tx: tx.clone(),
            sink,
            library: library.clone(),
            progress: progress.clone(),
            cache: paths.data.join("transcripts"),
            model: paths.model,
            generation: Arc::new(AtomicU64::new(0)),
            ids: Arc::clone(&ids),
        };
        std::thread::Builder::new().name("session".into()).stack_size(8 << 20).spawn(move || worker.run())?;
        std::thread::Builder::new().name("voice".into()).spawn(|| {
            let _ = TEST_VOICE.set(spoken("Repeat after me"));
        })?;
        Ok(Self { tx, library, progress, data: paths.data, ids, settings })
    }

    pub fn settings(&self) -> Settings {
        self.settings.lock().clone()
    }

    /// Save the settings and use them right away.
    pub fn set_settings(&self, s: Settings) -> Result<()> {
        s.save(&Settings::path(&self.data))?;
        *self.settings.lock() = s.clone();
        let _ = self.tx.send(Msg::Settings(s));
        Ok(())
    }

    pub fn devices(&self) -> DeviceList {
        DeviceList { inputs: crate::device::input_names(), outputs: crate::device::output_names() }
    }

    /// Is the microphone allowed and working? (Blocks up to ~0.8 s.)
    pub fn mic_check(&self) -> crate::device::MicState {
        let name = self.settings.lock().input.clone();
        crate::device::probe_mic(name.as_deref(), Duration::from_millis(800))
    }

    /// The logs folder: app.log (errors), and turns/ when diagnostics are on.
    pub fn logs_dir(&self) -> PathBuf {
        self.data.join("logs")
    }

    /// Everything useful for finding a problem, in one zip: logs, turn recordings, settings, the lesson list.
    pub fn export_logs(&self, dest: &Path) -> Result<usize> {
        use std::io::Write;
        let file = std::fs::File::create(dest)?;
        let mut zip = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let mut n = 0;
        let mut add = |zip: &mut zip::ZipWriter<std::fs::File>, path: &Path, name: String| -> Result<()> {
            if let Ok(bytes) = std::fs::read(path) {
                zip.start_file(name, opts)?;
                zip.write_all(&bytes)?;
                n += 1;
            }
            Ok(())
        };
        let logs = self.logs_dir();
        let mut stack = vec![logs.clone()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if let Ok(rel) = p.strip_prefix(&logs) {
                    add(&mut zip, &p, format!("logs/{}", rel.to_string_lossy().replace('\\', "/")))?;
                }
            }
        }
        for f in ["settings.json", "library.json", "progress.json"] {
            add(&mut zip, &self.data.join(f), f.to_string())?;
        }
        let about = format!(
            "Repeat After Me {}\nOS: {} {}\nMicrophones: {:?}\nSpeakers: {:?}\n",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH,
            crate::device::input_names(),
            crate::device::output_names()
        );
        zip.start_file("about.txt", opts)?;
        zip.write_all(about.as_bytes())?;
        zip.finish()?;
        Ok(n + 1)
    }

    /// A short two-note chime on the chosen speakers ("can you hear this?").
    pub fn test_sound(&self) {
        let _ = self.tx.send(Msg::TestSound);
    }

    /// Listen to the microphone for the settings screen (MicLevel events) until turned off.
    pub fn mic_test(&self, on: bool) {
        let _ = self.tx.send(Msg::MicTest(on));
    }

    pub fn data_dir(&self) -> &Path {
        &self.data
    }

    pub fn open(&self, id: &str) {
        let _ = self.tx.send(Msg::Open(id.to_string()));
    }
    pub fn command(&self, c: Cmd) {
        let _ = self.tx.send(Msg::Cmd(c));
    }
    /// Start a lesson over: done marks and tries go; the words you marked and the time practised stay.
    pub fn reset_lesson(&self, id: &str) {
        let _ = self.tx.send(Msg::ResetLesson(id.to_string()));
    }

    /// Clear all progress: every lesson back to not done, time practised back to zero. The words you
    /// marked stay (they're notes on the material, not progress).
    pub fn reset_all(&self) {
        let _ = self.tx.send(Msg::ResetAll);
    }

    /// Back to how the app was on first launch: no lessons, no progress, default settings, and none of
    /// what it made (transcripts, saved takes, unpacked zips). Your lesson files aren't touched; the
    /// logs stay.
    pub fn reset_app(&self) -> Result<()> {
        let (tx, rx) = channel();
        self.tx.send(Msg::ResetApp(tx)).map_err(|_| anyhow!("the app is closing"))?;
        rx.recv_timeout(Duration::from_secs(30)).map_err(|_| anyhow!("timed out"))?;
        self.library.lock().clear()?;
        self.progress.clear()?;
        self.ids.lock().clear();
        self.set_settings(Settings::default())?;
        for d in ["transcripts", "takes", "imported"] {
            let p = self.data.join(d);
            if p.exists() {
                std::fs::remove_dir_all(&p)?;
            }
        }
        Ok(())
    }

    pub fn clear_marks(&self, id: &str) {
        let _ = self.tx.send(Msg::ClearMarks(id.to_string()));
    }

    /// Look in the folders you added for new lessons.
    pub fn rescan(&self) -> Result<usize> {
        self.library.lock().rescan()
    }

    pub fn remove_group(&self, group: &str) -> Result<()> {
        self.library.lock().remove_group(group)
    }

    pub fn rename_group(&self, from: &str, to: &str) -> Result<()> {
        self.library.lock().rename_group(from, to)
    }

    /// Every lesson's progress in one file, to keep or take to another computer. Lessons are known by
    /// their recording's content, so it works wherever the files are there.
    pub fn backup_progress(&self, dest: &Path) -> Result<usize> {
        let all = self.progress.all();
        let json = serde_json::json!({ "app": "Repeat After Me", "kind": "progress", "version": 1, "lessons": all });
        crate::progress::write_atomic(dest, serde_json::to_string_pretty(&json)?.as_bytes())?;
        Ok(all.len())
    }

    /// Add a backup's progress to what's here: done stays done, marks are combined, nothing is lost.
    pub fn restore_progress(&self, src: &Path) -> Result<usize> {
        let text = std::fs::read_to_string(src)?;
        let v: serde_json::Value = serde_json::from_str(&text).map_err(|_| anyhow!("that isn't a progress backup"))?;
        // a backup file, or a plain progress.json from another computer's data folder
        let lessons = if v.get("kind").and_then(|k| k.as_str()) == Some("progress") { v["lessons"].clone() } else { v };
        let map: std::collections::BTreeMap<String, LessonProgress> =
            serde_json::from_value(lessons).map_err(|_| anyhow!("that isn't a progress backup"))?;
        let (tx, rx) = channel();
        self.tx.send(Msg::Restore(map, tx)).map_err(|_| anyhow!("the app is closing"))?;
        rx.recv_timeout(Duration::from_secs(30)).map_err(|_| anyhow!("timed out"))?
    }

    /// Mark (or unmark) printed words as needing attention.
    pub fn mark(&self, words: Vec<usize>) {
        let _ = self.tx.send(Msg::Mark(words));
    }
    pub fn mark_phrase(&self) {
        let _ = self.tx.send(Msg::MarkPhrase);
    }

    /// The open lesson's answers in one WAV for the teacher: per phrase, the teacher, then your latest take.
    pub fn export_answers(&self, dest: &Path) -> Result<usize> {
        let (tx, rx) = channel();
        self.tx.send(Msg::ExportAnswers(dest.to_path_buf(), tx)).map_err(|_| anyhow!("the app is closing"))?;
        rx.recv_timeout(Duration::from_secs(60)).map_err(|_| anyhow!("timed out"))?
    }

    /// Add folders, files, or .zip files of lessons (a zip is unpacked into the data folder first).
    pub fn add(&self, paths: &[PathBuf]) -> Result<(usize, Unpaired)> {
        let mut list = vec![];
        for p in paths {
            if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("zip")) {
                list.push(unzip(p, &self.data.join("imported"))?);
            } else {
                list.push(p.clone());
            }
        }
        self.library.lock().add(&list)
    }

    pub fn lessons(&self) -> Vec<LessonInfo> {
        let entries = self.library.lock().entries.clone();
        entries
            .iter()
            .map(|e| {
                let missing = !e.audio.is_file() || !e.pdf.is_file();
                let id = if missing { String::new() } else { id_for(&self.ids, &e.audio).unwrap_or_default() };
                let prog = self.progress.get(&id);
                LessonInfo {
                    id,
                    name: e.audio.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
                    group: e.group.clone(),
                    audio: e.audio.clone(),
                    pdf: e.pdf.clone(),
                    missing,
                    done: prog.phrases.values().filter(|p| p.done).count(),
                    total: prog.total,
                    secs: prog.secs,
                    last: prog.last,
                }
            })
            .collect()
    }

    /// Page `index` of a lesson's PDF as PNG.
    pub fn page_png(&self, id: &str, index: usize, width: u32) -> Result<Vec<u8>> {
        let pdf = entry_for(&self.library, &self.ids, id)?.pdf;
        pdf::render_page(&pdf, index, width)
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.tx.send(Msg::Quit);
    }
}

/// Unpack a zip of lessons into `base/<zip name>` (the same zip again refreshes that folder) and
/// return the folder to add: the zip's single top folder if it has one, so groups get its subfolders'
/// names. Entries that would land outside the folder, and macOS's __MACOSX copies, are skipped.
pub fn unzip(zip_path: &Path, base: &Path) -> Result<PathBuf> {
    let name = zip_path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Lessons".into());
    let dir = base.join(&name);
    let file = std::fs::File::open(zip_path)?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| anyhow!("{}: not a zip file I can open ({e})", zip_path.display()))?;
    let mut files = 0;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let Some(rel) = entry.enclosed_name() else { continue };
        if rel.components().any(|c| {
            let c = c.as_os_str().to_string_lossy();
            c == "__MACOSX" || c.starts_with('.')
        }) {
            continue;
        }
        let out = dir.join(&rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&out)?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut f = std::fs::File::create(&out)?;
        std::io::copy(&mut entry, &mut f)?;
        files += 1;
    }
    if files == 0 {
        return Err(anyhow!("{} has no files in it", zip_path.display()));
    }
    // one folder at the top (and nothing else): that's the course folder
    let top: Vec<PathBuf> = std::fs::read_dir(&dir)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')))
        .collect();
    Ok(if top.len() == 1 && top[0].is_dir() { top[0].clone() } else { dir })
}

/// Content ids, remembered per file (size + mtime) so the list doesn't re-read every file.
fn id_for(ids: &Mutex<std::collections::HashMap<PathBuf, (u64, String)>>, audio: &Path) -> Option<String> {
    let meta = std::fs::metadata(audio).ok()?;
    let stamp = meta.len() ^ meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_nanos() as u64;
    if let Some((s, id)) = ids.lock().get(audio) {
        if *s == stamp {
            return Some(id.clone());
        }
    }
    let id = content_id(audio).ok()?;
    ids.lock().insert(audio.to_path_buf(), (stamp, id.clone()));
    Some(id)
}

fn entry_for(library: &Mutex<Library>, ids: &Mutex<std::collections::HashMap<PathBuf, (u64, String)>>, id: &str) -> Result<Entry> {
    let entries = library.lock().entries.clone();
    entries.into_iter().find(|e| id_for(ids, &e.audio).as_deref() == Some(id)).ok_or_else(|| anyhow!("that lesson isn't in the library"))
}

struct Worker {
    settings: Settings,
    rx: Receiver<Msg>,
    tx: Sender<Msg>,
    sink: Sink,
    library: Arc<Mutex<Library>>,
    progress: Arc<Store>,
    cache: PathBuf,
    model: PathBuf,
    generation: Arc<AtomicU64>,
    ids: Arc<Mutex<std::collections::HashMap<PathBuf, (u64, String)>>>,
}

impl Worker {
    fn takes_dir(&self, id: &str) -> Option<PathBuf> {
        self.settings.save_takes.then(|| self.cache.parent().unwrap_or(Path::new(".")).join("takes").join(id))
    }

    fn export_answers(&self, c: &Current, dest: &Path) -> Result<usize> {
        let dir = self.cache.parent().unwrap_or(Path::new(".")).join("takes").join(&c.id);
        answers_wav(&dir, &c.session.lesson.phrases, &c.clip, dest)
    }

    /// Where turn logs go, when diagnostics are on (in settings, or RAM_DEBUG for development).
    fn debug_dir(&self) -> Option<PathBuf> {
        if !self.settings.diagnostics && std::env::var_os("RAM_DEBUG").is_none() {
            return None;
        }
        let d = self.cache.parent().unwrap_or(Path::new(".")).join("logs").join("turns");
        std::fs::create_dir_all(&d).ok().map(|_| d)
    }
}

struct Current {
    id: String,
    session: Session<Devices>,
    clip: Clip,
    done: Vec<bool>,
}

impl Worker {
    fn run(mut self) {
        let mut devices = Some(Devices::new(&self.settings));
        let mut test_mic: Option<(Mic, Instant)> = None;
        let mut cur: Option<Current> = None;
        let mut last = Instant::now();
        loop {
            let busy = cur.as_ref().is_some_and(|c| c.session.mode() != session::Mode::Idle && c.session.mode() != session::Mode::Decide);
            let wait = if busy { Duration::from_millis(15) } else if test_mic.is_some() { Duration::from_millis(50) } else { Duration::from_millis(250) };
            match self.rx.recv_timeout(wait) {
                Ok(Msg::Quit) => break,
                Ok(Msg::Open(id)) => {
                    if let Some(c) = cur.take() {
                        let mut s = c.session;
                        s.command(Cmd::Stop);
                        devices = Some(s.audio);
                    }
                    self.open(id);
                }
                Ok(Msg::Loaded(generation, res)) => {
                    if generation != self.generation.load(Ordering::SeqCst) {
                        continue; // a lesson you've already left
                    }
                    match res {
                        Ok(l) => {
                            let mut d = devices.take().unwrap_or_else(|| Devices::new(&self.settings));
                            d.ensure_player();
                            d.rate_changed = false;
                            let c = self.start_session(*l, d);
                            cur = Some(c);
                        }
                        Err(e) => (self.sink)(Out::Error { message: format!("{e:#}") }),
                    }
                }
                Ok(Msg::Settings(st)) => {
                    let b = Behaviour { turn_end: st.turn_end, playback: st.playback };
                    if let Some(d) = devices.as_mut() {
                        d.choose(&st);
                    }
                    if test_mic.is_some() {
                        test_mic = Mic::open(st.input.as_deref()).ok().map(|m| (m, Instant::now()));
                    }
                    self.settings = st.clone();
                    let dbg = self.debug_dir();
                    if let Some(c) = cur.as_mut() {
                        c.session.takes_dir = self.takes_dir(&c.id);
                    }
                    if let Some(c) = cur.as_mut() {
                        c.session.audio.choose(&st);
                        c.session.behaviour = b;
                        c.session.debug = dbg;
                    }
                }
                Ok(Msg::TestSound) => {
                    let d: &mut Devices = match cur.as_mut() {
                        Some(c) => &mut c.session.audio,
                        None => devices.get_or_insert_with(|| Devices::new(&self.settings)),
                    };
                    d.ensure_player();
                    let r = d.rate;
                    // never wait for the system voice here: the chime if it isn't made yet
                    let (sound, kind) = match TEST_VOICE.get().and_then(|v| v.as_ref()) {
                        Some(c) => (audio::resample(&c.samples, c.rate, r), "voice"),
                        None => (chime(r), "chime"),
                    };
                    let sound = Arc::new(sound);
                    let n = sound.len();
                    d.play(sound, 0, n);
                    log::warn!(
                        "test sound ({kind}, {:.1}s) on {:?} at {r} Hz; speakers open: {}",
                        n as f32 / r as f32,
                        d.output.as_deref().unwrap_or("system default"),
                        d.player.is_some()
                    );
                }
                Ok(Msg::MicTest(on)) => {
                    test_mic = if on {
                        match Mic::open(self.settings.input.as_deref()) {
                            Ok(m) => Some((m, Instant::now())),
                            Err(e) => {
                                (self.sink)(Out::Error { message: format!("Microphone: {e}") });
                                None
                            }
                        }
                    } else {
                        None
                    };
                }
                Ok(Msg::Cmd(c)) => {
                    if let Some(cur) = cur.as_mut() {
                        cur.session.command(c);
                    }
                }
                Ok(m @ (Msg::Mark(_) | Msg::MarkPhrase)) => {
                    if let Some(c) = cur.as_mut() {
                        let words: Vec<usize> = match m {
                            Msg::Mark(w) => w,
                            _ => {
                                let s = &c.session;
                                let p = &s.lesson.phrases[s.phrase()];
                                s.lesson.words[p.a..p.b].iter().filter(|w| w.pdf >= 0).map(|w| w.pdf as usize).collect()
                            }
                        };
                        let marked = &mut c.session.progress.marked;
                        if !words.is_empty() && words.iter().all(|w| marked.contains(w)) {
                            words.iter().for_each(|w| {
                                marked.remove(w);
                            });
                        } else {
                            marked.extend(words);
                        }
                        let list = marked.iter().copied().collect();
                        let _ = self.progress.put(&c.id, &c.session.progress);
                        (self.sink)(Out::Marks { marked: list });
                    }
                }
                Ok(Msg::ExportAnswers(dest, reply)) => {
                    let r = match cur.as_ref() {
                        Some(c) => self.export_answers(c, &dest),
                        None => Err(anyhow!("open a lesson first")),
                    };
                    let _ = reply.send(r);
                }
                Ok(Msg::ResetLesson(id)) => {
                    // start over: done marks and tries go; the words you marked and the time stay
                    let fresh = |p: &LessonProgress| LessonProgress { marked: p.marked.clone(), secs: p.secs, last: p.last, total: p.total, ..LessonProgress::default() };
                    if let Some(c) = cur.as_mut().filter(|c| c.id == id) {
                        c.session.progress = fresh(&c.session.progress);
                        let _ = self.progress.put(&c.id, &c.session.progress);
                        for k in 0..c.done.len() {
                            if c.done[k] {
                                c.done[k] = false;
                                (self.sink)(Out::Done { phrase: k, done: false });
                            }
                        }
                        c.session.command(Cmd::Stop);
                        c.session.restart();
                        self.forward(c);
                    } else {
                        let p = self.progress.get(&id);
                        let _ = self.progress.put(&id, &fresh(&p));
                    }
                }
                Ok(Msg::ResetApp(reply)) => {
                    // stop and close the open lesson; nothing loading may come back to it
                    self.generation.fetch_add(1, Ordering::SeqCst);
                    if let Some(c) = cur.take() {
                        let mut s = c.session;
                        s.command(Cmd::Stop);
                        devices = Some(s.audio);
                    }
                    let _ = reply.send(());
                }
                Ok(Msg::ResetAll) => {
                    let fresh = |p: &LessonProgress| LessonProgress { marked: p.marked.clone(), total: p.total, ..LessonProgress::default() };
                    for (id, p) in self.progress.all() {
                        let _ = self.progress.put(&id, &fresh(&p));
                    }
                    if let Some(c) = cur.as_mut() {
                        c.session.progress = fresh(&c.session.progress);
                        let _ = self.progress.put(&c.id, &c.session.progress);
                        for k in 0..c.done.len() {
                            if c.done[k] {
                                c.done[k] = false;
                                (self.sink)(Out::Done { phrase: k, done: false });
                            }
                        }
                        c.session.command(Cmd::Stop);
                        c.session.restart();
                        self.forward(c);
                    }
                }
                Ok(Msg::ClearMarks(id)) => {
                    if let Some(c) = cur.as_mut().filter(|c| c.id == id) {
                        c.session.progress.marked.clear();
                        let _ = self.progress.put(&c.id, &c.session.progress);
                        (self.sink)(Out::Marks { marked: vec![] });
                    } else {
                        let mut p = self.progress.get(&id);
                        p.marked.clear();
                        let _ = self.progress.put(&id, &p);
                    }
                }
                Ok(Msg::Restore(map, reply)) => {
                    // the open lesson's latest (time not saved yet) goes in first, then the backup on top
                    if let Some(c) = cur.as_ref() {
                        let _ = self.progress.put(&c.id, &c.session.progress);
                    }
                    let r = self.progress.merge_all(&map);
                    if let Some(c) = cur.as_mut() {
                        let p = self.progress.get(&c.id);
                        let marked = p.marked.iter().copied().collect();
                        c.session.set_progress(p);
                        for (k, ph) in c.session.lesson.phrases.iter().enumerate() {
                            let d = c.session.progress.is_done(ph);
                            if d != c.done[k] {
                                c.done[k] = d;
                                (self.sink)(Out::Done { phrase: k, done: d });
                            }
                        }
                        (self.sink)(Out::Marks { marked });
                        self.forward(c);
                    }
                    let _ = reply.send(r);
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            if let Some((m, since)) = &test_mic {
                let x = m.take();
                if !x.is_empty() {
                    let db = audio::frame_db(&x, m.rate).into_iter().fold(-100.0f32, f32::max);
                    (self.sink)(Out::MicLevel { level: ((db + 60.0) / 50.0).clamp(0.0, 1.0) });
                }
                if since.elapsed() > Duration::from_secs(120) {
                    test_mic = None; // don't leave the mic on if the screen was forgotten
                }
            }
            let now = Instant::now();
            let dt = (now - last).as_secs_f64();
            last = now;
            if let Some(c) = cur.as_mut() {
                // the speakers changed rate (new device): remake the teacher at the new rate
                if c.session.audio.rate_changed {
                    c.session.audio.rate_changed = false;
                    let r = c.session.audio.rate;
                    c.session.lesson.teacher = Arc::new(audio::resample(&c.clip.samples, c.clip.rate, r));
                }
                c.session.tick(dt);
                self.forward(c);
            }
        }
    }

    fn forward(&self, c: &mut Current) {
        for ev in c.session.drain() {
            if let session::Event::Progress = ev {
                let p = &mut c.session.progress;
                p.total = Some(c.session.lesson.phrases.iter().filter(|x| x.in_pdf).count());
                for (k, ph) in c.session.lesson.phrases.iter().enumerate() {
                    let d = p.is_done(ph);
                    if d != c.done[k] {
                        c.done[k] = d;
                        (self.sink)(Out::Done { phrase: k, done: d });
                    }
                    let key = LessonProgress::key(ph);
                    if p.phrases.contains_key(&key) {
                        let t = phrase_text(ph, &c.session.lesson.words, &c.session.lesson.pdf);
                        p.set_text(&key, &t);
                    }
                }
                if let Err(e) = self.progress.put(&c.id, p) {
                    (self.sink)(Out::Error { message: format!("Couldn't save progress: {e}") });
                }
                continue;
            }
            (self.sink)(Out::Session { event: ev });
        }
    }

    fn start_session(&self, l: Loaded, devices: Devices) -> Current {
        let mut words = l.words.clone();
        let mut pdfw = l.pdf.clone();
        align::align_to_pdf(&mut words, &mut pdfw);
        let phrases = build_phrases(&words, &pdfw);
        let prog = self.progress.get(&l.id);
        let mut lesson_view = view(&l, &words, &pdfw, &phrases, &prog);
        let done = phrases.iter().map(|p| prog.is_done(p)).collect();
        let teacher = Arc::new(audio::resample(&l.clip.samples, l.clip.rate, devices.rate));
        let lesson = Lesson { teacher, words, pdf: pdfw, phrases };
        lesson_view.marked = prog.marked.iter().copied().collect();
        let mut session = Session::new(devices, lesson, prog);
        session.takes_dir = self.takes_dir(&l.id);
        session.debug = self.debug_dir();
        session.behaviour = Behaviour { turn_end: self.settings.turn_end, playback: self.settings.playback };
        let mut c = Current { id: l.id, session, clip: l.clip, done };
        // the PDF and the practice panel arrive together (the panel needs the session's first state)
        (self.sink)(Out::Lesson { lesson: lesson_view });
        self.forward(&mut c);
        c
    }

    fn open(&self, id: String) {
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let entry = match entry_for(&self.library, &self.ids, &id) {
            Ok(e) => e,
            Err(e) => return (self.sink)(Out::Error { message: e.to_string() }),
        };
        let (tx, sink, cache, model, gen_now) = (self.tx.clone(), self.sink.clone(), self.cache.clone(), self.model.clone(), self.generation.clone());
        std::thread::Builder::new()
            .name("load".into())
            .stack_size(16 << 20)
            .spawn(move || {
                let report = |stage: &str, p: f32| sink(Out::Loading { id: id.clone(), stage: stage.into(), progress: p });
                let g = gen_now.clone();
                let cancelled: Arc<dyn Fn() -> bool + Send + Sync> = Arc::new(move || g.load(Ordering::SeqCst) != generation);
                let res = load_lesson(&entry, &id, &cache, &model, &report, cancelled).map(Box::new);
                let _ = tx.send(Msg::Loaded(generation, res));
            })
            .expect("thread");
    }
}

/// Two soft notes (E5, A5) with a gentle fade, ~0.7 s.
fn chime(rate: u32) -> Vec<f32> {
    let note = |f: f32, secs: f32| -> Vec<f32> {
        let n = (rate as f32 * secs) as usize;
        (0..n)
            .map(|i| {
                let t = i as f32 / rate as f32;
                let env = (t / 0.01).min(1.0) * (-t * 5.0).exp();
                0.35 * env * (std::f32::consts::TAU * f * t).sin()
            })
            .collect()
    };
    let mut v = note(659.3, 0.3);
    v.extend(note(880.0, 0.45));
    v
}

/// Say `text` with the computer's own voice (offline: macOS `say`, Windows speech, Linux espeak-ng/pico2wave).
/// "Repeat after me" in the system's voice, made once in the background when the app starts (the
/// system voice takes a second or more each time): the speaker test plays it right away.
static TEST_VOICE: std::sync::OnceLock<Option<Clip>> = std::sync::OnceLock::new();

fn spoken(text: &str) -> Option<Clip> {
    use std::process::Command;
    let path = std::env::temp_dir().join(format!("ram-voice-{}.wav", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let p = path.to_string_lossy().into_owned();
    let ok = if cfg!(target_os = "macos") {
        Command::new("say").args(["-r", "170", "--data-format=LEI16@22050", "-o", &p, text]).status().ok()?.success()
    } else if cfg!(target_os = "windows") {
        let script = format!(
            "Add-Type -AssemblyName System.Speech; $s = New-Object System.Speech.Synthesis.SpeechSynthesizer; $s.SetOutputToWaveFile('{}'); $s.Speak('{}'); $s.Dispose()",
            p.replace('\'', "''"),
            text
        );
        let mut c = Command::new("powershell");
        c.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            c.creation_flags(0x0800_0000); // no console window
        }
        c.status().ok()?.success()
    } else {
        Command::new("espeak-ng").args(["-w", &p, text]).status().map(|s| s.success()).unwrap_or(false)
            || Command::new("pico2wave").args(["-w", &p, text]).status().map(|s| s.success()).unwrap_or(false)
    };
    let clip = if ok { audio::decode_file(&path).ok() } else { None };
    let _ = std::fs::remove_file(&path);
    clip
}

/// Per phrase you practised (in order): the teacher, 0.4 s, your latest take, 1.2 s. One 16 kHz mono WAV.
pub fn answers_wav(takes: &Path, phrases: &[align::Phrase], teacher: &Clip, dest: &Path) -> Result<usize> {
    const R: u32 = 16000;
    let mut latest: std::collections::HashMap<String, (u64, PathBuf)> = Default::default();
    for e in std::fs::read_dir(takes).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let Some(stem) = name.strip_suffix(".wav") else { continue };
        let Some((key, at)) = stem.rsplit_once('-') else { continue };
        let at: u64 = at.parse().unwrap_or(0);
        if latest.get(key).is_none_or(|(t, _)| at > *t) {
            latest.insert(key.to_string(), (at, e.path()));
        }
    }
    if latest.is_empty() {
        return Err(anyhow!("No saved answers for this lesson yet. Practise a few phrases first."));
    }
    let teacher16 = audio::resample(&teacher.samples, teacher.rate, R);
    let gap = |s: f64| vec![0.0f32; (s * R as f64) as usize];
    let (mut out, mut n) = (Vec::<f32>::new(), 0);
    for p in phrases {
        let Some((_, path)) = latest.get(&LessonProgress::key(p)) else { continue };
        let Ok(take) = audio::decode_file(path) else { continue };
        let a = ((p.start - 0.05).max(0.0) * R as f64) as usize;
        let b = ((p.stop * R as f64) as usize).min(teacher16.len());
        out.extend_from_slice(&teacher16[a.min(b)..b]);
        out.extend(gap(0.4));
        out.extend(audio::resample(&take.samples, take.rate, R));
        out.extend(gap(1.2));
        n += 1;
    }
    audio::write_wav(dest, &out, R)?;
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reset_app_goes_back_to_a_fresh_install() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        let course = dir.path().join("course");
        std::fs::create_dir_all(&course).unwrap();
        std::fs::write(course.join("A.m4a"), b"audio").unwrap();
        std::fs::write(course.join("A.pdf"), b"%PDF").unwrap();
        for d in ["transcripts", "takes/x", "imported/zip", "logs"] {
            std::fs::create_dir_all(data.join(d)).unwrap();
        }
        std::fs::write(data.join("logs/app.log"), b"log").unwrap();
        let pdfium = std::env::var_os("RAM_PDFIUM_DIR").map(PathBuf::from);
        let paths = Paths { data: data.clone(), model: dir.path().join("none.bin"), pdfium_dir: pdfium };
        let e = Engine::start(paths, Arc::new(|_| {})).unwrap();
        e.add(&[course.clone()]).unwrap();
        e.progress.put("abc", &LessonProgress { pos: 3, ..Default::default() }).unwrap();
        e.set_settings(Settings { save_takes: false, ..Settings::default() }).unwrap();
        assert_eq!(e.lessons().len(), 1);

        e.reset_app().unwrap();
        assert!(e.lessons().is_empty(), "no lessons");
        assert!(e.library.lock().folders.is_empty(), "no folders to look in");
        assert_eq!(e.progress.get("abc"), LessonProgress::default(), "no progress");
        assert_eq!(e.settings(), Settings::default(), "default settings");
        for d in ["transcripts", "takes", "imported"] {
            assert!(!data.join(d).exists(), "{d} deleted");
        }
        assert!(data.join("logs/app.log").exists(), "logs kept");
        assert!(course.join("A.m4a").exists() && course.join("A.pdf").exists(), "lesson files untouched");
        // and it stays that way after a restart
        drop(e);
        assert!(Library::open(data.join("library.json")).entries.is_empty());
    }

    #[test]
    fn unzips_a_course_into_its_own_folder() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("My Course.zip");
        {
            let mut z = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
            let o = zip::write::SimpleFileOptions::default();
            for (name, data) in [("My Course/L1/A.m4a", "a"), ("My Course/L1/A.pdf", "p"), ("__MACOSX/My Course/._A.m4a", "junk"), ("../escape.txt", "no")] {
                z.start_file(name, o).unwrap();
                z.write_all(data.as_bytes()).unwrap();
            }
            z.finish().unwrap();
        }
        let base = dir.path().join("imported");
        let root = unzip(&zip_path, &base).unwrap();
        assert_eq!(root, base.join("My Course").join("My Course"), "the zip's own top folder");
        assert!(root.join("L1/A.m4a").is_file());
        assert!(!base.join("My Course/__MACOSX").exists());
        assert!(!dir.path().join("escape.txt").exists() && !base.join("escape.txt").exists());
        // the same zip again: same folder, no copy
        assert_eq!(unzip(&zip_path, &base).unwrap(), root);
        assert_eq!(std::fs::read_dir(&base).unwrap().count(), 1);
    }

    #[test]
    fn answers_file_has_teacher_then_latest_take_per_phrase() {
        let dir = tempfile::tempdir().unwrap();
        let takes = dir.path().join("takes");
        std::fs::create_dir_all(&takes).unwrap();
        let ph = |s: f64| align::Phrase { a: 0, b: 1, start: s, end: s + 1.0, stop: s + 1.2, in_pdf: true };
        let phrases = vec![ph(1.0), ph(3.0)];
        // phrase 1: an old 0.5 s take and a newer 1 s take; phrase 2: none
        audio::write_wav(&takes.join("100-1000.wav"), &vec![0.1; 8000], 16000).unwrap();
        audio::write_wav(&takes.join("100-2000.wav"), &vec![0.1; 16000], 16000).unwrap();
        let teacher = Clip { rate: 16000, samples: vec![0.2; 16000 * 5] };
        let dest = dir.path().join("answers.wav");
        assert_eq!(answers_wav(&takes, &phrases, &teacher, &dest).unwrap(), 1);
        let got = audio::decode_file(&dest).unwrap();
        let secs = got.samples.len() as f64 / 16000.0;
        assert!((secs - (1.25 + 0.4 + 1.0 + 1.2)).abs() < 0.02, "{secs}");
        let empty = dir.path().join("none");
        assert!(answers_wav(&empty, &phrases, &teacher, &dest).is_err());
    }
}
