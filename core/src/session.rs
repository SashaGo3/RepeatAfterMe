//! Practising a lesson: hear the teacher, say it, hear both, decide. No checking: you are the judge.
//!
//! The session only talks to audio through [`Audio`], so the whole loop runs in tests on a fake clock.

use crate::align::{PdfWord, Phrase, SpokenWord};
use crate::audio::{frame_db, resample};
use crate::settings::{Playback, TurnEnd};
use crate::progress::LessonProgress;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// Speakers + microphone, as the session needs them.
pub trait Audio {
    fn out_rate(&self) -> u32;
    fn play(&mut self, buf: Arc<Vec<f32>>, from: usize, to: usize);
    fn stop(&mut self);
    fn is_playing(&self) -> bool;
    /// position in the buffer being played, in samples
    fn position(&self) -> usize;
    /// opens the microphone (if it isn't open) and returns its rate
    fn mic_open(&mut self) -> anyhow::Result<u32>;
    fn mic_close(&mut self);
    /// samples heard since the last call
    fn mic_take(&mut self) -> Vec<f32>;
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    /// stopped on a phrase
    Idle,
    /// the teacher says the phrase
    Teacher,
    /// your turn: the mic listens
    Turn,
    /// teacher, then you
    Compare,
    /// your take alone
    Mine,
    /// your call: good (next) or again
    Decide,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum After {
    Turn,
    Decide,
    Idle,
    /// listen-only: on to the next phrase
    Next,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Cmd {
    /// play / stop / "done talking" / replay, depending on the moment
    Space,
    /// good: mark done, go on
    Good,
    /// try again
    Again,
    Prev,
    Next,
    Goto(usize),
    /// hear the teacher only
    Teacher,
    /// hear my take only
    Mine,
    /// stop everything
    Stop,
    ToggleListenOnly,
    /// which phrases to go through: all → not done → marked → all
    CycleFilter,
    SetFilter(Filter),
}

/// Which phrases Back/Next/Good go through.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Filter {
    #[default]
    All,
    /// skip the ones you already called good
    NotDone,
    /// only the ones with words you marked
    Marked,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub mode: Mode,
    pub phrase: usize,
    pub phrases: usize,
    pub listen_only: bool,
    pub filter: Filter,
    pub has_take: bool,
    /// a short note for the user ("Didn't hear you", "Lesson done")
    pub note: Option<String>,
    pub done: usize,
    pub to_repeat: usize,
    pub tries: u32,
    pub phrase_done: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Event {
    State(Snapshot),
    /// during the teacher: the spoken word being said (index into words) and the time
    Word { word: i32, t: f64 },
    /// during your turn: mic loudness 0..1 and whether you're heard
    Level { level: f32, voice: bool },
    /// progress changed: save it
    Progress,
}

pub struct Lesson {
    /// the teacher at the speaker rate
    pub teacher: Arc<Vec<f32>>,
    pub words: Vec<SpokenWord>,
    pub pdf: Vec<PdfWord>,
    pub phrases: Vec<Phrase>,
}

struct Take {
    /// at the speaker rate
    out: Arc<Vec<f32>>,
}

/// Hears your turn in 10 ms frames and decides when you're done.
///
/// Two levels: you *start* talking when it's 12 dB over the room, and you're *still* talking while it's
/// 6 dB over (so the soft ends of words, a final "s" or "th", count as speech and aren't cut). How long a
/// pause ends your turn depends on how far you are: once you've talked about as long as the teacher did,
/// a short pause (the teacher's own longest pause + a bit) is enough; before that, pauses are you thinking.
struct Listener {
    rate: u32,
    hop: usize,
    pending: Vec<f32>,
    heard: Vec<f32>,
    frames: usize,
    floor: f32,
    smooth: f32,
    loud_run: usize,
    /// first frame of your voice
    started: Option<usize>,
    /// last frame that was still voice
    last_voice: usize,
    quiet: usize,
    /// the teacher's phrase length, in frames
    expected: usize,
    short_pause: usize,
    long_pause: usize,
    max_frames: usize,
    no_voice_frames: usize,
    manual: bool,
    /// loudness of every frame (for the debug log)
    dbs: Vec<f32>,
    end: &'static str,
    /// voice-band filter: rumble (fans, hum, desk knocks) is cut before measuring loudness
    hp: crate::audio::HighPass,
}

const ON_OVER_FLOOR: f32 = 12.0;
const OFF_OVER_FLOOR: f32 = 6.0;
/// kept before your first sound / after your last
const PRE_FRAMES: usize = 15;
const POST_FRAMES: usize = 25;

enum Heard {
    Waiting,
    Done,
    Nothing,
}

impl Listener {
    fn new(rate: u32, phrase_secs: f64, teacher_gap: f64, words: usize, end: TurnEnd) -> Self {
        let patient = if end == TurnEnd::Patient { 1.5 } else { 1.0 };
        let _ = teacher_gap;
        // one or two words: done as soon as you stop (0.35 s).
        // a sentence: a breath mid-way is fine — the allowed pause grows with the sentence (0.6–2 s) —
        // and once you've said about all of it, a short 0.5 s pause ends it.
        let (short, long) = if words <= 2 {
            (0.35, 0.35)
        } else {
            (0.5, (0.4 * phrase_secs + 0.3).clamp(0.6, 2.0))
        };
        let (short, long) = (short * patient, long * patient);
        let max = (phrase_secs * 3.0 + 3.0).clamp(6.0, 30.0);
        Self {
            rate,
            hop: (rate / 100).max(1) as usize,
            pending: vec![],
            heard: vec![],
            frames: 0,
            floor: -60.0,
            smooth: -100.0,
            loud_run: 0,
            started: None,
            last_voice: 0,
            quiet: 0,
            expected: (phrase_secs * 100.0) as usize,
            short_pause: (short * 100.0) as usize,
            long_pause: (long * 100.0) as usize,
            max_frames: if end == TurnEnd::Manual { 6000 } else { (max * 100.0) as usize },
            no_voice_frames: if end == TurnEnd::Manual { 3000 } else { 800 },
            manual: end == TurnEnd::Manual,
            dbs: vec![],
            end: "",
            hp: crate::audio::HighPass::new(rate, 200.0),
        }
    }

    fn on(&self) -> f32 {
        (self.floor + ON_OVER_FLOOR).max(-52.0)
    }
    fn off(&self) -> f32 {
        (self.floor + OFF_OVER_FLOOR).max(-60.0)
    }

    /// Feed samples; returns the latest level (0..1), whether you're heard now, and what we know.
    fn feed(&mut self, x: &[f32]) -> (f32, bool, Heard) {
        self.pending.extend_from_slice(x);
        self.heard.extend_from_slice(x);
        let mut level_db = -100.0;
        while self.pending.len() >= self.hop {
            let mut frame: Vec<f32> = self.pending.drain(..self.hop).collect();
            self.hp.run(&mut frame);
            let db = frame_db(&frame, self.rate)[0];
            level_db = db;
            self.frames += 1;
            self.dbs.push(db);
            // the room's level: the quietest (smoothed, so one silent frame inside a word doesn't count)
            self.smooth = if self.frames == 1 { db } else { 0.6 * self.smooth + 0.4 * db };
            if self.frames <= 12 {
                self.floor = if self.frames == 1 { self.smooth } else { self.floor.min(self.smooth) };
            } else if self.frames % 5 == 0 {
                // the room = the quietest tenth of the last 3 s. Re-learned all the time, so a mic that is
                // still warming up when the turn starts, or a fan that comes on, can't pass for your voice.
                let recent = &self.dbs[self.dbs.len().saturating_sub(300)..];
                let mut v: Vec<f32> = recent.to_vec();
                v.sort_by(|a, b| a.partial_cmp(b).unwrap());
                self.floor = v[v.len() / 10];
            }
            match self.started {
                None => {
                    self.loud_run = if db > self.on() { self.loud_run + 1 } else { 0 };
                    if self.loud_run >= 3 {
                        let first = self.frames - 3;
                        self.started = Some(first);
                        self.last_voice = self.frames;
                    } else if self.frames >= self.no_voice_frames {
                        self.end = "no voice";
                        return (0.0, false, Heard::Nothing);
                    }
                }
                Some(s) => {
                    if db > self.off() {
                        self.quiet = 0;
                        self.last_voice = self.frames;
                    } else {
                        self.quiet += 1;
                    }
                    let said = self.last_voice - s;
                    // said about as much as the teacher (people often say it faster): a short pause is the end
                    let pause = if said * 20 >= self.expected * 17 { self.short_pause } else { self.long_pause };
                    if (!self.manual && self.quiet >= pause) || self.frames - s >= self.max_frames {
                        self.end = if self.frames - s >= self.max_frames { "too long" } else if pause == self.short_pause { "short pause" } else { "long pause" };
                        return (0.0, false, Heard::Done);
                    }
                }
            }
        }
        let level = ((level_db + 60.0) / 50.0).clamp(0.0, 1.0);
        (level, self.started.is_some() && self.quiet == 0, Heard::Waiting)
    }

    /// Your take: from a moment before your first sound to a moment after your last.
    fn take(&self) -> &[f32] {
        let Some(s) = self.started else { return &[] };
        let a = s.saturating_sub(PRE_FRAMES) * self.hop;
        let b = ((self.last_voice + POST_FRAMES) * self.hop).min(self.heard.len());
        &self.heard[a.min(b)..b]
    }
}

/// Replay recorded mic audio through the turn detector (for tuning on real takes).
pub fn replay_turn(x: &[f32], rate: u32, phrase_secs: f64, teacher_gap: f64, words: usize, end: TurnEnd) -> String {
    let mut l = Listener::new(rate, phrase_secs, teacher_gap, words, end);
    let mut out = "never ended".to_string();
    for chunk in x.chunks((rate / 50) as usize) {
        let (_, _, h) = l.feed(chunk);
        if !matches!(h, Heard::Waiting) {
            out = format!("ended at {:.2}s ({})", l.frames as f64 / 100.0, l.end);
            break;
        }
    }
    format!("floor {:.1} dB, on {:.1}, off {:.1}; voice {:?}–{:.2}s; {}; take {:.2}s",
        l.floor, l.on(), l.off(), l.started.map(|f| f as f64 / 100.0), l.last_voice as f64 / 100.0, out,
        l.take().len() as f64 / rate as f64)
}

/// How the session behaves (from the settings).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Behaviour {
    pub turn_end: TurnEnd,
    pub playback: Playback,
}

pub struct Session<A: Audio> {
    pub audio: A,
    pub lesson: Lesson,
    pub progress: LessonProgress,
    k: usize,
    mode: Mode,
    after: After,
    listen_only: bool,
    filter: Filter,
    /// how long you've been deciding (only the first minute counts as practice)
    deciding_for: f64,
    takes: HashMap<usize, Take>,
    listener: Option<Listener>,
    /// Compare: 0 teacher, 1 gap, 2 me; gap ends at this many ticks
    stage: u8,
    gap_left: f64,
    note: Option<String>,
    last_word: i32,
    events: Vec<Event>,
    pub behaviour: Behaviour,
    /// RAM_DEBUG: a folder for turn logs and recordings
    pub debug: Option<std::path::PathBuf>,
    /// where every take is kept (setting "save my answers"), None = not kept
    pub takes_dir: Option<std::path::PathBuf>,
    born: std::time::Instant,
    turn_no: usize,
}

impl<A: Audio> Session<A> {
    pub fn new(audio: A, lesson: Lesson, progress: LessonProgress) -> Self {
        let n = lesson.phrases.len();
        let k = progress.pos.min(n.saturating_sub(1));
        let mut s = Self {
            audio,
            lesson,
            progress,
            k,
            mode: Mode::Idle,
            after: After::Idle,
            listen_only: false,
            filter: Filter::All,
            deciding_for: 0.0,
            takes: HashMap::new(),
            listener: None,
            stage: 0,
            gap_left: 0.0,
            note: None,
            last_word: -1,
            events: vec![],
            behaviour: Behaviour::default(),
            debug: None,
            takes_dir: None,
            born: std::time::Instant::now(),
            turn_no: 0,
        };
        if !s.lesson.phrases.is_empty() && !s.lesson.phrases[s.k].in_pdf {
            s.k = s.next_from(s.k).unwrap_or(s.k);
        }
        s.emit_state();
        s
    }

