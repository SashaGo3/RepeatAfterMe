//! Progress per lesson: where you stopped, and per phrase how many tries and whether you called it good.
//! Saved in one small JSON file, keyed by the lesson's content hash (moving or renaming files keeps it).

use crate::align::Phrase;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct PhraseProgress {
    pub tries: u32,
    pub done: bool,
    /// unix seconds of the last change
    pub at: u64,
    /// for people reading the file
    #[serde(default)]
    pub text: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct LessonProgress {
    /// the phrase you were on
    pub pos: usize,
    /// by phrase start time in centiseconds (stable when phrase numbers shift)
    pub phrases: BTreeMap<String, PhraseProgress>,
    /// how many phrases there are to repeat (for the lesson list)
    #[serde(default)]
    pub total: Option<usize>,
    /// printed words you marked as needing attention (indices into the PDF's words)
    #[serde(default)]
    pub marked: std::collections::BTreeSet<usize>,
    /// seconds spent practising this lesson (listening, your turns, deciding)
    #[serde(default)]
    pub secs: f64,
    /// unix seconds you last practised it (0 = never)
    #[serde(default)]
    pub last: u64,
}

pub fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl LessonProgress {
    pub fn key(p: &Phrase) -> String {
        format!("{}", (p.start * 100.0).round() as i64)
    }
    pub fn is_done(&self, p: &Phrase) -> bool {
        self.phrases.get(&Self::key(p)).is_some_and(|r| r.done)
    }
    pub fn tried(&mut self, key: &str, _p: &Phrase) {
        let r = self.phrases.entry(key.to_string()).or_default();
        r.tries += 1;
        r.at = now();
    }
    pub fn mark_done(&mut self, key: &str, _p: &Phrase) {
        let r = self.phrases.entry(key.to_string()).or_default();
        r.done = true;
        r.at = now();
    }
    /// Bring in progress from elsewhere (a backup, another computer): nothing you did is lost on
    /// either side. Done stays done, tries and time take the larger, marks are combined.
    pub fn merge(&mut self, other: &LessonProgress) {
        for (k, o) in &other.phrases {
            let r = self.phrases.entry(k.clone()).or_default();
            r.tries = r.tries.max(o.tries);
            r.done |= o.done;
            r.at = r.at.max(o.at);
            if r.text.is_empty() {
                r.text = o.text.clone();
            }
        }
        if other.last > self.last {
            self.pos = other.pos;
        }
        self.total = self.total.max(other.total);
        self.marked.extend(other.marked.iter().copied());
        self.secs = self.secs.max(other.secs);
        self.last = self.last.max(other.last);
    }

    pub fn set_text(&mut self, key: &str, text: &str) {
        if let Some(r) = self.phrases.get_mut(key) {
            if r.text.is_empty() {
                r.text = text.to_string();
            }
        }
    }
}

pub struct Store {
    path: PathBuf,
    all: Mutex<BTreeMap<String, LessonProgress>>,
}

impl Store {
    pub fn open(path: PathBuf) -> Self {
        let all = std::fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        Self { path, all: Mutex::new(all) }
    }
    pub fn get(&self, id: &str) -> LessonProgress {
        self.all.lock().get(id).cloned().unwrap_or_default()
    }
    /// Save one lesson's progress (written to a temp file, then renamed: never half a file).
    pub fn put(&self, id: &str, p: &LessonProgress) -> anyhow::Result<()> {
        let json = {
            let mut all = self.all.lock();
            all.insert(id.to_string(), p.clone());
            serde_json::to_string_pretty(&*all)?
        };
        write_atomic(&self.path, json.as_bytes())
    }
    /// Every lesson's progress (for a backup).
    pub fn all(&self) -> BTreeMap<String, LessonProgress> {
        self.all.lock().clone()
    }

    /// Merge a backup into what's here (see [`LessonProgress::merge`]). Returns how many lessons it had.
    pub fn merge_all(&self, other: &BTreeMap<String, LessonProgress>) -> anyhow::Result<usize> {
        let json = {
            let mut all = self.all.lock();
            for (id, p) in other {
                all.entry(id.clone()).or_default().merge(p);
            }
            serde_json::to_string_pretty(&*all)?
        };
        write_atomic(&self.path, json.as_bytes())?;
        Ok(other.len())
    }

    /// Forget all progress, of every lesson.
    pub fn clear(&self) -> anyhow::Result<()> {
        self.all.lock().clear();
        write_atomic(&self.path, b"{}")
    }

    pub fn reset(&self, id: &str) -> anyhow::Result<()> {
        let json = {
            let mut all = self.all.lock();
            all.remove(id);
            serde_json::to_string_pretty(&*all)?
        };
        write_atomic(&self.path, json.as_bytes())
    }
}

pub fn write_atomic(path: &Path, data: &[u8]) -> anyhow::Result<()> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_reset() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("progress.json");
        let ph = Phrase { a: 0, b: 1, start: 12.345, end: 13.0, stop: 13.2, in_pdf: true };
        let s = Store::open(path.clone());
        let mut p = s.get("abc");
        p.tried(&LessonProgress::key(&ph), &ph);
        p.tried(&LessonProgress::key(&ph), &ph);
        p.mark_done(&LessonProgress::key(&ph), &ph);
        p.pos = 7;
        s.put("abc", &p).unwrap();
        let again = Store::open(path.clone()).get("abc");
        assert_eq!(again, p);
        assert!(again.is_done(&ph));
        assert_eq!(again.phrases["1235"].tries, 2);
        s.reset("abc").unwrap();
        assert_eq!(Store::open(path).get("abc"), LessonProgress::default());
    }

    #[test]
    fn broken_file_starts_fresh() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("progress.json");
        std::fs::write(&path, "{ not json").unwrap();
        assert_eq!(Store::open(path).get("x"), LessonProgress::default());
    }
}
