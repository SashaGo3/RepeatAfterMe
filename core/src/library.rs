//! Your lessons: each is an audio file + its PDF, used where they are (nothing is copied).

use crate::progress::write_atomic;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

pub const AUDIO_EXT: &[&str] = &["mp3", "m4a", "aac", "wav", "flac", "ogg", "oga", "mp4", "caf", "aif", "aiff"];

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Entry {
    pub audio: PathBuf,
    pub pdf: PathBuf,
    /// the folder it came from (for grouping in the list)
    pub group: String,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Unpaired {
    pub audio: Vec<PathBuf>,
    pub pdf: Vec<PathBuf>,
}

pub fn is_audio(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| AUDIO_EXT.contains(&e.to_lowercase().as_str()))
}
pub fn is_pdf(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
}

/// A lesson's id: its audio's size + a hash of its start and end (fast on big files; survives moves).
pub fn content_id(audio: &Path) -> Result<String> {
    let mut f = std::fs::File::open(audio)?;
    let len = f.metadata()?.len();
    let mut h = blake3::Hasher::new();
    h.update(&len.to_le_bytes());
    let mut buf = vec![0u8; 1 << 20];
    let n = f.read(&mut buf)?;
    h.update(&buf[..n]);
    if len > 2 << 20 {
        f.seek(SeekFrom::End(-(1 << 20)))?;
        let n = f.read(&mut buf)?;
        h.update(&buf[..n]);
    }
    Ok(h.finalize().to_hex()[..16].to_string())
}

fn tokens(name: &str) -> Vec<String> {
    let lower = name.to_lowercase();
    let mut out = vec![];
    for t in lower.split(|c: char| !c.is_alphanumeric()) {
        let t = match t {
            "" => continue,
            "versus" | "v" => "vs",
            "&" => "and",
            other => other,
        };
        out.push(t.to_string());
    }
    out
}

/// How alike two file names are, 0..1 (shared words, then spelling).
pub fn name_score(a: &str, b: &str) -> f64 {
    let (ta, tb) = (tokens(a), tokens(b));
    if ta == tb {
        return 1.0;
    }
    let (sa, sb): (HashSet<&String>, HashSet<&String>) = (ta.iter().collect(), tb.iter().collect());
    let inter = sa.intersection(&sb).count() as f64;
    let union = sa.union(&sb).count().max(1) as f64;
    let jac = inter / union;
    let spell = 1.0 - crate::align::distance(&ta.concat(), &tb.concat());
    0.7 * jac + 0.3 * spell
}

fn stem(p: &Path) -> String {
    p.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string()
}

/// Pair audio with PDFs: the same name first, then the most alike names (each PDF once), then a folder's
/// only audio with its only PDF.
pub fn pair(audio: &[PathBuf], pdfs: &[PathBuf]) -> (Vec<(PathBuf, PathBuf)>, Unpaired) {
    let mut pairs = vec![];
    let mut free_a: Vec<PathBuf> = audio.to_vec();
    let mut free_p: Vec<PathBuf> = pdfs.to_vec();
    // 1. same folder, same name
    free_a.retain(|a| {
        if let Some(i) = free_p.iter().position(|p| p.parent() == a.parent() && tokens(&stem(p)) == tokens(&stem(a))) {
            pairs.push((a.clone(), free_p.remove(i)));
            false
        } else {
            true
        }
    });
    // 2. most alike names in the same folder, best first
    let mut cands: Vec<(f64, usize, usize)> = vec![];
    for (i, a) in free_a.iter().enumerate() {
        for (j, p) in free_p.iter().enumerate() {
            if a.parent() == p.parent() {
                let s = name_score(&stem(a), &stem(p));
                if s >= 0.45 {
                    cands.push((s, i, j));
                }
            }
        }
    }
    cands.sort_by(|x, y| y.0.partial_cmp(&x.0).unwrap());
    let (mut used_a, mut used_p) = (HashSet::new(), HashSet::new());
    for (_, i, j) in cands {
        if used_a.contains(&i) || used_p.contains(&j) {
            continue;
        }
        used_a.insert(i);
        used_p.insert(j);
        pairs.push((free_a[i].clone(), free_p[j].clone()));
    }
    let mut rest_a: Vec<PathBuf> = free_a.into_iter().enumerate().filter(|(i, _)| !used_a.contains(i)).map(|(_, a)| a).collect();
    let mut rest_p: Vec<PathBuf> = free_p.into_iter().enumerate().filter(|(j, _)| !used_p.contains(j)).map(|(_, p)| p).collect();
    // 3. one audio and one PDF left in a folder belong together
    let mut by_dir: BTreeMap<PathBuf, (Vec<usize>, Vec<usize>)> = BTreeMap::new();
    for (i, a) in rest_a.iter().enumerate() {
        by_dir.entry(a.parent().unwrap_or(Path::new("")).to_path_buf()).or_default().0.push(i);
    }
    for (j, p) in rest_p.iter().enumerate() {
        by_dir.entry(p.parent().unwrap_or(Path::new("")).to_path_buf()).or_default().1.push(j);
    }
    let mut drop_a = HashSet::new();
    let mut drop_p = HashSet::new();
    for (_, (ai, pj)) in by_dir {
        if ai.len() == 1 && pj.len() == 1 {
            pairs.push((rest_a[ai[0]].clone(), rest_p[pj[0]].clone()));
            drop_a.insert(ai[0]);
            drop_p.insert(pj[0]);
        }
    }
    rest_a = rest_a.into_iter().enumerate().filter(|(i, _)| !drop_a.contains(i)).map(|(_, a)| a).collect();
    rest_p = rest_p.into_iter().enumerate().filter(|(j, _)| !drop_p.contains(j)).map(|(_, p)| p).collect();
    pairs.sort();
    (pairs, Unpaired { audio: rest_a, pdf: rest_p })
}

