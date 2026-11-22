use ram_core::{session::replay_turn, settings::TurnEnd};
fn tone(rate: u32, secs: f64, f: f32, a: f32) -> Vec<f32> { (0..(rate as f64 * secs) as usize).map(|i| a * (i as f32 * f * std::f32::consts::TAU / rate as f32).sin()).collect() }
fn main() {
    let mut x = tone(16000, 4.0, 60.0, 0.05);
    for (i, v) in x.iter_mut().enumerate() { let t = i as f64 / 16000.0; if (0.3..0.7).contains(&t) { *v += 0.2 * (i as f32 * 700.0 * std::f32::consts::TAU / 16000.0).sin(); } }
    println!("{}", replay_turn(&x, 16000, 0.4, 0.0, 1, TurnEnd::Smart));
}
