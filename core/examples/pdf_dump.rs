use ram_core::pdf;
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    pdf::init(None)?;
    let t = pdf::read_words(a[1].as_ref())?;
    let mut last = 0; let mut line = String::new();
    for w in &t.words { if w.chunk != last { if !line.is_empty() { println!("{line}"); } line = format!("[{} p{}] ", w.chunk, w.page); last = w.chunk; } line.push_str(&w.text); line.push(' '); }
    println!("{line}"); Ok(())
}
