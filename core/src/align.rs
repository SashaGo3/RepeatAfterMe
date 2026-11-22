//! Lining the spoken words (from the transcript) up with the words printed in the PDF, and
//! splitting the lesson into phrases you repeat one at a time. Port of the old `Lesson.swift`.

use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpokenWord {
    /// as transcribed, with punctuation
    pub w: String,
    /// start / end, seconds
    pub s: f64,
    pub e: f64,
    /// normalised (see [`norm`])
    #[serde(default)]
    pub n: String,
    /// index into the PDF's words, -1 = not in the PDF
    #[serde(default = "neg")]
    pub pdf: i32,
}

fn neg() -> i32 {
    -1
}

impl SpokenWord {
    pub fn new(w: &str, s: f64, e: f64) -> Self {
        Self { w: w.to_string(), s, e, n: norm(w), pdf: -1 }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn max_x(&self) -> f32 {
        self.x + self.w
    }
    pub fn mid_y(&self) -> f32 {
        self.y + self.h / 2.0
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PdfWord {
    pub text: String,
    pub n: String,
    pub page: usize,
    /// in page coordinates, as a fraction of the page size (0..1), origin top-left
    pub rect: Rect,
    /// index into the spoken words, -1 = not spoken
    pub tw: i32,
    /// words the PDF prints together (a sentence, a grid cell, a phrase between ·)
    pub chunk: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Phrase {
    /// spoken words a..b
    pub a: usize,
    pub b: usize,
    pub start: f64,
    pub end: f64,
    /// where playback stops: a little after the end, before the next phrase
    pub stop: f64,
    /// mostly printed in the PDF = something to repeat (not the teacher explaining)
    pub in_pdf: bool,
}

// ---------------------------------------------------------------- normalising words

const NUM: [&str; 21] = [
    "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven", "twelve",
    "thirteen", "fourteen", "fifteen", "sixteen", "seventeen", "eighteen", "nineteen", "twenty",
];
const TENS: [&str; 10] = ["", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety"];

fn num_word(n: usize) -> String {
    if n <= 20 {
        NUM[n].to_string()
    } else {
        format!("{}{}", TENS[n / 10], if n % 10 > 0 { NUM[n % 10] } else { "" })
    }
}

/// "Don't," → "dont", "3" → "three": what two spellings of a word have in common.
pub fn norm(w: &str) -> String {
    let s: String = w.to_lowercase().chars().filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit()).collect();
    if s == "versus" {
        return "vs".into(); // said in full, printed short ("EH vs. AE")
    }
    if !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()) {
        if let Ok(n) = s.parse::<usize>() {
            if n < 100 {
                return num_word(n);
            }
        }
    }
    s
}

/// Rough "sounds like" key so homophones match: read/red → rt, raise/race → rs, rend/rent → rnt.
pub fn sound_key(n: &str) -> String {
    static RULES: Lazy<Vec<(Regex, &'static str)>> = Lazy::new(|| {
        // (the Swift version used look-aheads; c/g before e,i,y are handled with a captured vowel)
        [
            ("th", "0"),
            ("ph", "f"),
            ("ck", "k"),
            ("c([eiy])", "s$1"),
            ("c", "k"),
            ("q", "k"),
            ("x", "ks"),
            ("z", "s"),
            ("d", "t"),
            ("g([eiy])", "j$1"),
            ("gh", ""),
            ("kn", "n"),
            ("wr", "r"),
            ("mb$", "m"),
        ]
        .iter()
        .map(|(p, t)| (Regex::new(p).unwrap(), *t))
        .collect()
    });
    let mut s: String = n.chars().filter(|c| !c.is_ascii_digit()).collect();
    for (re, t) in RULES.iter() {
        s = re.replace_all(&s, *t).into_owned();
    }
    let mut chars = s.chars();
    let Some(first) = chars.next() else { return String::new() };
    let rest: String = chars.filter(|c| !"aeiouyhw".contains(*c)).collect();
    let mut out = String::new();
    for c in std::iter::once(first).chain(rest.chars()) {
        if out.chars().last() != Some(c) {
            out.push(c);
        }
    }
    out
}

static FILLER: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    ["the", "a", "an", "to", "of", "in", "on", "and", "is", "it", "i", "so", "uh", "um", "oh"].into_iter().collect()
});

/// Whisper sometimes splits "o'clock" into "o" + "'clock".
pub fn merge_contractions(words: Vec<SpokenWord>) -> Vec<SpokenWord> {
    let mut out: Vec<SpokenWord> = Vec::with_capacity(words.len());
    for w in words {
        if let Some(prev) = out.last_mut() {
            if w.w.starts_with('\'') || w.w.starts_with('’') {
                prev.w.push_str(&w.w);
                prev.n = norm(&prev.w);
                prev.e = w.e;
                continue;
            }
        }
        out.push(w);
    }
    out
}

// ---------------------------------------------------------------- transcript ↔ PDF

fn lev(a: &[char], b: &[char]) -> usize {
    if a.is_empty() || b.is_empty() {
        return a.len().max(b.len());
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![0; b.len() + 1];
        cur[0] = i;
        for j in 1..=b.len() {
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + usize::from(a[i - 1] != b[j - 1]));
        }
        prev = cur;
    }
    prev[b.len()]
}

/// Edit distance over the longer length: 0 = same, 1 = nothing in common.
pub fn distance(a: &str, b: &str) -> f64 {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let m = a.len().max(b.len());
    if m == 0 {
        0.0
    } else {
        lev(&a, &b) as f64 / m as f64
    }
}

pub fn similar(a: &str, b: &str) -> f64 {
    if a == b {
        return 3.0;
    }
    if a.chars().count().min(b.chars().count()) < 4 {
        return -2.0;
    }
    // Fuzzy spelling only when both ends agree: keeps minimal pairs apart (think/thing, mouth/south).
    if a.chars().next() != b.chars().next() || a.chars().last() != b.chars().last() {
        return -2.0;
    }
    if 1.0 - distance(a, b) >= 0.75 { 1.5 } else { -2.0 }
}

/// In-order alignment: skipping spoken words (explanations) is cheap, skipping PDF words costs more.
pub fn align_to_pdf(t: &mut [SpokenWord], p: &mut [PdfWord]) {
    let (n, m) = (t.len(), p.len());
    let w = m + 1;
    const GAP_T: f64 = -0.5;
    const GAP_P: f64 = -0.8;
    let mut h = vec![0f64; (n + 1) * w];
    let mut bk = vec![0u8; (n + 1) * w]; // 1 diag, 2 skip spoken, 3 skip pdf
    for i in 1..=n {
        h[i * w] = i as f64 * GAP_T;
        bk[i * w] = 2;
    }
    for j in 1..=m {
        h[j] = j as f64 * GAP_P;
        bk[j] = 3;
    }
    for i in 1..=n {
        for j in 1..=m {
            let d = h[(i - 1) * w + j - 1] + similar(&t[i - 1].n, &p[j - 1].n);
            let u = h[(i - 1) * w + j] + GAP_T;
            let l = h[i * w + j - 1] + GAP_P;
            let k = i * w + j;
            if d >= u && d >= l {
                h[k] = d;
                bk[k] = 1;
            } else if u >= l {
                h[k] = u;
                bk[k] = 2;
            } else {
                h[k] = l;
                bk[k] = 3;
            }
        }
    }
    let (mut i, mut j) = (n, m);
    while i > 0 || j > 0 {
        match bk[i * w + j] {
            1 => {
                if similar(&t[i - 1].n, &p[j - 1].n) > 0.0 {
                    t[i - 1].pdf = (j - 1) as i32;
                    p[j - 1].tw = (i - 1) as i32;
                }
                i -= 1;
                j -= 1;
            }
            2 => i -= 1,
            _ => j -= 1,
        }
    }
    match_out_of_order(t, p);
}

