//! How well every lesson's words line up with its PDF, as text files you can compare before and after a
//! change to the alignment:
//!
//!   cargo run --release --example align_report -- <data dir> <out dir>
//!   diff -r before/ after/
//!
//! Uses the transcripts the app already made (<data dir>/transcripts), so it takes a second. With
//! TRANSCRIBE_TO=<dir>, lessons not transcribed yet are transcribed the way the app does it (same model,
//! the PDF's words as a hint) and kept in <dir>, never in the data folder. Per lesson:
//! a summary (what's printed but never heard, what's heard but never placed, matches nothing backs
//! up), then every spoken word and the printed word it went to.
use ram_core::{align::*, audio, library::{content_id, Library}, pdf, transcribe};
use std::fmt::Write;
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let (data, out) = (PathBuf::from(&a[1]), PathBuf::from(&a[2]));
    pdf::init(std::env::var_os("RAM_PDFIUM_DIR").map(PathBuf::from).as_deref())?;
    std::fs::create_dir_all(&out)?;
    let lib = Library::open(data.join("library.json"));
    let mut totals = (0, 0, 0, 0);
    for e in &lib.entries {
        let name = e.audio.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        let Ok(id) = content_id(&e.audio) else { println!("{name}: audio missing"); continue };
        let mut text = pdf::read_words(&e.pdf)?;
        let extra = std::env::var_os("TRANSCRIBE_TO").map(PathBuf::from);
        let file = format!("{id}.json");
        let found = std::iter::once(data.join("transcripts").join(&file))
            .chain(extra.iter().map(|d| d.join(&file)))
            .find_map(|p| std::fs::read_to_string(p).ok());
        let json = match (found, &extra) {
            (Some(j), _) => j,
            (None, Some(dir)) => {
                println!("{name}: transcribing (once)…");
                let model = std::env::var_os("RAM_MODEL").map(PathBuf::from).unwrap_or_else(|| "vendor/models/ggml-small.en-q5_1.bin".into());
                transcribe::set_model_path(model);
                let clip = audio::decode_file(&e.audio)?;
                let prompt = transcribe::prompt_from(&text.words);
                let opts = transcribe::Options { prompt: Some(&prompt), ..Default::default() };
                let words = transcribe::transcribe(&clip, &opts, |_| {}, std::sync::Arc::new(|| false))?;
                let j = serde_json::to_string(&serde_json::json!({ "words": words }))?;
                std::fs::create_dir_all(dir)?;
                std::fs::write(dir.join(&file), &j)?;
                j
            }
            (None, None) => {
                println!("{name}: not transcribed yet (open it in the app once, or set TRANSCRIBE_TO)");
                continue;
            }
        };
        let v: serde_json::Value = serde_json::from_str(&json)?;
        let mut words: Vec<SpokenWord> = serde_json::from_value(v["words"].clone())?;
        align_to_pdf(&mut words, &mut text.words);
        let phrases = build_phrases(&words, &text.words);

        let unheard: Vec<String> = text.words.iter().enumerate().filter(|(_, w)| w.tw < 0).map(|(j, w)| format!("[{j}] {}", w.text)).collect();
        let unplaced: Vec<String> = words.iter().enumerate().filter(|(_, w)| w.pdf < 0).map(|(i, w)| format!("{i} {:.2}s {}", w.s, w.w)).collect();
        let doubtful: Vec<String> = words
            .iter()
            .enumerate()
            .filter(|(i, w)| w.pdf >= 0 && !supported(&words, *i, w.pdf as i64))
            .map(|(i, w)| format!("{i} {:.2}s {} → [{}] {}", w.s, w.w, w.pdf, text.words[w.pdf as usize].text))
            .collect();
        let mut s = String::new();
        writeln!(s, "{name}\n{} spoken words, {} placed; {} printed words, {} heard; {} phrases",
            words.len(), words.len() - unplaced.len(), text.words.len(), text.words.len() - unheard.len(), phrases.len())?;
        writeln!(s, "\nprinted, never heard ({}):\n  {}", unheard.len(), unheard.join("\n  "))?;
        writeln!(s, "\nheard, not placed ({}):\n  {}", unplaced.len(), unplaced.join("\n  "))?;
        writeln!(s, "\nplaced where nothing around backs it up ({}):\n  {}", doubtful.len(), doubtful.join("\n  "))?;
        writeln!(s, "\nevery word:")?;
        for (i, w) in words.iter().enumerate() {
            let printed = if w.pdf >= 0 { format!("[{}] {}", w.pdf, text.words[w.pdf as usize].text) } else { "-".into() };
            writeln!(s, "{i:4} {:7.2} {:>16} → {printed}", w.s, w.w)?;
        }
        std::fs::write(out.join(format!("{name}.txt")), s)?;
        println!("{name}: {} unheard printed, {} unplaced spoken, {} doubtful", unheard.len(), unplaced.len(), doubtful.len());
        totals = (totals.0 + 1, totals.1 + unheard.len(), totals.2 + unplaced.len(), totals.3 + doubtful.len());
    }
    println!("{} lessons: {} unheard printed, {} unplaced spoken, {} doubtful", totals.0, totals.1, totals.2, totals.3);
    Ok(())
}
