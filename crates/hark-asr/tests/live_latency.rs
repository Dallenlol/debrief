//! Real-time live-caption check: feeds speech (the JFK clip, or HARK_TEST_SPEECH) into
//! `LiveTranscriber` at the pace a microphone would, and reports how long after each
//! line was spoken its final caption arrived. Fails if captions fall seriously behind.
//! Requires HARK_TEST_MODEL=<path to ggml-*.bin>. Run:
//! cargo test -p hark-asr --release --test live_latency -- --ignored --nocapture
use hark_asr::live::HZ;
use hark_asr::{LiveTranscriber, WhisperEngine};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn read_wav_16k_mono(p: &std::path::Path) -> Vec<f32> {
    let r = hound::WavReader::open(p).unwrap();
    let ch = r.spec().channels as usize;
    let samples: Vec<f32> = r.into_samples::<i16>().map(|s| s.unwrap() as f32 / i16::MAX as f32).collect();
    samples.chunks(ch).map(|f| f.iter().sum::<f32>() / ch as f32).collect()
}

#[test]
#[ignore = "needs a whisper model; set HARK_TEST_MODEL"]
fn live_captions_keep_up_in_real_time() {
    let model = std::env::var("HARK_TEST_MODEL").map(PathBuf::from).expect("HARK_TEST_MODEL");
    let engine = Arc::new(WhisperEngine::load(&model, true).unwrap());
    let clip = read_wav_16k_mono(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jfk.wav"));

    // Single-pass cost on a typical 6 s utterance.
    let six = &clip[..6 * HZ];
    let _ = engine.transcribe(six, 0, Some("en"), false); // warm-up
    let t = Instant::now();
    for _ in 0..5 {
        engine.transcribe(six, 0, Some("en"), false).unwrap();
    }
    let per_pass = t.elapsed().as_secs_f32() / 5.0;
    println!("gpu={} one pass over 6 s of audio: {:.0} ms", engine.is_gpu(), per_pass * 1000.0);

    // HARK_TEST_SPEECH=<16 kHz wav>: a longer, varied recording (a meeting excerpt). The JFK clip
    // alone cannot simply be repeated: a caption that only repeats the previous text is dropped
    // on purpose (whisper echoes its prompt on unclear audio).
    let mut audio = match std::env::var("HARK_TEST_SPEECH") {
        Ok(p) => read_wav_16k_mono(std::path::Path::new(&p)),
        Err(_) => clip.clone(),
    };
    audio.extend(std::iter::repeat(0.0).take(2 * HZ));
    let (pcm_tx, pcm_rx) = crossbeam_channel::unbounded::<Vec<f32>>();
    let (cap_tx, cap_rx) = crossbeam_channel::unbounded();
    let live = LiveTranscriber::start(engine.clone(), pcm_rx, cap_tx, Some("en".into()));
    let started = Instant::now();
    let chunk = HZ / 10; // 100 ms, like the capture callback
    let feeder = std::thread::spawn(move || {
        for (i, c) in audio.chunks(chunk).enumerate() {
            let due = Duration::from_millis(100 * (i as u64 + 1));
            if let Some(wait) = due.checked_sub(started.elapsed()) {
                std::thread::sleep(wait);
            }
            pcm_tx.send(c.to_vec()).unwrap();
        }
    });

    let mut lags = Vec::new();
    let mut partials = 0;
    for c in cap_rx.iter() {
        let now_ms = started.elapsed().as_millis() as i64;
        if c.is_final {
            let lag = now_ms - c.end_ms;
            println!("  +{:>5} ms after speech  [{:>6}-{:>6}] {}", lag, c.start_ms, c.end_ms, c.text);
            lags.push(lag);
        } else {
            partials += 1;
        }
    } // ends when the feeder drops its sender and the transcriber flushes

    feeder.join().unwrap();
    live.join();
    lags.sort();
    let worst = *lags.last().expect("no final captions");
    let median = lags[lags.len() / 2];
    println!("final captions: {} (partials {}), median lag {} ms, worst {} ms", lags.len(), partials, median, worst);
    assert!(median < 4_000, "captions lag too far behind speech: median {median} ms");
}