    /// Back to the first phrase to practise (after starting the lesson over), stopped.
    pub fn restart(&mut self) {
        self.takes.clear();
        self.note = None;
        self.k = (0..self.lesson.phrases.len()).find(|&i| self.wanted(i)).unwrap_or(0);
        self.set_mode(Mode::Idle);
        self.emit_state();
    }

    /// Use progress changed from outside (restored from a backup).
    pub fn set_progress(&mut self, p: crate::progress::LessonProgress) {
        self.progress = p;
        self.emit_state();
    }

    pub fn phrase(&self) -> usize {
        self.k
    }
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Events since the last call (state changes, the word being said, mic level).
    pub fn drain(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    fn key(&self, k: usize) -> String {
        LessonProgress::key(&self.lesson.phrases[k])
    }

    pub fn snapshot(&self) -> Snapshot {
        let ph = &self.lesson.phrases;
        let rec = ph.get(self.k).and_then(|p| self.progress.phrases.get(&LessonProgress::key(p)));
        Snapshot {
            mode: self.mode,
            phrase: self.k,
            phrases: ph.len(),
            listen_only: self.listen_only,
            filter: self.filter,
            has_take: self.takes.contains_key(&self.k),
            note: self.note.clone(),
            done: ph.iter().filter(|p| p.in_pdf && self.progress.is_done(p)).count(),
            to_repeat: ph.iter().filter(|p| p.in_pdf).count(),
            tries: rec.map(|r| r.tries).unwrap_or(0),
            phrase_done: rec.map(|r| r.done).unwrap_or(false),
        }
    }

    fn emit_state(&mut self) {
        let s = self.snapshot();
        self.events.push(Event::State(s));
    }

    fn log(&self, line: &str) {
        if let Some(d) = &self.debug {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(d.join("events.log")) {
                let _ = writeln!(f, "{:9.3} phrase {:3} {}", self.born.elapsed().as_secs_f64(), self.k, line);
            }
        }
    }

    /// Everything about one turn, for tuning: numbers in turns.jsonl, sound in NNN-*.wav.
    fn dump_turn(&mut self, l: &Listener, reason: &str) {
        let Some(dir) = self.debug.clone() else { return };
        // number turns across app launches, so recordings never overwrite each other
        let before = std::fs::read_to_string(dir.join("turns.jsonl")).map(|t| t.lines().count()).unwrap_or(0);
        self.turn_no = self.turn_no.max(before) + 1;
        let n = self.turn_no;
        let p = self.lesson.phrases[self.k].clone();
        let words = &self.lesson.words[p.a..p.b.min(self.lesson.words.len())];
        let text = crate::align::phrase_text(&p, &self.lesson.words, &self.lesson.pdf);
        let gap = words.windows(2).map(|w| w[1].s - w[0].e).fold(0.0, f64::max);
        let r = self.audio.out_rate();
        let (ta, tb) = self.teacher_range(self.k);
        let teacher = &self.lesson.teacher[ta.min(self.lesson.teacher.len())..tb.min(self.lesson.teacher.len())];
        let _ = crate::audio::write_wav(&dir.join(format!("{n:03}-teacher.wav")), teacher, r);
        let _ = crate::audio::write_wav(&dir.join(format!("{n:03}-heard.wav")), &l.heard, l.rate);
        let _ = crate::audio::write_wav(&dir.join(format!("{n:03}-take.wav")), l.take(), l.rate);
        let fr = |f: usize| f as f64 / 100.0;
        let line = serde_json::json!({
            "turn": n, "phrase": self.k, "text": text,
            "teacherSecs": p.end - p.start, "teacherMaxGap": gap, "words": p.b - p.a,
            "ended": reason, "listened": fr(l.frames),
            "floorDb": l.floor, "onDb": l.on(), "offDb": l.off(),
            "voiceStart": l.started.map(fr), "voiceEnd": l.started.map(|_| fr(l.last_voice)),
            "waitedAfterVoice": l.started.map(|_| fr(l.frames.saturating_sub(l.last_voice))),
            "shortPause": fr(l.short_pause), "longPause": fr(l.long_pause),
            "takeSecs": l.take().len() as f64 / l.rate as f64, "micRate": l.rate,
            "db": l.dbs.iter().map(|d| (d * 10.0).round() / 10.0).collect::<Vec<_>>(),
        });
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("turns.jsonl")) {
            let _ = writeln!(f, "{line}");
        }
        self.log(&format!("turn {n} ended: {reason}; voice {:?}-{:.2}s, waited {:.2}s after, take {:.2}s",
            l.started.map(fr), fr(l.last_voice), fr(l.frames.saturating_sub(l.last_voice)), l.take().len() as f64 / l.rate as f64));
    }