/// Word grids are often read column by column while the PDF text runs row by row.
/// Match the leftovers by exact word, anchored on already matched neighbours.
fn match_out_of_order(t: &mut [SpokenWord], p: &mut [PdfWord]) {
    let mut by_n: HashMap<String, Vec<usize>> = HashMap::new();
    for (j, pw) in p.iter().enumerate() {
        by_n.entry(pw.n.clone()).or_default().push(j);
    }
    let free = |t: &[SpokenWord], p: &[PdfWord], i: usize| -> Vec<usize> {
        by_n.get(&t[i].n).map(|v| v.iter().copied().filter(|&j| p[j].tw < 0).collect()).unwrap_or_default()
    };
    fn assign(t: &mut [SpokenWord], p: &mut [PdfWord], i: usize, j: usize) {
        t[i].pdf = j as i32;
        p[j].tw = i as i32;
    }
    fn nearest(c: &[usize], target: i64) -> usize {
        let mut best = c[0];
        for &x in &c[1..] {
            if (x as i64 - target).abs() < (best as i64 - target).abs() {
                best = x;
            }
        }
        best
    }
    fn pdf_at(t: &[SpokenWord], i: i64) -> Option<i64> {
        if i >= 0 && (i as usize) < t.len() && t[i as usize].pdf >= 0 { Some(t[i as usize].pdf as i64) } else { None }
    }

    for i in 0..t.len() {
        if t[i].pdf < 0 && by_n.get(&t[i].n).map(|v| v.len()) == Some(1) {
            if let Some(&j) = free(t, p, i).first() {
                assign(t, p, i, j);
            }
        }
    }
    for _ in 0..2 {
        for i in 0..t.len() {
            if t[i].pdf >= 0 {
                continue;
            }
            let c = free(t, p, i);
            if c.is_empty() {
                continue;
            }
            for d in 1..=2i64 {
                let ii = i as i64;
                let target = pdf_at(t, ii - d).map(|x| x + d).or_else(|| pdf_at(t, ii + d).map(|x| x - d));
                let Some(target) = target else { continue };
                let j = nearest(&c, target);
                if (j as i64 - target).abs() <= 1 {
                    assign(t, p, i, j);
                    break;
                }
            }
        }
    }
    let mut last: i64 = 0;
    for i in 0..t.len() {
        if t[i].pdf >= 0 {
            last = t[i].pdf as i64;
            continue;
        }
        let c = if t[i].n.len() >= 3 && !FILLER.contains(t[i].n.as_str()) { free(t, p, i) } else { vec![] };
        if !c.is_empty() {
            assign(t, p, i, nearest(&c, last));
            last = t[i].pdf as i64;
        }
    }
    // Whisper may spell a word differently from the PDF ("Read" for red, "Raise" for race): match by sound.
    let mut by_key: HashMap<String, Vec<usize>> = HashMap::new();
    for (j, pw) in p.iter().enumerate() {
        if pw.n.len() >= 2 {
            by_key.entry(sound_key(&pw.n)).or_default().push(j);
        }
    }
    // only near where the neighbouring spoken words were found (grids jump a row = a few words)
    let anchor = |t: &[SpokenWord], i: usize| -> Option<i64> {
        for d in 1..=3i64 {
            if let Some(a) = pdf_at(t, i as i64 - d) {
                return Some(a);
            }
            if let Some(a) = pdf_at(t, i as i64 + d) {
                return Some(a);
            }
        }
        None
    };
    for i in 0..t.len() {
        if t[i].pdf >= 0 || t[i].n.len() < 3 || FILLER.contains(t[i].n.as_str()) {
            continue;
        }
        let Some(at) = anchor(t, i) else { continue };
        let c: Vec<usize> = by_key
            .get(&sound_key(&t[i].n))
            .map(|v| v.iter().copied().filter(|&j| p[j].tw < 0 && (j as i64 - at).abs() <= 12).collect())
            .unwrap_or_default();
        if !c.is_empty() {
            let j = nearest(&c, at);
            assign(t, p, i, j);
        }
    }
    fill_gaps(t, p);
    pull_strays(t, p);
    match_confusable(t, p);
    if std::env::var_os("RAM_ALIGN_BASELINE").is_some() {
        return;
    }
    // the page's shape: columns, cells, words split in two
    for _ in 0..2 {
        fit_grid(t, p);
        keep_cells_together(t, p);
        fill_columns(t, p);
        join_split_words(t, p);
    }
    // the last word on what goes where: the order the teacher reads the page in, learned from the
    // matches so far, then once more from its own result
    if std::env::var_os("RAM_ALIGN_NO_ORDER").is_none() {
        fill_in_reading_order(t, p);
    }
    one_word_for_two(t, p);
    letters_as_words(t, p);
}

/// A lone letter ("C") is often the recogniser's spelling of a word that starts with the letter's name
/// ("see" → seat). Once everything else is placed, it goes to a free printed word between its
/// neighbours that starts with that sound (the one nearest the next word), unless it was matched to
/// that very letter.
fn letters_as_words(t: &mut [SpokenWord], p: &mut [PdfWord]) {
    for i in 0..t.len() {
        let Some(name) = letter_name(&t[i].n) else { continue };
        if t[i].pdf >= 0 && p[t[i].pdf as usize].n == t[i].n {
            continue;
        }
        let left = t[..i].iter().rev().find(|w| w.pdf >= 0).map_or(-1, |w| w.pdf as i64);
        let right = t[i + 1..].iter().find(|w| w.pdf >= 0).map_or(p.len() as i64, |w| w.pdf as i64);
        if right - left > 12 {
            continue;
        }
        let key = sound_key(&name).chars().next();
        let Some(j) = ((left + 1)..right)
            .rev()
            .map(|j| j as usize)
            .find(|&j| p[j].tw < 0 && p[j].n.len() >= 3 && sound_key(&p[j].n).chars().next() == key)
        else {
            continue;
        };
        if t[i].pdf >= 0 {
            p[t[i].pdf as usize].tw = -1;
        }
        t[i].pdf = j as i32;
        p[j].tw = i as i32;
    }
}

/// The consonants of a word with any vowels and with voiced/voiceless pairs merged: what's left of a
/// word when the recogniser gets its vowel wrong ("look" for luck, "shod" for shot, "dine" for Don).
fn skeleton(n: &str) -> String {
    loose_key(n).chars().map(|c| match c {
        't' => 's', // loose_key already folds th/f/s/t together; d became t in sound_key
        'p' | 'b' => 'p',
        'k' | 'g' | 'j' => 'k',
        x => x,
    }).filter(|c| !"aeiouy".contains(*c)).collect()
}

/// How likely the recogniser wrote spoken word `a` for printed word `b` (a log probability).
fn heard_as(a: &str, b: &str) -> f64 {
    if a == b {
        0.0
    } else if sound_key(a) == sound_key(b) {
        (0.5f64).ln()
    } else if loose_key(a) == loose_key(b) {
        (0.3f64).ln()
    } else if !skeleton(a).is_empty() && skeleton(a) == skeleton(b) {
        (0.2f64).ln()
    } else {
        (0.01 + 0.2 * similar(a, b).max(0.0)).ln()
    }
}

