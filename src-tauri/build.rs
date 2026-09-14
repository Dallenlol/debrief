fn main() {
    // Expose the target triple so the dev build can find `binaries/ffmpeg-<triple>`.
    let triple = std::env::var("TARGET").unwrap_or_default();
    println!("cargo:rustc-env=HARK_TARGET_TRIPLE={triple}");
    tauri_build::build()
}