    /// Practice time: everything but being stopped; deciding counts for its first minute only
    /// (so walking away mid-decision doesn't add up).
    fn count_time(&mut self, dt: f64) {
        let counts = match self.mode {
            Mode::Idle => false,
            Mode::Decide => {
                self.deciding_for += dt;
                self.deciding_for <= 60.0
            }
            _ => true,
        };
        if counts && dt > 0.0 && dt < 1.0 {
            self.progress.secs += dt;
        }
    }

    fn set_mode(&mut self, m: Mode) {
        if m != self.mode {
            self.log(&format!("mode {:?} → {:?}", self.mode, m));
            if m == Mode::Decide {
                self.deciding_for = 0.0;
            }
            // a stretch of practice ended: save the time (and when)
            if matches!(m, Mode::Idle | Mode::Decide) && self.mode != Mode::Idle && self.mode != Mode::Decide {
                self.progress.last = crate::progress::now();
                self.events.push(Event::Progress);
            }
        }
        self.mode = m;
        self.emit_state();
    }

    /// Whether phrase i has any printed word you marked.
    pub fn phrase_marked(&self, i: usize) -> bool {
        let p = &self.lesson.phrases[i];
        let words = &self.lesson.words[p.a.min(self.lesson.words.len())..p.b.min(self.lesson.words.len())];
        words.iter().any(|w| w.pdf >= 0 && self.progress.marked.contains(&(w.pdf as usize)))
    }