/// Every audio file and PDF under `dir` (hidden folders skipped).
pub fn scan(dir: &Path) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let (mut audio, mut pdfs) = (vec![], vec![]);
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name();
            if name.to_string_lossy().starts_with('.') {
                continue;
            }
            if p.is_dir() {
                stack.push(p);
            } else if is_audio(&p) {
                audio.push(p);
            } else if is_pdf(&p) {
                pdfs.push(p);
            }
        }
    }
    audio.sort();
    pdfs.sort();
    (audio, pdfs)
}

/// "L3 - connections / EEE vs. IH": the folders between the one you added and the file.
fn group_of(root: Option<&Path>, audio: &Path) -> String {
    let parent = audio.parent().unwrap_or(Path::new(""));
    if let Some(r) = root {
        if let Ok(rel) = parent.strip_prefix(r) {
            let parts: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
            if !parts.is_empty() {
                return parts.join(" / ");
            }
            return r.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        }
    }
    parent.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

pub struct Library {
    path: PathBuf,
    pub entries: Vec<Entry>,
    /// folders you added: looked in again for new lessons
    pub folders: Vec<PathBuf>,
    /// recordings you removed: a re-scan doesn't bring them back (adding them yourself does)
    pub ignored: std::collections::BTreeSet<PathBuf>,
    /// folder names you changed (as found → as you called it), so new lessons land in the same group
    pub renames: BTreeMap<String, String>,
}

/// library.json. Older versions saved just the list of lessons; that still opens.
#[derive(Serialize, Deserialize, Default)]
struct LibraryFile {
    #[serde(default)]
    folders: Vec<PathBuf>,
    entries: Vec<Entry>,
    #[serde(default)]
    ignored: std::collections::BTreeSet<PathBuf>,
    #[serde(default)]
    renames: BTreeMap<String, String>,
}

impl Library {
    pub fn open(path: PathBuf) -> Self {
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let f: LibraryFile = serde_json::from_str(&text)
            .or_else(|_| serde_json::from_str::<Vec<Entry>>(&text).map(|entries| LibraryFile { entries, ..Default::default() }))
            .unwrap_or_default();
        Self { path, entries: f.entries, folders: f.folders, ignored: f.ignored, renames: f.renames }
    }

    fn save(&self) -> Result<()> {
        let f = LibraryFile { folders: self.folders.clone(), entries: self.entries.clone(), ignored: self.ignored.clone(), renames: self.renames.clone() };
        write_atomic(&self.path, serde_json::to_string_pretty(&f)?.as_bytes())
    }

    /// A group's name as you see it (after any rename).
    fn shown(&self, group: String) -> String {
        self.renames.get(&group).cloned().unwrap_or(group)
    }

    fn sort(&mut self) {
        self.entries.sort_by(|x, y| (natural(&x.group), natural(&stem(&x.audio))).cmp(&(natural(&y.group), natural(&stem(&y.audio)))));
    }

    /// Add dropped or picked paths: folders are scanned, files are paired. Returns what couldn't be paired.
    pub fn add(&mut self, paths: &[PathBuf]) -> Result<(usize, Unpaired)> {
        let (mut audio, mut pdfs) = (vec![], vec![]);
        let mut root: Option<PathBuf> = None;
        for p in paths {
            if p.is_dir() {
                let (a, d) = scan(p);
                audio.extend(a);
                pdfs.extend(d);
                if root.is_none() {
                    root = Some(p.clone());
                }
                if !self.folders.contains(p) {
                    self.folders.push(p.clone());
                }
            } else if is_audio(p) {
                audio.push(p.clone());
            } else if is_pdf(p) {
                pdfs.push(p.clone());
            }
        }
        let (pairs, unpaired) = pair(&audio, &pdfs);
        let mut added = 0;
        for (a, p) in pairs {
            let group = self.shown(group_of(root.as_deref(), &a));
            self.ignored.remove(&a); // you added it yourself
            self.entries.retain(|e| e.audio != a);
            self.entries.push(Entry { audio: a, pdf: p, group });
            added += 1;
        }
        self.sort();
        self.save()?;
        Ok((added, unpaired))
    }

    /// Look in the folders you added for recordings that aren't in the list yet (and that you
    /// didn't remove). Lessons already there keep their PDF, even one you picked yourself.
    pub fn rescan(&mut self) -> Result<usize> {
        let mut added = 0;
        for root in self.folders.clone() {
            if !root.is_dir() {
                continue; // an unplugged drive, or iCloud not there yet: nothing changes
            }
            let (audio, pdfs) = scan(&root);
            let (pairs, _) = pair(&audio, &pdfs);
            for (a, p) in pairs {
                if self.ignored.contains(&a) || self.entries.iter().any(|e| e.audio == a) {
                    continue;
                }
                let group = self.shown(group_of(Some(&root), &a));
                self.entries.push(Entry { audio: a, pdf: p, group });
                added += 1;
            }
        }
        if added > 0 {
            self.sort();
            self.save()?;
        }
        Ok(added)
    }

    pub fn set_pdf(&mut self, audio: &Path, pdf: PathBuf) -> Result<()> {
        if let Some(e) = self.entries.iter_mut().find(|e| e.audio == audio) {
            e.pdf = pdf;
        }
        self.save()
    }

    pub fn remove(&mut self, audio: &Path) -> Result<()> {
        self.entries.retain(|e| e.audio != audio);
        self.ignored.insert(audio.to_path_buf());
        self.save()
    }

    /// Forget every lesson and folder (the files themselves stay where they are).
    pub fn clear(&mut self) -> Result<()> {
        self.entries.clear();
        self.folders.clear();
        self.ignored.clear();
        self.renames.clear();
        self.save()
    }

    /// Take every lesson of a group off the list (the files stay; a re-scan won't bring them back).
    pub fn remove_group(&mut self, group: &str) -> Result<()> {
        let gone: Vec<PathBuf> = self.entries.iter().filter(|e| e.group == group).map(|e| e.audio.clone()).collect();
        self.entries.retain(|e| e.group != group);
        self.ignored.extend(gone);
        self.save()
    }

    /// Call a group something else; lessons found in it later get the new name too.
    pub fn rename_group(&mut self, from: &str, to: &str) -> Result<()> {
        let to = to.trim();
        if to.is_empty() || to == from {
            return Ok(());
        }
        for e in self.entries.iter_mut().filter(|e| e.group == from) {
            e.group = to.to_string();
        }
        // renames: the folder's name on disk → what you call it. Renaming again updates that.
        let renamed_before = self.renames.values().any(|v| v == from);
        for v in self.renames.values_mut().filter(|v| v.as_str() == from) {
            *v = to.to_string();
        }
        if !renamed_before {
            self.renames.insert(from.to_string(), to.to_string());
        }
        self.renames.retain(|k, v| k != v);
        self.sort();
        self.save()
    }
}

/// Sort key where "L10" comes after "L9".
pub fn natural(s: &str) -> Vec<(String, u64)> {
    let mut out = vec![];
    let mut text = String::new();
    let mut num: Option<u64> = None;
    for c in s.to_lowercase().chars() {
        if let Some(d) = c.to_digit(10) {
            num = Some(num.unwrap_or(0) * 10 + d as u64);
        } else {
            if let Some(n) = num.take() {
                out.push((std::mem::take(&mut text), n));
            }
            text.push(c);
        }
    }
    out.push((text, num.unwrap_or(0)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// File names from a real course folder (names only, no course material).
    const COURSE: &[&str] = &[
        "L1/Muscle Exercises for American Posture.pdf",
        "L10 - L + OOvsU/L/Light L and Dark L Sentences.pdf",
        "L10 - L + OOvsU/L/Light L and Dark L.pdf",
        "L10 - L + OOvsU/L/Light L vs. Dark L Sentences.m4a",
        "L10 - L + OOvsU/L/Light L vs. Dark L.m4a",
        "L10 - L + OOvsU/UUU vs. Book Vowel/UUU vs Book Sentences.pdf",
        "L10 - L + OOvsU/UUU vs. Book Vowel/UUU vs Book.pdf",
        "L10 - L + OOvsU/UUU vs. Book Vowel/UUU vs. Book Vowel Sentences.m4a",
        "L10 - L + OOvsU/UUU vs. Book Vowel/UUU vs. Book Vowel.m4a",
        "L11 - OH vs AH/OH vs. AH/OH vs AH Sentences.pdf",
        "L11 - OH vs AH/OH vs. AH/OH vs AH.pdf",
        "L11 - OH vs AH/OH vs. AH/OH vs. AH Sentences.m4a",
        "L11 - OH vs AH/OH vs. AH/OH vs. AH.m4a",
        "L2 - th/TH_Intro.mp3",
        "L2 - th/TH_Intro.pdf",
        "L2 - th/TH_Sentences.mp3",
        "L2 - th/TH_Sentences.pdf",
        "L2 - th/TH_THE.mp3",
        "L2 - th/TH_THE.pdf",
        "L4 - R/R Sentences.m4a",
        "L4 - R/R Sentences.pdf",
        "L4 - R/R.m4a",
        "L4 - R/R.pdf",
        "L6 - rushing/Malice.m4a",
        "L6 - rushing/Malice.pdf",
        "L6 - rushing/Rushing.pdf",
        "L8 - EH vs AE/EH vs. AE/EH vs. AE Sentences.m4a",
        "L8 - EH vs AE/EH vs. AE/EH vs. AE Sentences.pdf",
        "L8 - EH vs AE/EH vs. AE/EH vs. AE.m4a",
        "L8 - EH vs AE/EH vs. AE/EH vs. AE.pdf",
        "L8 - EH vs AE/Intonation/Intonation Patterns.pdf",
    ];

    #[test]
    fn pairs_the_real_course() {
        let root = PathBuf::from("/c");
        let all: Vec<PathBuf> = COURSE.iter().map(|p| root.join(p)).collect();
        let audio: Vec<PathBuf> = all.iter().filter(|p| is_audio(p)).cloned().collect();
        let pdfs: Vec<PathBuf> = all.iter().filter(|p| is_pdf(p)).cloned().collect();
        let (pairs, rest) = pair(&audio, &pdfs);
        let got: BTreeMap<String, String> = pairs.iter().map(|(a, p)| (stem(a), stem(p))).collect();
        assert_eq!(got["Light L vs. Dark L Sentences"], "Light L and Dark L Sentences");
        assert_eq!(got["Light L vs. Dark L"], "Light L and Dark L");
        assert_eq!(got["UUU vs. Book Vowel Sentences"], "UUU vs Book Sentences");
        assert_eq!(got["UUU vs. Book Vowel"], "UUU vs Book");
        assert_eq!(got["OH vs. AH Sentences"], "OH vs AH Sentences");
        assert_eq!(got["OH vs. AH"], "OH vs AH");
        assert_eq!(got["TH_THE"], "TH_THE");
        assert_eq!(got["R"], "R");
        assert_eq!(got["Malice"], "Malice");
        assert_eq!(pairs.len(), audio.len(), "every recording found its PDF");
        let rest_pdf: Vec<String> = rest.pdf.iter().map(|p| stem(p)).collect();
        assert_eq!(rest_pdf, vec!["Muscle Exercises for American Posture", "Rushing", "Intonation Patterns"]);
    }

    #[test]
    fn rescan_finds_new_lessons_but_not_removed_ones() {
        let dir = tempfile::tempdir().unwrap();
        let course = dir.path().join("Course");
        let l1 = course.join("L1");
        std::fs::create_dir_all(&l1).unwrap();
        for f in ["A.m4a", "A.pdf"] {
            std::fs::write(l1.join(f), b"x").unwrap();
        }
        let path = dir.path().join("library.json");
        let mut lib = Library::open(path.clone());
        assert_eq!(lib.add(&[course.clone()]).unwrap().0, 1);
        assert_eq!(lib.rescan().unwrap(), 0, "nothing new");
        // a new lesson appears in the folder, and in a new subfolder
        std::fs::create_dir_all(course.join("L2")).unwrap();
        for f in ["L1/B.m4a", "L1/B.pdf", "L2/C.mp3", "L2/C.pdf"] {
            std::fs::write(course.join(f), b"x").unwrap();
        }
        lib.rename_group("L1", "Week one").unwrap();
        let mut lib = Library::open(path.clone()); // the folders, renames and ignores are saved
        assert_eq!(lib.rescan().unwrap(), 2);
        let b = lib.entries.iter().find(|e| e.audio.ends_with("B.m4a")).unwrap();
        assert_eq!(b.group, "Week one", "a new lesson in a renamed folder joins it");
        // removed ones stay removed; adding them yourself brings them back
        lib.remove(&l1.join("A.m4a")).unwrap();
        lib.remove_group("L2").unwrap();
        assert_eq!(lib.rescan().unwrap(), 0);
        assert_eq!(lib.entries.len(), 1);
        lib.add(&[l1.join("A.m4a"), l1.join("A.pdf")]).unwrap();
        assert_eq!(lib.entries.len(), 2);
    }

    #[test]
    fn old_library_file_still_opens() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.json");
        std::fs::write(&path, r#"[{"audio":"/c/A.m4a","pdf":"/c/A.pdf","group":"L1"}]"#).unwrap();
        let lib = Library::open(path);
        assert_eq!(lib.entries.len(), 1);
        assert!(lib.folders.is_empty());
    }

    #[test]
    fn natural_sort() {
        let mut v = vec!["L10 - L", "L2 - th", "L1", "L9 - uh"];
        v.sort_by_key(|s| natural(s));
        assert_eq!(v, vec!["L1", "L2 - th", "L9 - uh", "L10 - L"]);
    }

    #[test]
    fn add_folder_and_ids_survive_moves() {
        let dir = tempfile::tempdir().unwrap();
        let course = dir.path().join("Course");
        std::fs::create_dir_all(course.join("L4 - R")).unwrap();
        std::fs::write(course.join("L4 - R/R.m4a"), b"audio bytes").unwrap();
        std::fs::write(course.join("L4 - R/R.pdf"), b"%PDF").unwrap();
        std::fs::write(course.join("L4 - R/.hidden.mp3"), b"x").unwrap();
        let mut lib = Library::open(dir.path().join("library.json"));
        let (n, rest) = lib.add(&[course.clone()]).unwrap();
        assert_eq!(n, 1);
        assert!(rest.audio.is_empty() && rest.pdf.is_empty());
        assert_eq!(lib.entries[0].group, "L4 - R");
        let id = content_id(&course.join("L4 - R/R.m4a")).unwrap();
        std::fs::rename(course.join("L4 - R/R.m4a"), dir.path().join("moved.m4a")).unwrap();
        assert_eq!(content_id(&dir.path().join("moved.m4a")).unwrap(), id);
        // adding again doesn't duplicate
        std::fs::rename(dir.path().join("moved.m4a"), course.join("L4 - R/R.m4a")).unwrap();
        lib.add(&[course]).unwrap();
        assert_eq!(Library::open(dir.path().join("library.json")).entries.len(), 1);
    }
}
