//! Replays recorded turns (debug/NNN-heard.wav + turns.jsonl) through the turn detector.
//! cargo run --release --example replay_turns -- "<data dir>/debug"
use ram_core::{session::replay_turn, settings::TurnEnd};
fn read_wav(p: &std::path::Path) -> (Vec<f32>, u32) {
    let b = std::fs::read(p).unwrap();
    let rate = u32::from_le_bytes(b[24..28].try_into().unwrap());
    (b[44..].chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0).collect(), rate)
}
fn main() {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).unwrap());
    for line in std::fs::read_to_string(dir.join("turns.jsonl")).unwrap().lines() {
        let t: serde_json::Value = serde_json::from_str(line).unwrap();
        let n = t["turn"].as_u64().unwrap();
        let (x, rate) = read_wav(&dir.join(format!("{n:03}-heard.wav")));
        println!("turn {n} \"{}\" (was: {}, voice {}–{})", t["text"].as_str().unwrap_or(""), t["ended"], t["voiceStart"], t["voiceEnd"]);
        println!("   now: {}", replay_turn(&x, rate, t["teacherSecs"].as_f64().unwrap(), t["teacherMaxGap"].as_f64().unwrap(),
            t["words"].as_u64().unwrap_or_else(|| t["text"].as_str().unwrap_or("").split_whitespace().count() as u64) as usize, TurnEnd::Smart));
    }
}