    /// Whether phrase i is one to go to: listen-only plays everything, else only what's printed;
    /// the filter can keep just the ones not done yet, or just the marked ones.
    fn wanted(&self, i: usize) -> bool {
        (self.listen_only || self.lesson.phrases[i].in_pdf)
            && match self.filter {
                Filter::All => true,
                Filter::NotDone => !self.progress.is_done(&self.lesson.phrases[i]),
                Filter::Marked => self.phrase_marked(i),
            }
    }

    /// The next phrase to practise after k.
    fn next_from(&self, k: usize) -> Option<usize> {
        ((k + 1)..self.lesson.phrases.len()).find(|&i| self.wanted(i))
    }
    fn prev_from(&self, k: usize) -> Option<usize> {
        (0..k).rev().find(|&i| self.wanted(i))
    }

    fn end_note(&self) -> String {
        match (self.filter, self.listen_only) {
            (Filter::Marked, _) => "End of the marked phrases.".into(),
            (Filter::NotDone, _) => "End of the phrases still to do.".into(),
            (Filter::All, true) => "End of the lesson.".into(),
            (Filter::All, false) => "Lesson done!".into(),
        }
    }

    fn teacher_range(&self, k: usize) -> (usize, usize) {
        let p = &self.lesson.phrases[k];
        let r = self.audio.out_rate() as f64;
        // start a hair early so the first consonant isn't clipped
        (((p.start - 0.05).max(0.0) * r) as usize, (p.stop * r) as usize)
    }

    fn play_teacher(&mut self, k: usize, after: After) {
        if self.lesson.phrases.is_empty() {
            return;
        }
        self.k = k;
        self.after = after;
        self.note = None;
        self.last_word = -1;
        let (a, b) = self.teacher_range(k);
        self.audio.play(self.lesson.teacher.clone(), a, b);
        if after == After::Turn {
            // open the mic now: it is ready (and warm) when the teacher stops
            if let Err(e) = self.audio.mic_open() {
                self.note = Some(format!("Microphone: {e}"));
                self.after = After::Decide;
            }
        }
        if self.progress.pos != k {
            self.progress.pos = k;
            self.events.push(Event::Progress);
        }
        self.set_mode(Mode::Teacher);
    }

    fn start_turn(&mut self) {
        let rate = match self.audio.mic_open() {
            Ok(r) => r,
            Err(e) => {
                self.note = Some(format!("Microphone: {e}"));
                self.set_mode(Mode::Decide);
                return;
            }
        };
        // what the mic heard while the teacher spoke, except its last 150 ms (in case you started early)
        let before = self.audio.mic_take();
        let keep = (rate as usize * 15 / 100).min(before.len());
        let p = &self.lesson.phrases[self.k];
        let words = &self.lesson.words[p.a..p.b.min(self.lesson.words.len())];
        let gap = words.windows(2).map(|w| w[1].s - w[0].e).fold(0.0, f64::max);
        let mut l = Listener::new(rate, p.end - p.start, gap, p.b - p.a, self.behaviour.turn_end);
        l.feed(&before[before.len() - keep..]);
        self.listener = Some(l);
        self.set_mode(Mode::Turn);
    }

    fn finish_turn(&mut self, heard_something: bool, reason: &str) {
        let Some(l) = self.listener.take() else { return };
        self.audio.mic_close();
        let reason = if l.end.is_empty() { reason } else { l.end };
        self.dump_turn(&l, reason);
        if !heard_something || l.started.is_none() {
            self.note = Some("Didn't hear you. Check the microphone, then press R to try again.".into());
            self.set_mode(Mode::Decide);
            return;
        }
        if let Some(dir) = &self.takes_dir {
            // kept small: 16 kHz mono; named <phrase key>-<unix time>.wav
            let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
            let small = resample(l.take(), l.rate, 16000);
            if std::fs::create_dir_all(dir).is_ok() {
                let _ = crate::audio::write_wav(&dir.join(format!("{}-{at}.wav", self.key(self.k))), &small, 16000);
            }
        }
        let out = resample(l.take(), l.rate, self.audio.out_rate());
        self.takes.insert(self.k, Take { out: Arc::new(out) });
        let key = self.key(self.k);
        self.progress.tried(&key, &self.lesson.phrases[self.k]);
        self.events.push(Event::Progress);
        match self.behaviour.playback {
            Playback::TeacherThenMe => self.start_compare(),
            Playback::MeOnly => {
                self.play_mine();
                self.set_mode(Mode::Mine);
            }
            Playback::Nothing => self.set_mode(Mode::Decide),
        }
    }

    fn start_compare(&mut self) {
        let (a, b) = self.teacher_range(self.k);
        self.audio.play(self.lesson.teacher.clone(), a, b);
        self.stage = 0;
        self.last_word = -1;
        self.set_mode(Mode::Compare);
    }

    fn play_mine(&mut self) -> bool {
        let Some(t) = self.takes.get(&self.k) else { return false };
        let buf = t.out.clone();
        let n = buf.len();
        self.audio.play(buf, 0, n);
        true
    }

    fn stop_all(&mut self) {
        self.audio.stop();
        if let Some(l) = self.listener.take() {
            self.audio.mic_close();
            self.dump_turn(&l, "cancelled");
        }
    }

    fn practise(&mut self, k: usize) {
        self.stop_all();
        let after = if self.listen_only { After::Next } else { After::Turn };
        self.play_teacher(k, after);
    }