/// Every spoken word, placed again in one go: each goes where it both sounds like the printed word and
/// fits the way the teacher moves through this page. That way is learned from the words that matched
/// for sure: down a column (+4, +6…), along a sentence (+1), pairs in a column (+1, +3, +1, +3…), and
/// the occasional jump to the next column or section. A word can also be off the page (the title said
/// differently, a remark). This is a Viterbi search over printed positions.
fn read_in_order(t: &mut [SpokenWord], p: &mut [PdfWord]) {
    let (n, m) = (t.len(), p.len());
    if n == 0 || m == 0 || n * m > 400_000 {
        return; // nothing to do, or too big to search (then the earlier passes stand)
    }
    // the steps between words said one after the other, from the sure matches
    let mut steps: HashMap<i64, f64> = HashMap::new();
    let mut total = 0.0;
    for k in 1..n {
        let (a, b) = (t[k - 1].pdf, t[k].pdf);
        if a >= 0 && b >= 0 && t[k - 1].n == p[a as usize].n && t[k].n == p[b as usize].n {
            *steps.entry(b as i64 - a as i64).or_default() += 1.0;
            total += 1.0;
        }
    }
    if total < 4.0 {
        return; // too little to learn the page's order from
    }
    // smoothed: most of the weight on steps seen here, some on any short step, a little on any jump
    let ln_step = |d: i64| -> f64 {
        let seen = steps.get(&d).copied().unwrap_or(0.0) / total;
        let short = if (1..=8).contains(&d.abs()) { 1.0 / 16.0 } else { 0.0 };
        (0.85 * seen + 0.1 * short + 0.05 / (2.0 * m as f64)).ln()
    };
    let lp: Vec<f64> = (-(m as i64)..=(m as i64)).map(ln_step).collect();
    let step = |d: i64| lp[(d + m as i64) as usize];
    let (on_page, off_page) = ((0.9f64).ln(), (0.1f64).ln());
    let off_word = |k: usize| if FILLER.contains(t[k].n.as_str()) { (0.2f64).ln() } else { (0.02f64).ln() };
    let emit: Vec<Vec<f64>> = (0..n).map(|k| (0..m).map(|j| heard_as(&t[k].n, &p[j].n)).collect()).collect();

    // state j < m: word k is printed word j; state m + j: word k is off the page, the last one on it was j
    // (m + m: none yet)
    let states = 2 * m + 1;
    let none = 2 * m;
    let neg = f64::NEG_INFINITY;
    let mut score = vec![neg; states];
    let mut back: Vec<Vec<u32>> = vec![vec![0; states]; n];
    for j in 0..m {
        score[j] = emit[0][j] + on_page - (m as f64).ln();
    }
    score[none] = off_word(0) + off_page;
    let pos = |s: usize| -> Option<usize> { if s < m { Some(s) } else if s < 2 * m { Some(s - m) } else { None } };
    for k in 1..n {
        let mut next = vec![neg; states];
        // best way into each printed word j: from any state, by the step from where the reading was
        for j in 0..m {
            let mut best = (neg, 0usize);
            for (s, &sc) in score.iter().enumerate() {
                if sc == neg {
                    continue;
                }
                let v = sc + match pos(s) { Some(i) => step(j as i64 - i as i64), None => -(m as f64).ln() };
                if v > best.0 {
                    best = (v, s);
                }
            }
            next[j] = best.0 + on_page + emit[k][j];
            back[k][j] = best.1 as u32;
        }
        // off the page: the reading stays where it was
        for s in 0..states {
            if score[s] == neg {
                continue;
            }
            let target = match pos(s) { Some(i) => m + i, None => none };
            let v = score[s] + off_page + off_word(k);
            if v > next[target] {
                next[target] = v;
                back[k][target] = s as u32;
            }
        }
        score = next;
    }
    let mut s = (0..states).max_by(|&a, &b| score[a].partial_cmp(&score[b]).unwrap()).unwrap();
    let mut placed = vec![-1i32; n];
    for k in (0..n).rev() {
        if s < m {
            placed[k] = s as i32;
        }
        s = back[k][s] as usize;
    }
    // a printed word said twice (not one word in pieces): the closer-sounding one keeps it
    let mut owner: HashMap<i32, usize> = HashMap::new();
    for k in 0..n {
        let j = placed[k];
        if j < 0 {
            continue;
        }
        match owner.get(&j).copied() {
            Some(o) if o + 1 == k && placed[o] == j => {} // pieces of one word
            Some(o) if emit[o][j as usize] >= emit[k][j as usize] => placed[k] = -1,
            Some(o) => {
                placed[o] = -1;
                owner.insert(j, k);
            }
            None => {
                owner.insert(j, k);
            }
        }
    }
    for w in p.iter_mut() {
        w.tw = -1;
    }
    for k in 0..n {
        t[k].pdf = placed[k];
        if placed[k] >= 0 && p[placed[k] as usize].tw < 0 {
            p[placed[k] as usize].tw = k as i32;
        }
    }
}

/// The reading-order search ([`read_in_order`]) on a copy, used only where the passes before left a
/// word unplaced, or placed where nothing backs it up: there it takes the search's spot if that's free.
/// What's already backed up stays (the search alone ties on repeated words like "the", and gives up on
/// badly misheard words more easily than the layout passes do).
fn fill_in_reading_order(t: &mut [SpokenWord], p: &mut [PdfWord]) {
    let (mut t2, mut p2) = (t.to_vec(), p.to_vec());
    read_in_order(&mut t2, &mut p2);
    read_in_order(&mut t2, &mut p2);
    let n = t.len();
    // words free to move: unplaced, or placed where nothing backs them up
    let movable: Vec<bool> = (0..n).map(|k| t[k].pdf < 0 || !supported(t, k, t[k].pdf as i64)).collect();
    // ...to the search's spot, if it could have been heard as that word
    let goes: Vec<Option<usize>> = (0..n)
        .map(|k| {
            let w = t2[k].pdf;
            (movable[k] && w >= 0 && w != t[k].pdf && heard_as(&t[k].n, &p[w as usize].n) >= (0.05f64).ln()).then_some(w as usize)
        })
        .collect();
    // a spot is open if it's free, or its holder is moving away too
    let open = |j: usize, t: &[SpokenWord], p: &[PdfWord]| -> bool {
        let h = p[j].tw;
        h < 0 || goes[h as usize].is_some_and(|g| g != j) && t[h as usize].pdf == j as i32
    };
    let moving: Vec<usize> = (0..n).filter(|&k| goes[k].is_some_and(|j| open(j, t, p))).collect();
    for &k in &moving {
        if t[k].pdf >= 0 {
            p[t[k].pdf as usize].tw = -1;
            t[k].pdf = -1;
        }
    }
    for &k in &moving {
        let j = goes[k].unwrap();
        if p[j].tw < 0 {
            t[k].pdf = j as i32;
            p[j].tw = k as i32;
        }
    }
}

/// One spoken word for two printed ones ("team's" for team is, "I'm" for I am, "Northside" for north
/// side): the printed word skipped between it and the next one is heard too, if the letters agree.
fn one_word_for_two(t: &mut [SpokenWord], p: &mut [PdfWord]) {
    // an unplaced word whose letters are two free printed words in a row, near its neighbours
    let at = |t: &[SpokenWord], k: i64| -> Option<i64> { (k >= 0 && (k as usize) < t.len() && t[k as usize].pdf >= 0).then(|| t[k as usize].pdf as i64) };
    for k in 0..t.len() {
        if t[k].pdf >= 0 || t[k].n.len() < 4 {
            continue;
        }
        let Some(anchor) = (1..=3i64).find_map(|d| at(t, k as i64 - d).map(|x| x + d).or_else(|| at(t, k as i64 + d).map(|x| x - d))) else { continue };
        let best = (0..p.len().saturating_sub(1))
            .filter(|&j| p[j].tw < 0 && p[j + 1].tw < 0 && p[j].chunk == p[j + 1].chunk && (j as i64 - anchor).abs() <= 12)
            .filter(|&j| distance(&t[k].n, &format!("{}{}", p[j].n, p[j + 1].n)) <= 0.25)
            .min_by_key(|&j| (j as i64 - anchor).abs());
        if let Some(j) = best {
            t[k].pdf = j as i32;
            p[j].tw = k as i32;
            p[j + 1].tw = k as i32;
        }
    }
    for k in 0..t.len() {
        let j = t[k].pdf;
        if j < 0 || j as usize + 1 >= p.len() || p[j as usize + 1].tw >= 0 {
            continue;
        }
        let (a, b) = (&p[j as usize], &p[j as usize + 1]);
        if a.chunk != b.chunk || t[k].n == a.n {
            continue;
        }
        let joined = format!("{}{}", a.n, b.n);
        if distance(&t[k].n, &joined) <= 0.25 {
            p[j as usize + 1].tw = k as i32;
        }
    }
}

