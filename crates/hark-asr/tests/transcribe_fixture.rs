//! Requires HARK_TEST_MODEL=<path to ggml-*.bin>. Run: cargo test -p hark-asr -- --ignored
use std::path::PathBuf;

fn read_wav_16k_mono(p: &std::path::Path) -> Vec<f32> {
    let r = hound::WavReader::open(p).unwrap();
    let spec = r.spec();
    assert_eq!(spec.sample_rate, 16000);
    let ch = spec.channels as usize;
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => r.into_samples::<i16>().map(|s| s.unwrap() as f32 / i16::MAX as f32).collect(),
        hound::SampleFormat::Float => r.into_samples::<f32>().map(|s| s.unwrap()).collect(),
    };
    samples.chunks(ch).map(|f| f.iter().sum::<f32>() / ch as f32).collect()
}

#[test]
#[ignore = "needs a whisper model; set HARK_TEST_MODEL"]
fn transcribes_jfk_fixture() {
    let model = std::env::var("HARK_TEST_MODEL").map(PathBuf::from).expect("HARK_TEST_MODEL");
    let engine = hark_asr::WhisperEngine::load(&model, true).unwrap();
    let pcm = read_wav_16k_mono(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jfk.wav"));
    let caps = engine.transcribe(&pcm, 0, Some("en"), true).unwrap();
    let text: String = caps.iter().map(|c| c.text.to_lowercase()).collect::<Vec<_>>().join(" ");
    println!("gpu={} captions={caps:#?}", engine.gpu);
    assert!(text.contains("country"), "got: {text}");
    assert!(caps[0].start_ms >= 0 && caps.last().unwrap().end_ms > 5000);
}
