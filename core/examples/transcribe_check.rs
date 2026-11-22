//! cargo run --release --example transcribe_check -- <audio> <pdf> [reference.json]
//! Transcribes with each setting and compares with the old large-v3-turbo transcript.
use ram_core::{align::*, audio, pdf, transcribe::*};
use std::time::Instant;

fn compare(name: &str, got: &[SpokenWord], reference: &[SpokenWord]) {
    // match words in order by normalised text (LCS), then compare times
    let (n, m) = (got.len(), reference.len());
    let mut l = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() { for j in (0..m).rev() {
        l[i][j] = if got[i].n == reference[j].n { l[i + 1][j + 1] + 1 } else { l[i + 1][j].max(l[i][j + 1]) };
    }}
    let (mut i, mut j, mut ds, mut de, mut k, mut far) = (0, 0, 0.0, 0.0, 0, 0);
    while i < n && j < m {
        if got[i].n == reference[j].n {
            let (a, b) = ((got[i].s - reference[j].s).abs(), (got[i].e - reference[j].e).abs());
            ds += a; de += b; k += 1; if a > 0.3 || b > 0.3 { far += 1; }
            i += 1; j += 1;
        } else if l[i + 1][j] >= l[i][j + 1] { i += 1 } else { j += 1 }
    }
    println!("  {name}: {} words, {k}/{m} same as reference; mean |Δstart| {:.3}s |Δend| {:.3}s; {far} off by >0.3s",
        n, ds / k.max(1) as f64, de / k.max(1) as f64);
}

fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    pdf::init(None)?;
    let t = Instant::now();
    let clip = audio::decode_file(a[1].as_ref())?;
    println!("decoded {:.1}s of audio at {} Hz in {:?}", clip.duration(), clip.rate, t.elapsed());
    let text = pdf::read_words(a[2].as_ref())?;
    let reference: Vec<SpokenWord> = a.get(3).map(|p| {
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
        v["words"].as_array().unwrap().iter().map(|w| SpokenWord::new(w["w"].as_str().unwrap(), w["s"].as_f64().unwrap(), w["e"].as_f64().unwrap())).collect()
    }).unwrap_or_default();
    let prompt = prompt_from(&text.words);
    let modes: Vec<(&str, Option<&str>, Timing)> = match std::env::var("MODES").as_deref() {
        Ok("best") => vec![("dtw+prompt", Some(prompt.as_str()), Timing::Dtw)],
        _ => vec![("tokens", None, Timing::Tokens), ("dtw", None, Timing::Dtw), ("dtw+prompt", Some(prompt.as_str()), Timing::Dtw)],
    };
    for (name, pr, timing) in modes {
        let t = Instant::now();
        let mut words = transcribe(&clip, &Options { prompt: pr, timing, ..Default::default() }, |_| {}, std::sync::Arc::new(|| false))?;
        let took = t.elapsed();
        let mut pw = text.words.clone();
        align_to_pdf(&mut words, &mut pw);
        let ph = build_phrases(&words, &pw);
        println!("{name}: {:?} ({:.0}x real time); {}/{} words in PDF, {}/{} PDF words spoken, {} phrases",
            took, clip.duration() / took.as_secs_f64(), words.iter().filter(|w| w.pdf >= 0).count(), words.len(),
            pw.iter().filter(|w| w.tw >= 0).count(), pw.len(), ph.len());
        if !reference.is_empty() {
            compare(name, &words, &reference);
            // the phrases the old pipeline made, from the same PDF
            let mut rw = merge_contractions(reference.clone());
            let mut rp = text.words.clone();
            align_to_pdf(&mut rw, &mut rp);
            let rph: Vec<(f64, String)> = build_phrases(&rw, &rp).iter().filter(|p| p.in_pdf).map(|p| (p.start, phrase_text(p, &rw, &rp).to_lowercase())).collect();
            let ours: Vec<(f64, String)> = ph.iter().filter(|p| p.in_pdf).map(|p| (p.start, phrase_text(p, &words, &pw).to_lowercase())).collect();
            let strip = |s: &str| s.chars().filter(|c| c.is_alphanumeric()).collect::<String>();
            let same = rph.iter().filter(|(t, x)| ours.iter().any(|(u, y)| (t - u).abs() < 0.2 && strip(x) == strip(y))).count();
            println!("  phrases to repeat: {} (reference {}), {same} identical (same words, start within 0.2s)", ours.len(), rph.len());
            if std::env::var("DIFF").is_ok() {
                for (t, x) in &rph { if !ours.iter().any(|(u, y)| (t - u).abs() < 0.2 && strip(x) == strip(y)) {
                    let near: Vec<String> = ours.iter().filter(|(u, _)| (t - u).abs() < 1.5).map(|(u, y)| format!("{u:.2} {y}")).collect();
                    println!("    ref {t:.2} {x:30} | ours: {}", near.join(" / "));
                }}
            }
        }
        if std::env::var("SHOW").is_ok() {
            for p in ph.iter().take(12) {
                let s: Vec<String> = words[p.a..p.b].iter().map(|w| format!("{}[{:.2}-{:.2}]", label(w, &pw), w.s, w.e)).collect();
                println!("    {}", s.join(" "));
            }
        }
    }
    Ok(())
}
