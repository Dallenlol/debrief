//! Windows only: capture one process tree's audio for a few seconds and print the level.
//! cargo run -p hark-capture --example proc_loopback -- <pid> [seconds]
fn main() {
    #[cfg(windows)]
    {
        use std::sync::{Arc, Mutex};
        let pid: u32 = std::env::args().nth(1).expect("pid").parse().expect("pid");
        let secs: u64 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(6);
        let buf = Arc::new(Mutex::new(Vec::<f32>::new()));
        let b = buf.clone();
        let s = hark_capture::stream::InputStream::process_loopback(
            pid,
            48_000,
            Arc::new(move |pcm| b.lock().unwrap().extend_from_slice(pcm)),
            Arc::new(|e| eprintln!("error: {e}")),
        )
        .expect("open");
        std::thread::sleep(std::time::Duration::from_secs(secs));
        drop(s);
        let v = buf.lock().unwrap();
        println!("samples: {} ({:.1} s), rms {:.1} dBFS", v.len(), v.len() as f32 / 48_000.0, hark_capture::dsp::rms_db(&v));
    }
}