/// Spoken word o's printed word j is firmly backed: two words in a row next to it agree, going on
/// through the text or down a column. The layout passes never move such a word.
pub fn firmly_supported(t: &[SpokenWord], o: usize, j: i64) -> bool {
    let at = |k: i64| -> Option<i64> { (k >= 0 && (k as usize) < t.len() && t[k as usize].pdf >= 0).then(|| t[k as usize].pdf as i64) };
    let o = o as i64;
    for dir in [-1i64, 1] {
        let (Some(a), Some(b)) = (at(o + dir), at(o + 2 * dir)) else { continue };
        let step = (a - j) * dir;
        if (1..=8).contains(&step) && (b - a) * dir == step {
            return true;
        }
    }
    false
}

/// Spoken word o's printed word j is backed by its neighbours: the word before (or after) it is the
/// previous (next) word in the text, or two words on one side go down the same column with it.
pub fn supported(t: &[SpokenWord], o: usize, j: i64) -> bool {
    let at = |k: i64| -> Option<i64> { (k >= 0 && (k as usize) < t.len() && t[k as usize].pdf >= 0).then(|| t[k as usize].pdf as i64) };
    let o = o as i64;
    if at(o - 1) == Some(j - 1) || at(o + 1) == Some(j + 1) {
        return true; // running text
    }
    if at(o - 1) == Some(j) || at(o + 1) == Some(j) {
        // one printed word said in two pieces ("all together"): backed if the pieces are
        let other = if at(o - 1) == Some(j) { o - 1 } else { o + 1 };
        let (lo, hi) = (o.min(other), o.max(other));
        return at(lo - 1) == Some(j - 1) || at(hi + 1) == Some(j + 1) || at(lo - 1).is_some_and(|x| (2..=8).contains(&(j - x))) || at(hi + 1).is_some_and(|x| (2..=8).contains(&(x - j)));
    }
    let column = |n1: Option<i64>, n2: Option<i64>, dir: i64| -> bool {
        // n1 next to o, n2 one further, both in the direction `dir`
        match (n1, n2) {
            (Some(a), Some(b)) => {
                let step = (a - j) * dir;
                step >= 2 && step <= 8 && (b - a) * dir == step
            }
            _ => false,
        }
    };
    column(at(o - 1), at(o - 2), -1) || column(at(o + 1), at(o + 2), 1)
}

/// Down a column, the words said between two matched words go on the rows between them, in order
/// (like [`fill_gaps`] for text, with the column's step): "father, rather, rather, bathe" read down
/// father · weather · rather · bathe puts the first "rather" on weather.
fn fill_columns(t: &mut [SpokenWord], p: &mut [PdfWord]) {
    let n = t.len();
    for a in 0..n {
        if t[a].pdf < 0 {
            continue;
        }
        for b in (a + 2)..=(a + 4).min(n.saturating_sub(1)) {
            if t[b].pdf < 0 {
                continue;
            }
            let (x, y) = (t[a].pdf as i64, t[b].pdf as i64);
            let span = (b - a) as i64;
            if (y - x) % span != 0 {
                continue;
            }
            let step = (y - x) / span;
            if !(2..=8).contains(&step) {
                continue;
            }
            // a column for sure: the word before a and the one after b are one more step along it
            let sure = a > 0 && t[a - 1].pdf >= 0 && t[a - 1].pdf as i64 == x - step && b + 1 < n && t[b + 1].pdf as i64 == y + step;
            if !sure {
                continue;
            }
            let targets: Vec<(usize, usize)> = ((a + 1)..b).map(|k| (k, (x + (k - a) as i64 * step) as usize)).collect();
            if targets.iter().all(|&(k, j)| t[k].pdf == j as i32) {
                break; // already in place
            }
            // every row must be free, held by one of these words, or held by a word from elsewhere that
            // nothing backs up there (and this word could have been heard as it)
            let run = (a + 1)..b;
            let ok = targets.iter().all(|&(k, j)| {
                j < p.len()
                    // only words nothing backs up where they are move (or fill a row they weren't on)
                    && (t[k].pdf < 0 || !supported(t, k, t[k].pdf as i64))
                    && (p[j].tw < 0 || run.contains(&(p[j].tw as usize)) || !supported(t, p[j].tw as usize, j as i64))
            });
            if !ok {
                break;
            }
            let mut displaced = vec![];
            for &(k, j) in &targets {
                if t[k].pdf >= 0 {
                    p[t[k].pdf as usize].tw = -1;
                    t[k].pdf = -1;
                }
                let o = p[j].tw;
                if o >= 0 && !run.contains(&(o as usize)) {
                    t[o as usize].pdf = -1;
                    p[j].tw = -1;
                    displaced.push(o as usize);
                }
            }
            for &(k, j) in &targets {
                t[k].pdf = j as i32;
                p[j].tw = k as i32;
            }
            // what was moved out finds its place when its own column comes up (or is rematched)
            for o in displaced {
                if o < a {
                    rematch(t, p, o);
                }
            }
            break;
        }
    }
    // a word said in a column's gap whose row was taken by a word that nothing backs: it takes the row
    for k in 1..n.saturating_sub(1) {
        let (Some(x), Some(y)) = ((t[k - 1].pdf >= 0).then(|| t[k - 1].pdf as i64), (t[k + 1].pdf >= 0).then(|| t[k + 1].pdf as i64)) else { continue };
        let step = (y - x) / 2;
        if t[k].pdf >= 0 || (y - x) % 2 != 0 || !(2..=8).contains(&step) {
            continue;
        }
        let j = (x + step) as usize;
        let backed = (k >= 2 && t[k - 2].pdf as i64 == x - step) || (k + 2 < n && t[k + 2].pdf as i64 == y + step);
        if !backed || j >= p.len() || !hearable(&t[k].n, &p[j].n) {
            continue;
        }
        let other = p[j].tw;
        if other >= 0 && supported(t, other as usize, j as i64) {
            continue;
        }
        if other >= 0 {
            t[other as usize].pdf = -1;
        }
        t[k].pdf = j as i32;
        p[j].tw = k as i32;
        if other >= 0 {
            rematch(t, p, other as usize);
        }
    }
}

/// The recogniser sometimes splits a printed word in two ("all together" for altogether): two words
/// said back to back that match nothing, whose letters together are close to a free printed word near
/// their neighbours, both go to it.
fn join_split_words(t: &mut [SpokenWord], p: &mut [PdfWord]) {
    for i in 0..t.len().saturating_sub(1) {
        if t[i].pdf >= 0 || t[i + 1].pdf >= 0 || t[i + 1].s - t[i].e > 0.3 {
            continue;
        }
        let joined = format!("{}{}", t[i].n, t[i + 1].n);
        if joined.len() < 5 {
            continue;
        }
        let anchor = (1..=3i64).find_map(|d| {
            let before = i as i64 - d;
            let after = i as i64 + 1 + d;
            (before >= 0 && t[before as usize].pdf >= 0).then(|| t[before as usize].pdf as i64).or_else(|| {
                ((after as usize) < t.len() && t[after as usize].pdf >= 0).then(|| t[after as usize].pdf as i64)
            })
        });
        let Some(at) = anchor else { continue };
        let best = (0..p.len())
            .filter(|&j| p[j].tw < 0 && (j as i64 - at).abs() <= 12 && distance(&joined, &p[j].n) <= 0.2)
            .min_by(|&x, &y| distance(&joined, &p[x].n).partial_cmp(&distance(&joined, &p[y].n)).unwrap());
        if let Some(j) = best {
            t[i].pdf = j as i32;
            t[i + 1].pdf = j as i32;
            p[j].tw = i as i32;
        }
    }
}

