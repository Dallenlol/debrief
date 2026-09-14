fn main() {
    // Expose the target triple so the dev build can find `binaries/ffmpeg-<triple>`.
    let triple = std::env::var("TARGET").unwrap_or_default();
    println!("cargo:rustc-env=HARK_TARGET_TRIPLE={triple}");
    // Build variant (cpu | cuda | metal) picks which update manifest this binary follows.
    println!("cargo:rerun-if-env-changed=HARK_VARIANT");
    let variant = std::env::var("HARK_VARIANT").ok().filter(|v| !v.is_empty()).unwrap_or_else(|| {
        if triple.contains("apple") { "metal".into() } else { "cpu".into() }
    });
    println!("cargo:rustc-env=HARK_VARIANT={variant}");
    tauri_build::build()
}
