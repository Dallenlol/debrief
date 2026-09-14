use std::path::PathBuf;
fn main() {
    let models = std::env::var("HARK_MODELS_DIR").map(PathBuf::from).unwrap();
    let wav = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../hark-asr/tests/fixtures/jfk.wav");
    let r = hound::WavReader::open(&wav).unwrap();
    let pcm: Vec<f32> = r.into_samples::<i16>().map(|s| s.unwrap() as f32 / 32768.0).collect();
    let mut eng = hark_diarize::DiarizeEngine::load(&models.join("pyannote-segmentation-3-0.onnx"), &models.join("nemo_en_titanet_small.onnx")).unwrap();
    let t0 = std::time::Instant::now();
    let turns = eng.diarize(&pcm, |p| { let _ = p; }).unwrap();
    println!("turns ({:?}): {turns:?}", t0.elapsed());
    let emb = eng.embed(&pcm[..16000 * 3]).unwrap();
    println!("embedding dim {} (declared {})", emb.len(), eng.embedding_dim);
    let emb2 = eng.embed(&pcm[16000 * 4..16000 * 8]).unwrap();
    println!("self-similarity {:.3}", hark_diarize::cosine(&emb, &emb2));
}
