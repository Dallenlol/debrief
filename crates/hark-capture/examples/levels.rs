//! Print mic / system loopback levels over a few seconds.
fn main() {
    let secs: u64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(4);
    let (mic, sys) = hark_capture::probe::sample_levels(None, None, std::time::Duration::from_secs(secs));
    println!("mic {mic:.1} dBFS, system {sys:.1} dBFS");
}