/// Words printed together (a grid cell like "that one") and said back to back belong together. When
/// the recogniser misheard one of them ("Dow one") and its printed word went to the same word said
/// somewhere else, where nothing around it fits, the cell gets it back; the other one then takes a
/// free copy of its word near its own neighbours.
fn keep_cells_together(t: &mut [SpokenWord], p: &mut [PdfWord]) {
    let at = |t: &[SpokenWord], k: i64| -> Option<i64> {
        (k >= 0 && (k as usize) < t.len() && t[k as usize].pdf >= 0).then(|| t[k as usize].pdf as i64)
    };
    // 1. A word heard right before its cell-mate, and printed right before it in that cell, is that
    //    copy ("the hour": the "the" of the hour cell, not the next "the" in reading order). All such
    //    words move at once, so copies of one word ("the", "the", ...) sort themselves out.
    let mut want: Vec<(usize, usize)> = vec![];
    for i in 0..t.len().saturating_sub(1) {
        let Some(m) = at(t, i as i64 + 1) else { continue };
        let j = m as usize;
        if j == 0 || t[i + 1].s - t[i].e > 0.3 || t[i].pdf == (j - 1) as i32 {
            continue;
        }
        let j = j - 1;
        if p[j].chunk == p[j + 1].chunk && p[j].n == t[i].n && (t[i].pdf < 0 || !firmly_supported(t, i, t[i].pdf as i64)) {
            want.push((i, j));
        }
    }
    let movers: std::collections::HashSet<usize> = want.iter().map(|&(i, _)| i).collect();
    // a spot held by a word that isn't moving, and that fits where it is, stays with it
    want.retain(|&(_, j)| p[j].tw < 0 || movers.contains(&(p[j].tw as usize)) || !supported(t, p[j].tw as usize, j as i64));
    let mut freed = vec![];
    for &(i, j) in &want {
        if t[i].pdf >= 0 {
            p[t[i].pdf as usize].tw = -1;
        }
        let o = p[j].tw;
        if o >= 0 && !movers.contains(&(o as usize)) {
            t[o as usize].pdf = -1;
            freed.push(o as usize);
        }
        t[i].pdf = -1;
    }
    for &(i, j) in &want {
        t[i].pdf = j as i32;
        p[j].tw = i as i32;
    }
    for o in freed {
        rematch(t, p, o);
    }
    // 2. A word said next to its cell-mate that matched nothing (misheard: "Dow one" for that one)
    for i in 0..t.len() {
        if t[i].pdf >= 0 || t[i].n.is_empty() {
            continue;
        }
        // its printed spot, from the word said right after or right before it in the same cell
        let spot = [1i64, -1].iter().find_map(|&d| {
            let k = i as i64 + d;
            let m = at(t, k)?;
            let gap = if d > 0 { t[k as usize].s - t[i].e } else { t[i].s - t[k as usize].e };
            let j = m - d;
            (gap < 0.3 && j >= 0 && (j as usize) < p.len() && p[j as usize].chunk == p[m as usize].chunk).then_some(j as usize)
        });
        let Some(j) = spot else { continue };
        let other = p[j].tw;
        if other >= 0 && supported(t, other as usize, j as i64) {
            continue; // the word that has it fits where it is
        }
        // a free spot takes only a word that could be heard as it; a taken one (the same word said
        // somewhere it doesn't fit) shows the word was said here, misheard ("Dow one") — if that word
        // was said well away from here and has another copy of itself to go to
        if other < 0 && !hearable(&t[i].n, &p[j].n) {
            continue;
        }
        if other >= 0 && ((other as usize).abs_diff(i) < 4 || rematch_spot(t, p, other as usize, Some(j)).is_none()) {
            continue;
        }
        if other >= 0 {
            t[other as usize].pdf = -1;
        }
        t[i].pdf = j as i32;
        p[j].tw = i as i32;
        if other >= 0 {
            rematch(t, p, other as usize);
        }
    }
}

/// A spoken word that lost its printed word takes a free copy of it next to where its neighbours are.
fn rematch(t: &mut [SpokenWord], p: &mut [PdfWord], o: usize) {
    if let Some(j) = rematch_spot(t, p, o, None) {
        t[o].pdf = j as i32;
        p[j].tw = o as i32;
    }
}

/// Where spoken word o would go if it lost its printed word: a free copy of it next to its neighbours
/// (`except`: a spot about to be taken).
fn rematch_spot(t: &[SpokenWord], p: &[PdfWord], o: usize, except: Option<usize>) -> Option<usize> {
    let at = |k: i64| -> Option<i64> { (k >= 0 && (k as usize) < t.len() && t[k as usize].pdf >= 0).then(|| t[k as usize].pdf as i64) };
    let target = (1..=3i64).find_map(|d| at(o as i64 + d).map(|x| x - d).or_else(|| at(o as i64 - d).map(|x| x + d)))?;
    (0..p.len())
        .filter(|&j| p[j].tw < 0 && Some(j) != except && p[j].n == t[o].n && (j as i64 - target).abs() <= 12)
        .min_by_key(|&j| (j as i64 - target).abs())
}

/// Could the recogniser have heard this printed word as this spoken one? (same word, homophone, or
/// a commonly confused sound: "father" for feather, "lather" for leather)
fn hearable(spoken: &str, printed: &str) -> bool {
    spoken == printed || sound_key(spoken) == sound_key(printed) || loose_key(spoken) == loose_key(printed) || similar(spoken, printed) > 0.0
}

/// Grids read down the columns: a word said between two matched words belongs halfway between them in
/// the PDF (the column's next row). When a word was matched elsewhere, often because the recogniser
/// heard another word that's printed in the grid too ("father" for feather), it moves to that spot,
/// or trades places with the word that took it.
fn fit_grid(t: &mut [SpokenWord], p: &mut [PdfWord]) {
    let at = |t: &[SpokenWord], k: i64| -> Option<i64> {
        (k >= 0 && (k as usize) < t.len() && t[k as usize].pdf >= 0).then(|| t[k as usize].pdf as i64)
    };
    // the printed spot halfway between the matched words just before and after spoken word i
    let midpoint = |t: &[SpokenWord], i: usize| -> Option<i64> {
        let (a, b) = (at(t, i as i64 - 1)?, at(t, i as i64 + 1)?);
        ((b - a) % 2 == 0 && (b - a) / 2 >= 2 && (b - a) / 2 <= 8).then_some((a + b) / 2)
    };
    // sure it's a column: two matched words on each side, all one step (a grid row) apart
    let column_spot = |t: &[SpokenWord], i: usize| -> Option<usize> {
        let ii = i as i64;
        let (a1, a, b, b2) = (at(t, ii - 2)?, at(t, ii - 1)?, at(t, ii + 1)?, at(t, ii + 2)?);
        let step = a - a1;
        (step >= 2 && step <= 8 && b - a == 2 * step && b2 - b == step).then_some((a + step) as usize)
    };
    let misfit = |t: &[SpokenWord], i: usize, j: usize| -> i64 { midpoint(t, i).map(|e| (j as i64 - e).abs()).unwrap_or(0) };
    for i in 0..t.len() {
        if t[i].pdf < 0 {
            continue;
        }
        let j = t[i].pdf as usize;
        let Some(k) = column_spot(t, i) else { continue };
        // two copies of the same printed word never trade places here (pull_strays sees to those)
        if k == j || k >= p.len() || p[k].n == p[j].n || !hearable(&t[i].n, &p[k].n) || firmly_supported(t, i, j as i64) {
            continue;
        }
        let other = p[k].tw;
        if other < 0 {
            p[j].tw = -1;
            t[i].pdf = k as i32;
            p[k].tw = i as i32;
        } else {
            let o = other as usize;
            // trade places only if the other word could be this one's printed word too, and it fits as well
            if hearable(&t[o].n, &p[j].n) && misfit(t, o, j) <= misfit(t, o, k) {
                t[i].pdf = k as i32;
                p[k].tw = i as i32;
                t[o].pdf = j as i32;
                p[j].tw = o as i32;
            }
        }
    }
}