    pub fn command(&mut self, c: Cmd) {
        self.log(&format!("key {c:?} (in {:?})", self.mode));
        let n = self.lesson.phrases.len();
        if n == 0 {
            return;
        }
        match c {
            Cmd::Space => match self.mode {
                Mode::Idle => self.practise(self.k),
                Mode::Teacher if self.after == After::Next => {
                    self.stop_all();
                    self.set_mode(Mode::Idle);
                }
                Mode::Teacher => {
                    // skip the rest of the teacher: your turn now
                    self.audio.stop();
                    if self.after == After::Turn {
                        self.start_turn();
                    } else {
                        self.set_mode(if self.takes.contains_key(&self.k) { Mode::Decide } else { Mode::Idle });
                    }
                }
                Mode::Turn => {
                    let heard = self.listener.as_ref().is_some_and(|l| l.started.is_some());
                    self.finish_turn(heard, "space");
                }
                Mode::Compare | Mode::Mine => {
                    self.audio.stop();
                    self.set_mode(Mode::Decide);
                }
                Mode::Decide => {
                    if self.takes.contains_key(&self.k) {
                        self.start_compare();
                    } else {
                        self.practise(self.k);
                    }
                }
            },
            Cmd::Good => {
                let key = self.key(self.k);
                if self.lesson.phrases[self.k].in_pdf {
                    self.progress.mark_done(&key, &self.lesson.phrases[self.k]);
                    self.events.push(Event::Progress);
                }
                match self.next_from(self.k) {
                    Some(k) => self.practise(k),
                    None => {
                        self.stop_all();
                        self.note = Some(self.end_note());
                        self.set_mode(Mode::Idle);
                    }
                }
            }
            Cmd::Again => {
                let k = self.k;
                self.stop_all();
                self.play_teacher(k, After::Turn);
            }
            Cmd::Next => {
                if let Some(k) = self.next_from(self.k) {
                    self.practise(k);
                }
            }
            Cmd::Prev => {
                if let Some(k) = self.prev_from(self.k) {
                    self.practise(k);
                }
            }
            Cmd::Goto(k) if k < n => self.practise(k),
            Cmd::Goto(_) => {}
            Cmd::Teacher => {
                let k = self.k;
                self.stop_all();
                let after = if self.takes.contains_key(&k) { After::Decide } else { After::Idle };
                self.play_teacher(k, after);
            }
            Cmd::Mine => {
                self.stop_all();
                if self.play_mine() {
                    self.set_mode(Mode::Mine);
                } else {
                    self.note = Some("No take of this phrase yet.".into());
                    self.emit_state();
                }
            }
            Cmd::Stop => {
                self.stop_all();
                self.set_mode(Mode::Idle);
            }
            Cmd::ToggleListenOnly => {
                self.listen_only = !self.listen_only;
                if self.mode == Mode::Teacher {
                    self.after = if self.listen_only { After::Next } else { After::Turn };
                    if !self.listen_only {
                        let _ = self.audio.mic_open();
                    }
                } else if self.mode == Mode::Turn && self.listen_only {
                    self.stop_all();
                    self.set_mode(Mode::Idle);
                }
                self.emit_state();
            }
            Cmd::CycleFilter => {
                let next = match self.filter {
                    Filter::All => Filter::NotDone,
                    Filter::NotDone => Filter::Marked,
                    Filter::Marked => Filter::All,
                };
                self.set_filter(next);
            }
            Cmd::SetFilter(f) => self.set_filter(f),
        }
    }

    fn set_filter(&mut self, f: Filter) {
        let n = self.lesson.phrases.len();
        let old = self.filter;
        self.filter = f;
        self.note = None;
        if f != Filter::All && !(0..n).any(|i| self.wanted(i)) {
            self.filter = old;
            self.note = Some(match f {
                Filter::Marked => "Mark some words first: F, or right-click a word.".into(),
                _ => "Every phrase is done. Start over to practise them again.".into(),
            });
        } else if self.mode == Mode::Idle && !self.wanted(self.k) {
            // stopped on a phrase this filter skips: go to the next one it keeps (or the first)
            if let Some(k) = self.next_from(self.k).or_else(|| (0..n).find(|&i| self.wanted(i))) {
                self.k = k;
            }
        }
        self.emit_state();
    }

