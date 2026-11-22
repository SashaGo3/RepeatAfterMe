//! Headless run of the engine on real devices: add a course folder, open a lesson, play in listen-only mode.
//! cargo run --release --example engine_check -- <data dir> <course dir> <lesson name>
use ram_core::{engine::*, session::{Cmd, Event}};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let (tx, rx) = mpsc::channel::<Out>();
    let tx = std::sync::Mutex::new(tx);
    let t0 = Instant::now();
    let sink: Sink = Arc::new(move |o| { let _ = tx.lock().unwrap().send(o); });
    let model = std::env::var("RAM_MODEL")?;
    let e = Engine::start(Paths { data: a[1].clone().into(), model: model.into(), pdfium_dir: None }, sink)?;
    let (n, rest) = e.add(&[a[2].clone().into()])?;
    println!("added {n} lessons; unpaired audio {:?}, pdf {}", rest.audio, rest.pdf.len());
    let lessons = e.lessons();
    let l = lessons.iter().find(|l| l.name == a[3]).expect("lesson");
    println!("opening {} ({}) in {:?}", l.name, l.id, t0.elapsed());
    let t1 = Instant::now();
    e.open(&l.id);
    let mut words_seen = 0; let mut last_stage = String::new(); let mut spoken = vec![];
    let mut phrases = vec![];
    let deadline = Instant::now() + Duration::from_secs(120);
    let mut started: Option<Instant> = None;
    while Instant::now() < deadline {
        let Ok(o) = rx.recv_timeout(Duration::from_millis(100)) else {
            if let Some(s) = started { if s.elapsed() > Duration::from_secs(14) { break; } }
            continue;
        };
        match o {
            Out::Loading { stage, progress, .. } => { if stage != last_stage { println!("  {stage} … {:?}", t1.elapsed()); last_stage = stage; } let _ = progress; }
            Out::Lesson { lesson } => {
                println!("  lesson ready in {:?}: {} pages, {} printed words, {} phrases", t1.elapsed(), lesson.pages.len(), lesson.words.len(), lesson.phrases.len());
                let png = e.page_png(&lesson.id, 0, 1200)?;
                println!("  page 1 → {} KB PNG", png.len() / 1024);
                std::fs::write(std::path::Path::new(&a[1]).join("page1.png"), &png)?;
                spoken = lesson.spoken.clone(); phrases = lesson.phrases.clone();
                std::fs::write(std::path::Path::new(&a[1]).join("lesson.json"), serde_json::to_string(&lesson)?)?;
                e.command(Cmd::ToggleListenOnly);
                e.command(Cmd::Goto(1));
                started = Some(Instant::now());
            }
            Out::Session { event: Event::Word { word, t } } if word >= 0 => {
                words_seen += 1;
                let wall = started.map(|s| s.elapsed().as_secs_f64()).unwrap_or(0.0);
                if words_seen <= 12 { println!("    word {word:3} (pdf {:3}) at lesson t={t:6.2}s, wall {wall:5.2}s", spoken[word as usize]); }
            }
            Out::Session { event: Event::State(s) } => println!("  state {:?} phrase {}/{} {:?}", s.mode, s.phrase, s.phrases, s.note),
            Out::Error { message } => println!("  ERROR {message}"),
            _ => {}
        }
    }
    e.command(Cmd::Stop);
    println!("{words_seen} word highlights; phrase 1: {:?}", phrases.get(1).map(|p| &p.text));
    std::thread::sleep(Duration::from_millis(300));
    Ok(())
}