/// The sounds learners (and the recogniser) mix up: TH heard as S/F/T/D ("thin" → "sin"), V/W, R/L.
fn loose_key(n: &str) -> String {
    sound_key(n).chars().map(|c| match c {
        '0' | 'f' | 's' | 't' => 's',
        'v' | 'w' => 'w',
        'r' | 'l' => 'r',
        x => x,
    }).collect()
}

/// Last pass, leftovers only: a spoken word that matched nothing takes a printed word that also matched
/// nothing, if they differ only by a commonly confused sound and it's printed near the neighbours' words.
fn match_confusable(t: &mut [SpokenWord], p: &mut [PdfWord]) {
    let pdf_at = |t: &[SpokenWord], i: i64| -> Option<i64> {
        (i >= 0 && (i as usize) < t.len() && t[i as usize].pdf >= 0).then(|| t[i as usize].pdf as i64)
    };
    for i in 0..t.len() {
        if t[i].pdf >= 0 || t[i].n.len() < 2 || FILLER.contains(t[i].n.as_str()) {
            continue;
        }
        let anchor = (1..=3i64).find_map(|d| pdf_at(t, i as i64 - d).or_else(|| pdf_at(t, i as i64 + d)));
        let Some(at) = anchor else { continue };
        let key = loose_key(&t[i].n);
        let c: Vec<usize> = (0..p.len())
            .filter(|&j| p[j].tw < 0 && (j as i64 - at).abs() <= 12 && p[j].n.len() >= 2 && loose_key(&p[j].n) == key)
            .collect();
        if c.len() == 1 {
            t[i].pdf = c[0] as i32;
            p[c[0]].tw = i as i32;
        }
    }
}

/// A word matched far from its matched neighbours ("the" from the instructions instead of the "The" that
/// starts the sentence) moves to a free copy of itself right next to them.
fn pull_strays(t: &mut [SpokenWord], p: &mut [PdfWord]) {
    let near = |t: &[SpokenWord], i: usize, dir: i64| -> Option<i64> {
        (1..=3i64).map(|d| i as i64 + d * dir).filter(|&k| k >= 0 && (k as usize) < t.len()).find_map(|k| {
            let w = t[k as usize].pdf;
            (w >= 0).then_some(w as i64)
        })
    };
    for i in 0..t.len() {
        if t[i].pdf < 0 {
            continue;
        }
        let j = t[i].pdf as i64;
        let (before, after) = (near(t, i, -1), near(t, i, 1));
        // where it should be: just before the next match, or just after the previous one
        let target = match (before, after) {
            (_, Some(a)) if j < a - 3 || j > a => Some(a - 1),
            (Some(b), None) if j > b + 3 || j < b => Some(b + 1),
            _ => None,
        };
        let Some(target) = target else { continue };
        if let Some(b) = before {
            if (j - b).abs() <= 3 && j > b {
                continue; // fits with the word before it: leave it
            }
        }
        let cands: Vec<i64> = (target - 2..=target + 2)
            .filter(|&k| k >= 0 && (k as usize) < p.len() && p[k as usize].tw < 0 && p[k as usize].n == t[i].n)
            .collect();
        if let Some(&k) = cands.iter().min_by_key(|&&k| (k - target).abs()) {
            p[j as usize].tw = -1;
            t[i].pdf = k as i32;
            p[k as usize].tw = i as i32;
        }
    }
}

/// Between two matched words, the leftover spoken words fill the leftover printed words in order: the
/// PDF says what was said even where the recogniser misheard it ("c" for seat, "be in" for bean).
fn fill_gaps(t: &mut [SpokenWord], p: &mut [PdfWord]) {
    // anchors: matched words whose PDF positions keep increasing
    let mut anchors: Vec<(i64, i64)> = vec![(-1, -1)];
    for (i, w) in t.iter().enumerate() {
        if w.pdf >= 0 && w.pdf as i64 > anchors.last().unwrap().1 {
            anchors.push((i as i64, w.pdf as i64));
        }
    }
    anchors.push((t.len() as i64, p.len() as i64));
    for win in anchors.windows(2) {
        let ((i1, j1), (i2, j2)) = (win[0], win[1]);
        let us: Vec<usize> = ((i1 + 1)..i2).map(|i| i as usize).filter(|&i| t[i].pdf < 0).collect();
        let vs: Vec<usize> = ((j1 + 1)..j2).map(|j| j as usize).filter(|&j| p[j].tw < 0).collect();
        if us.is_empty() || vs.is_empty() || us.len() > 2 * vs.len() {
            continue;
        }
        // the very start and end are often the teacher talking: only fill them when they look alike
        let edge = i1 < 0 || i2 == t.len() as i64;
        // words that run together without a pause are one printed word ("be in" → bean)
        let mut groups: Vec<Vec<usize>> = vec![];
        for &i in &us {
            match groups.last_mut() {
                Some(g) if us.len() > vs.len() && t[i].s - t[*g.last().unwrap()].e < 0.15 && *g.last().unwrap() + 1 == i => g.push(i),
                _ => groups.push(vec![i]),
            }
        }
        let alike = |g: &Vec<usize>, j: usize| sounds_alike(t, p, g, j);
        // fewer words heard than printed (column headers, a word the recogniser lost): they go to the
        // printed words at one end of the gap, when every one of them sounds alike
        let vs: Vec<usize> = if groups.len() == vs.len() {
            vs
        } else if groups.len() < vs.len() {
            let n = groups.len();
            let fits = |w: &[usize]| groups.iter().zip(w).all(|(g, &j)| alike(g, j));
            if fits(&vs[vs.len() - n..]) {
                vs[vs.len() - n..].to_vec()
            } else if fits(&vs[..n]) {
                vs[..n].to_vec()
            } else {
                continue;
            }
        } else {
            continue;
        };
        for (g, &j) in groups.iter().zip(&vs) {
            if edge && !sounds_alike(t, p, g, j) {
                continue;
            }
            let span = t[*g.last().unwrap()].e - t[g[0]].s;
            if span > 3.0 {
                continue; // a long stretch is the teacher talking, not one printed word
            }
            for &i in g {
                t[i].pdf = j as i32;
            }
            p[j].tw = g[0] as i32;
        }
    }
}

/// The words heard in `g` start like printed word `j` (a lone letter counts as its name).
fn sounds_alike(t: &[SpokenWord], p: &[PdfWord], g: &[usize], j: usize) -> bool {
    let heard: String = g.iter().map(|&i| t[i].n.as_str()).collect();
    let heard = letter_name(&heard).unwrap_or(heard);
    heard.chars().next() == p[j].n.chars().next()
        || sound_key(&heard).chars().next() == sound_key(&p[j].n).chars().next()
        || distance(&heard, &p[j].n) <= 0.5
}

/// A letter said on its own is heard as its name: "C" for "see" (or the start of "seat").
fn letter_name(n: &str) -> Option<String> {
    const NAMES: [&str; 26] = [
        "ay", "bee", "see", "dee", "ee", "ef", "jee", "aych", "eye", "jay", "kay", "el", "em", "en", "oh", "pee", "cue",
        "ar", "es", "tee", "you", "vee", "doubleyou", "ex", "why", "zee",
    ];
    let mut c = n.chars();
    match (c.next(), c.next()) {
        // "a" and "i" are words, not letters
        (Some(l @ 'b'..='z'), None) if l != 'i' => Some(NAMES[(l as u8 - b'a') as usize].to_string()),
        _ => None,
    }
}

