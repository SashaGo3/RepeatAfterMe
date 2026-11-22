use cpal::traits::{DeviceTrait, HostTrait};
fn main() {
    let h = cpal::default_host();
    let n = |d: Option<cpal::Device>| d.and_then(|d| d.description().ok()).map(|x| x.name().to_string());
    println!("default output: {:?}", n(h.default_output_device()));
    println!("default input:  {:?}", n(h.default_input_device()));
    println!("outputs: {:?}", ram_core::device::output_names());
}