    /// Advance by `dt` seconds of wall time (called every ~15 ms).
    pub fn tick(&mut self, dt: f64) {
        self.count_time(dt);
        match self.mode {
            Mode::Teacher | Mode::Compare if self.mode == Mode::Teacher || self.stage == 0 => {
                // which word is being said
                let t = self.audio.position() as f64 / self.audio.out_rate() as f64;
                let w = self.lesson.words.partition_point(|w| w.s <= t + 0.03) as i32 - 1;
                let p = &self.lesson.phrases[self.k];
                let w = if w >= p.a as i32 && w < p.b as i32 { w } else { -1 };
                if w != self.last_word {
                    self.last_word = w;
                    self.events.push(Event::Word { word: w, t });
                }
                if !self.audio.is_playing() {
                    if self.last_word != -1 {
                        self.last_word = -1;
                        self.events.push(Event::Word { word: -1, t });
                    }
                    if self.mode == Mode::Compare {
                        self.stage = 1;
                        self.gap_left = 0.25;
                    } else {
                        match self.after {
                            After::Turn => self.start_turn(),
                            After::Decide => self.set_mode(Mode::Decide),
                            After::Idle => self.set_mode(Mode::Idle),
                            After::Next => match self.next_from(self.k) {
                                Some(k) => {
                                    // a breath between phrases
                                    self.k = k;
                                    self.mode = Mode::Compare; // reuse the gap timer
                                    self.stage = 3;
                                    self.gap_left = 0.6;
                                }
                                None => {
                                    self.note = Some(self.end_note());
                                    self.set_mode(Mode::Idle);
                                }
                            },
                        }
                    }
                }
            }
            Mode::Compare => match self.stage {
                1 | 3 => {
                    self.gap_left -= dt;
                    if self.gap_left <= 0.0 {
                        if self.stage == 3 {
                            let k = self.k;
                            self.play_teacher(k, After::Next);
                        } else if self.play_mine() {
                            self.stage = 2;
                        } else {
                            self.set_mode(Mode::Decide);
                        }
                    }
                }
                _ => {
                    if !self.audio.is_playing() {
                        self.set_mode(Mode::Decide);
                    }
                }
            },
            Mode::Mine => {
                if !self.audio.is_playing() {
                    self.set_mode(Mode::Decide);
                }
            }
            Mode::Turn => {
                let x = self.audio.mic_take();
                let Some(l) = self.listener.as_mut() else { return };
                let (level, voice, heard) = l.feed(&x);
                match heard {
                    Heard::Waiting => self.events.push(Event::Level { level, voice }),
                    Heard::Done => self.finish_turn(true, "auto"),
                    Heard::Nothing => self.finish_turn(false, "no voice"),
                }
            }
            Mode::Idle | Mode::Decide => {}
            Mode::Teacher => unreachable!(),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::align::SpokenWord;
    use crate::audio::tone;

    /// Speakers and mic on a fake clock: `advance` moves time; the mic hears a script.
    pub struct Fake {
        pub rate: u32,
        pub buf: Option<(Arc<Vec<f32>>, f64, usize)>, // buffer, play position (float), end
        pub played: Vec<(usize, usize, usize)>,       // (buffer len, from, to)
        pub mic: Vec<f32>,
        pub mic_open: bool,
        pub mic_opens: usize,
    }

    impl Fake {
        pub fn new() -> Self {
            Self { rate: 1000, buf: None, played: vec![], mic: vec![], mic_open: false, mic_opens: 0 }
        }
        fn advance(&mut self, dt: f64) {
            if let Some((_, pos, end)) = self.buf.as_mut() {
                *pos = (*pos + dt * self.rate as f64).min(*end as f64);
            }
        }
    }

    impl Audio for Fake {
        fn out_rate(&self) -> u32 {
            self.rate
        }
        fn play(&mut self, buf: Arc<Vec<f32>>, from: usize, to: usize) {
            self.played.push((buf.len(), from, to));
            self.buf = Some((buf, from as f64, to));
        }
        fn stop(&mut self) {
            self.buf = None;
        }
        fn is_playing(&self) -> bool {
            self.buf.as_ref().is_some_and(|(_, p, e)| (*p as usize) < *e)
        }
        fn position(&self) -> usize {
            self.buf.as_ref().map(|(_, p, _)| *p as usize).unwrap_or(0)
        }
        fn mic_open(&mut self) -> anyhow::Result<u32> {
            if !self.mic_open {
                self.mic_opens += 1;
            }
            self.mic_open = true;
            Ok(16000)
        }
        fn mic_close(&mut self) {
            self.mic_open = false;
        }
        fn mic_take(&mut self) -> Vec<f32> {
            std::mem::take(&mut self.mic)
        }
    }

    fn lesson() -> Lesson {
        let words = vec![
            SpokenWord { pdf: -1, ..SpokenWord::new("Listen.", 0.0, 0.5) },
            SpokenWord { pdf: 0, ..SpokenWord::new("red", 1.0, 1.4) },
            SpokenWord { pdf: 1, ..SpokenWord::new("rat", 2.0, 2.4) },
        ];
        let ph = |a, b, s: f64, e: f64, inp| Phrase { a, b, start: s, end: e, stop: e + 0.2, in_pdf: inp };
        Lesson {
            teacher: Arc::new(vec![0.1; 3000]),
            words,
            pdf: vec![],
            phrases: vec![ph(0, 1, 0.0, 0.5, false), ph(1, 2, 1.0, 1.4, true), ph(2, 3, 2.0, 2.4, true)],
        }
    }

    fn run<A: Audio>(s: &mut Session<A>, secs: f64, mut each: impl FnMut(&mut Session<A>)) {
        let steps = (secs / 0.015) as usize;
        for _ in 0..steps {
            each(s);
            s.tick(0.015);
        }
    }

    fn step(s: &mut Session<Fake>, secs: f64, voice: impl Fn(f64) -> bool) {
        let mut t = 0.0;
        run(s, secs, |s| {
            s.audio.advance(0.015);
            if s.audio.mic_open {
                let x = if voice(t) { tone(16000, 0.015, 220.0, 0.3) } else { tone(16000, 0.015, 50.0, 0.0005) };
                s.audio.mic.extend(x);
            }
            t += 0.015;
        });
    }

    #[test]
    fn listen_turn_compare_decide_good() {
        let mut s = Session::new(Fake::new(), lesson(), LessonProgress::default());
        assert_eq!(s.phrase(), 1, "skips the teacher's explanation");
        s.command(Cmd::Space);
        assert_eq!(s.mode(), Mode::Teacher);
        assert!(s.audio.mic_open, "mic opens with the teacher, ready for your turn");
        step(&mut s, 0.8, |_| false); // phrase 1.0-1.6 incl. lead-in
        assert_eq!(s.mode(), Mode::Turn);
        let ev = s.drain();
        assert!(ev.iter().any(|e| matches!(e, Event::Word { word: 1, .. })), "highlights 'red'");
        // you speak 0.3-1.0 s into the turn, then silence
        step(&mut s, 2.2, |t| t > 0.3 && t < 1.0);
        assert_eq!(s.mode(), Mode::Compare, "turn ends by itself after the silence");
        assert!(!s.audio.mic_open);
        step(&mut s, 0.8, |_| false); // teacher again
        step(&mut s, 0.4, |_| false); // the gap
        let take = s.audio.played.last().unwrap();
        assert!(take.0 > 700 && take.0 < 1200, "the take is trimmed: {take:?}");
        step(&mut s, 1.2, |_| false);
        assert_eq!(s.mode(), Mode::Decide);
        assert_eq!(s.snapshot().tries, 1);
        s.command(Cmd::Good);
        assert_eq!(s.phrase(), 2);
        assert_eq!(s.mode(), Mode::Teacher);
        assert_eq!(s.snapshot().done, 1);
        assert_eq!(s.progress.pos, 2);
    }

    #[test]
    fn again_until_happy_then_done() {
        let mut s = Session::new(Fake::new(), lesson(), LessonProgress::default());
        for _ in 0..3 {
            s.command(Cmd::Again);
            step(&mut s, 0.8, |_| false);
            assert_eq!(s.mode(), Mode::Turn);
            s.audio.mic.clear();
            step(&mut s, 0.6, |t| t > 0.1);
            s.command(Cmd::Space); // done talking: no waiting for the silence
            assert_eq!(s.mode(), Mode::Compare);
            s.command(Cmd::Space); // skip the playback
            assert_eq!(s.mode(), Mode::Decide);
        }
        assert_eq!(s.snapshot().tries, 3);
        assert!(s.snapshot().has_take);
        s.command(Cmd::Mine);
        assert_eq!(s.mode(), Mode::Mine);
        step(&mut s, 2.0, |_| false);
        assert_eq!(s.mode(), Mode::Decide);
        s.command(Cmd::Goto(2));
        s.command(Cmd::Good);
        assert_eq!(s.snapshot().note.as_deref(), Some("Lesson done!"));
        assert_eq!(s.mode(), Mode::Idle);
    }

    #[test]
    fn silence_means_didnt_hear_you() {
        let mut s = Session::new(Fake::new(), lesson(), LessonProgress::default());
        s.command(Cmd::Space);
        step(&mut s, 0.8, |_| false);
        step(&mut s, 8.5, |_| false);
        assert_eq!(s.mode(), Mode::Decide);
        assert!(s.snapshot().note.unwrap().contains("Didn't hear"));
        assert_eq!(s.snapshot().tries, 0);
        assert!(!s.audio.mic_open);
    }

    #[test]
    fn listen_only_plays_through() {
        let mut s = Session::new(Fake::new(), lesson(), LessonProgress::default());
        s.command(Cmd::ToggleListenOnly);
        s.command(Cmd::Goto(0));
        assert!(!s.audio.mic_open);
        step(&mut s, 6.0, |_| false);
        assert_eq!(s.audio.played.len(), 3, "all phrases, no turns");
        assert_eq!(s.mode(), Mode::Idle);
        assert_eq!(s.audio.mic_opens, 0);
    }

    #[test]
    fn filter_marked_then_not_done() {
        let mut s = Session::new(Fake::new(), lesson(), LessonProgress::default());
        assert_eq!(s.phrase(), 1);
        // nothing marked yet: Marked can't be picked, and it says how to mark
        s.command(Cmd::SetFilter(Filter::Marked));
        assert_eq!(s.snapshot().filter, Filter::All);
        assert!(s.snapshot().note.is_some());
        // mark "rat" (phrase 2): picking Marked moves there, and Good finishes the marked phrases
        s.progress.marked.insert(1);
        s.command(Cmd::SetFilter(Filter::Marked));
        assert_eq!(s.snapshot().filter, Filter::Marked);
        assert_eq!(s.phrase(), 2);
        s.command(Cmd::Good);
        assert_eq!(s.snapshot().note.as_deref(), Some("End of the marked phrases."));
        // Back from there skips the unmarked phrase 1
        s.command(Cmd::Prev);
        assert_eq!(s.phrase(), 2);
        // Not done: phrase 2 is done now, so only phrase 1 is left
        s.command(Cmd::Stop);
        s.command(Cmd::SetFilter(Filter::NotDone));
        assert_eq!(s.snapshot().filter, Filter::NotDone);
        assert_eq!(s.phrase(), 1);
        s.command(Cmd::Good);
        assert_eq!(s.snapshot().note.as_deref(), Some("End of the phrases still to do."));
        // everything done: Not done can't be picked
        s.command(Cmd::SetFilter(Filter::All));
        s.command(Cmd::SetFilter(Filter::NotDone));
        assert_eq!(s.snapshot().filter, Filter::All);
        // the key cycles All → Not done → Marked → All (Not done is skipped back to All here)
        s.command(Cmd::CycleFilter);
        assert_eq!(s.snapshot().filter, Filter::All);
    }

    #[test]
    fn practice_time_counts_while_practising() {
        let mut s = Session::new(Fake::new(), lesson(), LessonProgress::default());
        step(&mut s, 2.0, |_| false); // stopped: no time
        assert_eq!(s.progress.secs, 0.0);
        s.command(Cmd::Teacher);
        step(&mut s, 2.0, |_| false); // the teacher (~0.6 s), then stopped again
        assert!(s.progress.secs > 0.3 && s.progress.secs < 1.0, "{}", s.progress.secs);
        assert!(s.progress.last > 0, "when it ended is saved");
    }

    /// A lesson with one long phrase (`secs` long, the teacher pausing `gap` inside it).
    fn long_lesson(secs: f64, gap: f64) -> Lesson {
        // six words: three, the teacher's pause, three more
        let half = secs / 2.0 - gap / 2.0;
        let words: Vec<SpokenWord> = (0..6)
            .map(|i| {
                let (a, w) = if i < 3 { (0.0, half / 3.0) } else { (secs / 2.0 + gap / 2.0, half / 3.0) };
                let k = (i % 3) as f64;
                SpokenWord { pdf: i, ..SpokenWord::new("word", a + k * w, a + (k + 1.0) * w) }
            })
            .collect();
        Lesson {
            teacher: Arc::new(vec![0.1; ((secs + 1.0) * 1000.0) as usize]),
            words,
            pdf: vec![],
            phrases: vec![Phrase { a: 0, b: 6, start: 0.0, end: secs, stop: secs + 0.2, in_pdf: true }],
        }
    }

    /// Plays the teacher, then you speak by `voice(t)` (amplitude, 0 = silence) for up to `secs`.
    /// Returns (seconds from your last sound until the turn ended, take length in seconds).
    fn turn_with(lesson: Lesson, behaviour: Behaviour, secs: f64, voice: impl Fn(f64) -> f32) -> (f64, f64) {
        let mut s = Session::new(Fake::new(), lesson, LessonProgress::default());
        s.behaviour = behaviour;
        s.command(Cmd::Space);
        while s.mode() == Mode::Teacher {
            s.audio.advance(0.01);
            s.tick(0.01);
        }
        assert_eq!(s.mode(), Mode::Turn);
        let (mut t, mut last_voice) = (0.0, 0.0);
        while s.mode() == Mode::Turn && t < secs {
            let a = voice(t);
            let x = if a > 0.0 { tone(16000, 0.01, 220.0, a) } else { tone(16000, 0.01, 50.0, 0.0005) };
            if a > 0.0 {
                last_voice = t + 0.01;
            }
            s.audio.mic.extend(x);
            s.tick(0.01);
            t += 0.01;
        }
        assert_ne!(s.mode(), Mode::Turn, "the turn should have ended by itself");
        let take = s.takes.get(&s.k).map(|x| x.out.len()).unwrap_or(0) as f64 / 1000.0;
        (t - last_voice, take)
    }

    #[test]
    fn turn_ends_quickly_once_you_have_said_it() {
        // a 1.5 s phrase; you say it in 1.4 s
        let (wait, take) = turn_with(long_lesson(1.5, 0.0), Behaviour::default(), 5.0, |t| if t > 0.2 && t < 1.6 { 0.3 } else { 0.0 });
        println!("turn ended {wait:.2}s after you stopped; take {take:.2}s");
        // a sentence you've finished ends after 0.5 s
        assert!(wait <= 0.55, "waited {wait:.2}s after you stopped");
        assert!((take - (1.4 + 0.15 + 0.25)).abs() < 0.06, "take {take:.2}s");
    }

    #[test]
    fn a_pause_mid_phrase_does_not_end_the_turn() {
        // a 3 s phrase; you say half, think for 0.7 s, then say the rest
        let (_, take) = turn_with(long_lesson(3.0, 0.1), Behaviour::default(), 8.0, |t| {
            if (0.2..1.4).contains(&t) || (2.1..3.6).contains(&t) { 0.3 } else { 0.0 }
        });
        assert!(take > 3.3, "both halves are in the take: {take:.2}s");
    }

    #[test]
    fn a_breath_in_a_long_sentence_is_fine() {
        // a 4 s sentence; you say 2 s of it, breathe for 1.1 s, then finish
        let (wait, take) = turn_with(long_lesson(4.0, 0.1), Behaviour::default(), 10.0, |t| {
            if (0.2..2.2).contains(&t) || (3.3..5.3).contains(&t) { 0.3 } else { 0.0 }
        });
        assert!(take > 5.0, "both parts are in the take: {take:.2}s");
        assert!(wait <= 0.55, "and it still ends quickly at the end: {wait:.2}s");
    }

    #[test]
    fn soft_word_endings_are_kept() {
        // loud for 1 s, then a soft "s" (25 dB quieter) for 0.25 s
        let (_, take) = turn_with(long_lesson(1.2, 0.0), Behaviour::default(), 5.0, |t| {
            if (0.2..1.2).contains(&t) { 0.3 } else if (1.2..1.45).contains(&t) { 0.017 } else { 0.0 }
        });
        assert!(take >= 1.25 + 0.15 + 0.2, "the soft end is in the take: {take:.2}s");
    }

    #[test]
    fn patient_waits_longer_and_manual_waits_for_space() {
        let say = |t: f64| if t > 0.2 && t < 1.6 { 0.3 } else { 0.0 };
        let (quick, _) = turn_with(long_lesson(1.5, 0.0), Behaviour::default(), 5.0, say);
        let (slow, _) = turn_with(long_lesson(1.5, 0.0), Behaviour { turn_end: TurnEnd::Patient, ..Default::default() }, 5.0, say);
        assert!(slow > quick + 0.15, "patient {slow:.2} vs smart {quick:.2}");
        let mut s = Session::new(Fake::new(), long_lesson(1.5, 0.0), LessonProgress::default());
        s.behaviour.turn_end = TurnEnd::Manual;
        s.command(Cmd::Space);
        step(&mut s, 1.8, |_| false);
        step(&mut s, 4.0, |t| t < 1.0);
        assert_eq!(s.mode(), Mode::Turn, "manual: still listening");
        s.command(Cmd::Space);
        assert_eq!(s.mode(), Mode::Compare);
    }

    #[test]
    fn playback_setting() {
        let say = |t: f64| if t > 0.2 && t < 1.6 { 0.3 } else { 0.0 };
        let mut s = Session::new(Fake::new(), long_lesson(1.5, 0.0), LessonProgress::default());
        s.behaviour.playback = Playback::Nothing;
        s.command(Cmd::Space);
        step(&mut s, 1.8, |_| false);
        step(&mut s, 3.0, |t| say(t) > 0.0);
        assert_eq!(s.mode(), Mode::Decide, "straight to deciding");
        let _ = turn_with(long_lesson(1.5, 0.0), Behaviour { playback: Playback::MeOnly, ..Default::default() }, 5.0, say);
    }

    #[test]
    fn one_word_ends_after_a_short_pause_even_if_said_fast() {
        // the teacher draws a word out over 0.9 s; you say it in 0.3 s
        let mut l = lesson();
        l.phrases[1].end = l.phrases[1].start + 0.9;
        let (wait, _) = turn_with(l, Behaviour::default(), 5.0, |t| if (0.2..0.5).contains(&t) { 0.3 } else { 0.0 });
        assert!(wait <= 0.4, "waited {wait:.2}s");
    }

    #[test]
    fn rumble_does_not_keep_the_turn_open() {
        // a loud 60 Hz hum all the time (a fan, a raw webcam mic); you say one word
        // one continuous hum (cut into 10 ms pieces below), there all along
        let hum = tone(16000, 10.0, 60.0, 0.05);
        let mut at = 0;
        let mut next = |n: usize| {
            let x = hum[at..at + n].to_vec();
            at += n;
            x
        };
        let mut s = Session::new(Fake::new(), lesson(), LessonProgress::default());
        s.command(Cmd::Space);
        while s.mode() == Mode::Teacher {
            s.audio.advance(0.01);
            if s.audio.mic_open {
                s.audio.mic.extend(next(160));
            }
            s.tick(0.01);
        }
        assert_eq!(s.mode(), Mode::Turn);
        let mut t = 0.0;
        let mut ended = None;
        while t < 4.0 {
            let mut x = next(160);
            if (0.3..0.7).contains(&t) {
                for (a, b) in x.iter_mut().zip(tone(16000, 0.01, 700.0, 0.2)) {
                    *a += b;
                }
            }
            s.audio.mic.extend(x);
            s.tick(0.01);
            t += 0.01;
            if ended.is_none() && s.mode() != Mode::Turn {
                ended = Some(t);
            }
        }
        let ended = ended.expect("turn ended");
        assert!(ended < 0.7 + 0.5, "ended {ended:.2}s, voice stopped at 0.70s");
    }

    #[test]
    fn resumes_where_you_stopped() {
        let p = LessonProgress { pos: 2, ..Default::default() };
        let s = Session::new(Fake::new(), lesson(), p);
        assert_eq!(s.phrase(), 2);
    }
}
