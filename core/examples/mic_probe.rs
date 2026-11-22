fn main() {
    println!("inputs: {:?}", ram_core::device::input_names());
    let t = std::time::Instant::now();
    let r = ram_core::device::probe_mic(None, std::time::Duration::from_millis(800));
    println!("{r:?} in {:?}", t.elapsed());
}
