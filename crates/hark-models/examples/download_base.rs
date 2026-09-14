//! Downloads whisper-base into the Hark models dir (or HARK_MODELS_DIR) and prints the hardware probe.
#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() {
    let hw = hark_models::probe();
    println!("hardware: {hw:#?}\ntier: {:?}", hark_models::select_tier(&hw));
    let dir = std::env::var("HARK_MODELS_DIR").map(std::path::PathBuf::from).unwrap_or_else(|_| std::env::temp_dir().join("hark-models"));
    let cat = hark_models::Catalog::builtin();
    let spec = cat.get("whisper-base").unwrap();
    let last = std::sync::atomic::AtomicU64::new(0);
    let p = hark_models::download(spec, &dir, |d, t| {
        let pct = d * 100 / t.max(1);
        if pct / 10 != last.swap(pct, std::sync::atomic::Ordering::Relaxed) / 10 { println!("{pct}%"); }
    }, || false).await.unwrap();
    println!("saved {p:?}");
}
