//! cargo run --example align_check -- <lesson.pdf> <transcript.json>
use ram_core::{align::*, pdf};
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    pdf::init(None)?;
    let t0 = std::time::Instant::now();
    let mut text = pdf::read_words(a[1].as_ref())?;
    let t1 = t0.elapsed();
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&a[2])?)?;
    let words: Vec<SpokenWord> = v["words"].as_array().unwrap().iter()
        .map(|w| SpokenWord::new(w["w"].as_str().unwrap(), w["s"].as_f64().unwrap(), w["e"].as_f64().unwrap())).collect();
    let mut words = merge_contractions(words);
    align_to_pdf(&mut words, &mut text.words);
    let ph = build_phrases(&words, &text.words);
    // MAP=170-200: which printed word each spoken word in that time range went to
    if let Some((a, b)) = std::env::var("MAP").ok().and_then(|m| m.split_once('-').map(|(a, b)| (a.parse::<f64>().unwrap_or(0.0), b.parse::<f64>().unwrap_or(1e9)))) {
        for (i, w) in words.iter().enumerate().filter(|(_, w)| w.s >= a && w.s <= b) {
            let printed = if w.pdf >= 0 { format!("[{}] {}", w.pdf, text.words[w.pdf as usize].text) } else { "-".into() };
            println!("{i:4} {:7.2} {:>14} → {printed}", w.s, w.w);
        }
    }
    let matched = words.iter().filter(|w| w.pdf >= 0).count();
    let printed = text.words.iter().filter(|w| w.tw >= 0).count();
    println!("pdf: {} pages, {} words, {} chunks, read in {:?}; total {:?}", text.pages.len(), text.words.len(),
        text.words.last().map(|w| w.chunk).unwrap_or(0), t1, t0.elapsed());
    println!("spoken {} words, {} matched; {}/{} pdf words spoken; {} phrases ({} to repeat)",
        words.len(), matched, printed, text.words.len(), ph.len(), ph.iter().filter(|p| p.in_pdf).count());
    let missed: Vec<&str> = text.words.iter().filter(|w| w.tw < 0).map(|w| w.text.as_str()).collect();
    println!("printed but not matched: {missed:?}");
    for p in ph.iter().take(std::env::var("N").ok().and_then(|s| s.parse().ok()).unwrap_or(25)) {
        let s = vec![phrase_text(p, &words, &text.words)];
        println!("{} {:6.2}-{:6.2} {}", if p.in_pdf { "●" } else { "·" }, p.start, p.end, s.join(" "));
    }
    Ok(())
}