static SENT_END: Lazy<Regex> = Lazy::new(|| Regex::new(r#"[.?!…]["”’')]*$"#).unwrap());

pub fn build_phrases(w: &[SpokenWord], p: &[PdfWord]) -> Vec<Phrase> {
    let mut phrases: Vec<Phrase> = Vec::new();
    let mut a = 0;
    for i in 1..=w.len() {
        let prev = &w[i - 1];
        let gap = if i < w.len() { w[i].s - prev.e } else { f64::INFINITY };
        // A sentence ends where the PDF prints its full stop (Whisper's punctuation only counts at a pause).
        let printed_end = prev.pdf >= 0 && SENT_END.is_match(&p[prev.pdf as usize].text);
        let sentence_end = printed_end || (SENT_END.is_match(&prev.w) && gap > 0.25);
        // Words the PDF prints apart (grid cells, phrases between ·) are separate phrases even when the
        // teacher reads them without a pause: "church, search" are two words to repeat, not one phrase.
        let apart =
            i < w.len() && prev.pdf >= 0 && w[i].pdf >= 0 && p[prev.pdf as usize].chunk != p[w[i].pdf as usize].chunk;
        // Inside a printed sentence the teacher's pauses don't split it: you repeat whole sentences.
        let in_sentence = i < w.len() && prev.pdf >= 0 && w[i].pdf >= 0 && !apart && !sentence_end;
        if sentence_end || (apart && gap > 0.05) || (gap > 0.6 && !in_sentence) || gap > 2.5 || i - a >= 40 {
            let matched = w[a..i].iter().filter(|x| x.pdf >= 0).count();
            phrases.push(Phrase {
                a,
                b: i,
                start: w[a].s,
                end: prev.e,
                stop: 0.0,
                in_pdf: matched as f64 / (i - a) as f64 >= 0.5,
            });
            a = i;
        }
    }
    let n = phrases.len();
    for k in 0..n {
        let p = &phrases[k];
        phrases[k].stop =
            if k + 1 < n { (p.end + 0.25).min((p.end + phrases[k + 1].start) / 2.0) } else { p.end + 0.3 };
    }
    phrases
}

/// A word the way the PDF writes it ("three", not "3"), keeping the spoken word's end punctuation.
pub fn label(w: &SpokenWord, pdf: &[PdfWord]) -> String {
    static TRAIL: Lazy<Regex> = Lazy::new(|| Regex::new(r"[.,!?;:]+$").unwrap());
    if w.pdf < 0 || w.pdf as usize >= pdf.len() {
        return w.w.clone();
    }
    let text = TRAIL.replace(&pdf[w.pdf as usize].text, "").into_owned();
    let punct = TRAIL.find(&w.w).map(|m| m.as_str()).unwrap_or("");
    text + punct
}

/// What a phrase says, written the way the PDF prints it.
pub fn phrase_text(ph: &Phrase, w: &[SpokenWord], pdf: &[PdfWord]) -> String {
    let mut out: Vec<String> = vec![];
    for k in ph.a..ph.b.min(w.len()) {
        if k > ph.a && w[k].pdf >= 0 && w[k].pdf == w[k - 1].pdf {
            continue; // several spoken pieces of one printed word
        }
        out.push(label(&w[k], pdf));
    }
    out.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pw(text: &str, chunk: u32) -> PdfWord {
        PdfWord { text: text.into(), n: norm(text), page: 0, rect: Rect::default(), tw: -1, chunk }
    }

    #[test]
    fn grid_read_by_column_keeps_misheard_words_in_their_column() {
        // two columns, printed row by row; the teacher reads down each column, and the recogniser hears
        // "father" for feather in the first one, while the second one really has father
        // four columns, like the lesson; the other two don't matter here
        let left = ["thus", "brother", "feather", "either", "smooth", "worthy"];
        let right = ["though", "father", "rather", "bathe", "soothe", "within"];
        let printed: Vec<&str> = left.iter().zip(right.iter()).flat_map(|(a, b)| [*a, *b, "c3", "c4"]).collect();
        let mut p: Vec<PdfWord> = printed.iter().enumerate().map(|(i, w)| pw(w, i as u32)).collect();
        let mut said: Vec<&str> = left.to_vec();
        said[2] = "father";
        said.extend(right);
        let mut t: Vec<SpokenWord> = said.iter().enumerate().map(|(i, w)| SpokenWord::new(w, i as f64, i as f64 + 0.5)).collect();
        align_to_pdf(&mut t, &mut p);
        let got: Vec<&str> = t.iter().map(|w| if w.pdf >= 0 { printed[w.pdf as usize] } else { "-" }).collect();
        let want: Vec<&str> = left.iter().chain(right.iter()).copied().collect();
        assert_eq!(got, want);
    }

    #[test]
    fn a_misheard_word_stays_with_its_cell() {
        // "that one" is one cell; the teacher says it, the recogniser hears "Dow one", and the printed
        // "that" went to a "that" said later, in another column ("that" printed at 5 is free)
        let printed = ["southwest", "that", "one", "a", "b", "c", "otherwise", "x", "that", "them", "than"];
        let mut p: Vec<PdfWord> = printed.iter().enumerate().map(|(i, w)| pw(w, if i == 1 || i == 2 { 1 } else { 10 + i as u32 })).collect();
        let said = ["southwest", "Dow", "one", "a", "b", "c", "otherwise", "that", "them", "than"];
        let mut t: Vec<SpokenWord> = said.iter().enumerate().map(|(i, w)| SpokenWord::new(w, i as f64, i as f64 + 0.5)).collect();
        t[1].e = 1.95; // "Dow" runs into "one"
        t[2].s = 2.0;
        for (i, j) in [(0, 0), (2, 2), (3, 3), (4, 4), (5, 5), (6, 6), (7, 1), (8, 9), (9, 10)] {
            t[i].pdf = j;
            p[j as usize].tw = i as i32;
        }
        keep_cells_together(&mut t, &mut p);
        assert_eq!(t[1].pdf, 1, "the cell gets its 'that' back");
        assert_eq!(t[7].pdf, 8, "the other 'that' takes the free one by its neighbours");
    }

    #[test]
    fn each_the_goes_with_its_own_cell() {
        // cells "the X" in two columns, printed row by row, read down the columns
        let cells = [["the", "apple"], ["the", "hour"], ["the", "entrance"], ["the", "ocean"], ["the", "icon"], ["the", "exit"]];
        let printed: Vec<&str> = cells.iter().flatten().copied().collect();
        let mut p: Vec<PdfWord> = printed.iter().enumerate().map(|(i, w)| pw(w, (i / 2) as u32)).collect();
        // column 1: cells 0, 2, 4; column 2: cells 1, 3, 5
        let said: Vec<&str> = [0, 2, 4, 1, 3, 5].iter().flat_map(|&c| cells[c]).collect();
        let mut t: Vec<SpokenWord> = said.iter().enumerate().map(|(i, w)| SpokenWord::new(w, i as f64 * 0.5, i as f64 * 0.5 + 0.4)).collect();
        align_to_pdf(&mut t, &mut p);
        for k in (0..t.len()).step_by(2) {
            assert_eq!(t[k].pdf + 1, t[k + 1].pdf, "'the' before '{}' is that cell's", t[k + 1].w);
        }
    }

    #[test]
    fn misheard_words_in_two_columns_sort_out() {
        // four columns printed row by row, read down each; the recogniser hears raise for race,
        // reign for rain and ran for rent, and raise really is in column 2
        // the rows as the R lesson prints them
        let rows = [["rich", "round", "x0", "y0"], ["room", "rough", "x1", "y1"], ["rock", "rent", "x2", "y2"], ["race", "raise", "x3", "y3"], ["rain", "raw", "x4", "y4"], ["run", "rise", "x5", "y5"], ["rule", "risk", "x6", "y6"]];
        let printed: Vec<&str> = rows.iter().flatten().copied().collect();
        let mut p: Vec<PdfWord> = printed.iter().enumerate().map(|(i, w)| pw(w, i as u32)).collect();
        let col = |c: usize| rows.iter().map(move |r| r[c]);
        let mut said: Vec<&str> = col(0).chain(col(1)).collect();
        said[3] = "raise"; // race
        said[4] = "reign"; // rain
        said[9] = "ran"; // rent
        let mut t: Vec<SpokenWord> = said.iter().enumerate().map(|(i, w)| SpokenWord::new(w, i as f64 * 2.0, i as f64 * 2.0 + 0.5)).collect();
        align_to_pdf(&mut t, &mut p);
        let got: Vec<&str> = t.iter().map(|w| if w.pdf >= 0 { printed[w.pdf as usize] } else { "-" }).collect();
        let want: Vec<&str> = col(0).chain(col(1)).collect();
        assert_eq!(got, want);
    }

    #[test]
    fn norm_and_sound() {
        assert_eq!(norm("Don't,"), "dont");
        assert_eq!(norm("3"), "three");
        assert_eq!(norm("42"), "fortytwo");
        assert_eq!(sound_key("read"), sound_key("red"));
        assert_eq!(sound_key("raise"), sound_key("race"));
        assert_ne!(sound_key("think"), sound_key("sink"));
    }

    #[test]
    fn similar_keeps_minimal_pairs_apart() {
        assert_eq!(similar("think", "think"), 3.0);
        assert!(similar("think", "thing") < 0.0);
        assert!(similar("pronunciation", "pronounciation") > 0.0);
    }

    #[test]
    fn aligns_and_splits_sentences_and_grid_cells() {
        // the teacher explains a bit, then reads two grid cells without a pause, then a sentence
        let mut t = vec![
            SpokenWord::new("Okay,", 0.0, 0.3),
            SpokenWord::new("listen.", 0.3, 0.7),
            SpokenWord::new("church,", 2.0, 2.4),
            SpokenWord::new("search.", 2.5, 2.9),
            SpokenWord::new("The", 4.0, 4.1),
            SpokenWord::new("tourist", 4.1, 4.5),
            SpokenWord::new("took", 5.3, 5.5), // a pause inside the printed sentence
            SpokenWord::new("a", 5.5, 5.6),
            SpokenWord::new("picture.", 5.6, 6.0),
        ];
        let mut p = vec![pw("church", 1), pw("search", 2), pw("The", 3), pw("tourist", 3), pw("took", 3), pw("a", 3), pw("picture.", 3)];
        align_to_pdf(&mut t, &mut p);
        assert_eq!(t[0].pdf, -1);
        assert_eq!(t[2].pdf, 0);
        assert_eq!(t[8].pdf, 6);
        let ph = build_phrases(&t, &p);
        let spans: Vec<(usize, usize, bool)> = ph.iter().map(|x| (x.a, x.b, x.in_pdf)).collect();
        assert_eq!(spans, vec![(0, 2, false), (2, 3, true), (3, 4, true), (4, 9, true)]);
        assert!(ph[0].stop <= ph[1].start);
    }

    #[test]
    fn grid_read_by_column() {
        // PDF rows: "red rat" / "rice rose"; teacher reads down the columns
        let mut t = vec![
            SpokenWord::new("red", 0.0, 0.3),
            SpokenWord::new("rice", 1.0, 1.3),
            SpokenWord::new("rat", 2.0, 2.3),
            SpokenWord::new("rose", 3.0, 3.3),
        ];
        let mut p = vec![pw("red", 1), pw("rat", 2), pw("rice", 3), pw("rose", 4)];
        align_to_pdf(&mut t, &mut p);
        assert!(t.iter().all(|w| w.pdf >= 0), "{t:?}");
        assert_eq!(t[1].pdf, 2);
    }

    #[test]
    fn misheard_words_fill_gaps() {
        // base.en hears "c" for Seat and "be in" for Bean; the PDF says what it was
        let mut t = vec![
            SpokenWord::new("Vowels.", 0.5, 1.2),
            SpokenWord::new("c", 3.3, 3.7),
            SpokenWord::new("sit", 5.0, 5.5),
            SpokenWord::new("be", 7.3, 7.7),
            SpokenWord::new("bit", 9.1, 9.4),
            SpokenWord::new("be", 11.1, 11.4),
            SpokenWord::new("in", 11.42, 12.0),
            SpokenWord::new("bin", 13.0, 13.5),
        ];
        let mut p: Vec<PdfWord> = ["Vowels", "Seat", "Sit", "Beat", "Bit", "Bean", "Bin"].iter().enumerate().map(|(i, x)| pw(x, i as u32)).collect();
        align_to_pdf(&mut t, &mut p);
        let got: Vec<i32> = t.iter().map(|w| w.pdf).collect();
        assert_eq!(got, vec![0, 1, 2, 3, 4, 5, 5, 6]);
        let ph = build_phrases(&t, &p);
        assert_eq!(ph.len(), 7);
        assert_eq!(phrase_text(&ph[5], &t, &p), "Bean");
    }

    #[test]
    fn a_letter_fills_its_cell_past_the_column_headers() {
        // "EEE vs. IH" lesson: the grid's headers (EEE, IH) are printed, not said; "C" is Seat
        let mut t = vec![
            SpokenWord::new("E", 0.59, 0.78),
            SpokenWord::new("vs.", 0.78, 1.32),
            SpokenWord::new("IH", 1.32, 1.97),
            SpokenWord::new("C", 3.33, 3.82),
            SpokenWord::new("sit", 5.02, 5.5),
            SpokenWord::new("bead", 7.29, 8.02),
            SpokenWord::new("bit", 9.14, 9.46),
        ];
        let words = ["EEE", "vs.", "IH", "EEE", "IH", "seat", "sit", "beat", "bit"];
        let chunks = [0, 0, 0, 1, 2, 3, 4, 5, 6];
        let mut p: Vec<PdfWord> = words.iter().zip(chunks).map(|(x, c)| pw(x, c)).collect();
        align_to_pdf(&mut t, &mut p);
        let got: Vec<i32> = t.iter().map(|w| w.pdf).collect();
        assert_eq!(&got[3..], &[5, 6, 7, 8], "C is seat, not a column header");
    }

    #[test]
    fn explanations_are_not_forced_onto_print() {
        // the teacher talks before the list starts; the first printed word is spoken later
        let mut t = vec![
            SpokenWord::new("now", 0.0, 0.3),
            SpokenWord::new("listen", 0.3, 0.8),
            SpokenWord::new("seat", 2.0, 2.4),
        ];
        let mut p = vec![pw("Seat", 1), pw("Sit", 2)];
        align_to_pdf(&mut t, &mut p);
        assert_eq!(t.iter().map(|w| w.pdf).collect::<Vec<_>>(), vec![-1, -1, 0]);
    }

    #[test]
    fn stray_word_moves_next_to_its_sentence() {
        // instructions: "no gap between the R" (chunk 1), then the sentence "The tourist took a picture"
        let mut t: Vec<SpokenWord> = ["the", "tourist", "took", "a", "picture."].iter().enumerate()
            .map(|(i, w)| SpokenWord::new(w, i as f64 * 0.4, i as f64 * 0.4 + 0.3)).collect();
        let mut p: Vec<PdfWord> = ["no", "gap", "between", "the", "R"].iter().map(|w| pw(w, 1))
            .chain(["The", "tourist", "took", "a", "picture."].iter().map(|w| pw(w, 2))).collect();
        align_to_pdf(&mut t, &mut p);
        assert_eq!(t[0].pdf, 5, "{:?}", t.iter().map(|w| w.pdf).collect::<Vec<_>>());
        assert_eq!(p[3].tw, -1);
    }

    #[test]
    fn th_heard_as_s_still_finds_its_word() {
        // a word grid read by column; the recogniser hears "sin" for "thin"
        let mut t = vec![SpokenWord::new("thief", 0.0, 0.4), SpokenWord::new("sin", 2.0, 2.4), SpokenWord::new("threat", 4.0, 4.4)];
        let mut p = vec![pw("theme", 1), pw("thief", 2), pw("thin", 3), pw("threat", 4)];
        align_to_pdf(&mut t, &mut p);
        assert_eq!(t[1].pdf, 2);
    }

    #[test]
    fn contractions_merge() {
        let w = merge_contractions(vec![SpokenWord::new("o", 0.0, 0.1), SpokenWord::new("'clock", 0.1, 0.4)]);
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].n, "oclock");
    }
}
