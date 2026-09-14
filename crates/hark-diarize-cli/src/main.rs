//! `hark-diarize --seg <onnx> --emb <onnx> --wav <wav> [--max-embed-secs N]`
//! Reads a WAV (any rate/channels), diarizes, computes one voice embedding per
//! cluster, prints `DiarizeOutput` JSON on stdout. Progress lines `progress <0..1>`
//! go to stderr. Exit code 0 even on engine errors (error is in the JSON).

mod engine;

use anyhow::{anyhow, Context, Result};
use hark_diarize::{centroid, DiarizeOutput, DiarizeRequest, Turn};
use std::path::Path;

fn parse_args() -> Result<DiarizeRequest> {
    let mut seg = None;
    let mut emb = None;
    let mut wav = None;
    let mut max_embed_secs = 60usize;
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--seg" => seg = it.next(),
            "--emb" => emb = it.next(),
            "--wav" => wav = it.next(),
            "--max-embed-secs" => max_embed_secs = it.next().and_then(|v| v.parse().ok()).unwrap_or(60),
            other => return Err(anyhow!("unknown argument {other}")),
        }
    }
    Ok(DiarizeRequest {
        seg_model: seg.ok_or_else(|| anyhow!("--seg required"))?,
        emb_model: emb.ok_or_else(|| anyhow!("--emb required"))?,
        wav: wav.ok_or_else(|| anyhow!("--wav required"))?,
        max_embed_secs,
    })
}

/// Any WAV -> 16 kHz mono f32 (simple windowed-sinc-free decimation is fine for embeddings? no: use linear
/// interpolation only when rates differ; the app always hands us 48 kHz mono mix.wav).
fn read_wav_16k(path: &Path) -> Result<Vec<f32>> {
    let reader = hound::WavReader::open(path).with_context(|| format!("open {}", path.display()))?;
    let spec = reader.spec();
    let ch = spec.channels as usize;
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => {
            let max = ((1u64 << (spec.bits_per_sample - 1)) - 1) as f32;
            reader.into_samples::<i32>().map(|s| s.map(|v| v as f32 / max)).collect::<std::result::Result<_, _>>()?
        }
        hound::SampleFormat::Float => reader.into_samples::<f32>().collect::<std::result::Result<_, _>>()?,
    };
    let mono: Vec<f32> = samples.chunks(ch).map(|f| f.iter().sum::<f32>() / ch as f32).collect();
    if spec.sample_rate == 16_000 {
        return Ok(mono);
    }
    // Linear interpolation resample (adequate for segmentation/embedding features).
    let ratio = spec.sample_rate as f64 / 16_000.0;
    let n = (mono.len() as f64 / ratio) as usize;
    Ok((0..n)
        .map(|i| {
            let pos = i as f64 * ratio;
            let j = pos.floor() as usize;
            let frac = (pos - j as f64) as f32;
            let a = mono.get(j).copied().unwrap_or(0.0);
            let b = mono.get(j + 1).copied().unwrap_or(a);
            a + (b - a) * frac
        })
        .collect())
}

fn cluster_audio(pcm: &[f32], turns: &[Turn], cluster: i32, max_secs: usize) -> Vec<f32> {
    let cap = max_secs * 16_000;
    let mut out = Vec::new();
    for t in turns.iter().filter(|t| t.cluster == cluster) {
        let a = ((t.start_ms.max(0) as usize) * 16).min(pcm.len());
        let b = ((t.end_ms.max(0) as usize) * 16).min(pcm.len());
        if b > a {
            out.extend_from_slice(&pcm[a..b]);
        }
        if out.len() >= cap {
            out.truncate(cap);
            break;
        }
    }
    out
}

fn run(req: &DiarizeRequest) -> Result<DiarizeOutput> {
    let pcm = read_wav_16k(Path::new(&req.wav))?;
    let mut eng = engine::DiarizeEngine::load(Path::new(&req.seg_model), Path::new(&req.emb_model))?;
    let turns = eng.diarize(&pcm, |p| eprintln!("progress {p:.3}"))?;
    let mut out = DiarizeOutput { turns: turns.clone(), embedding_dim: eng.embedding_dim, ..Default::default() };
    let mut clusters: Vec<i32> = turns.iter().map(|t| t.cluster).collect();
    clusters.sort_unstable();
    clusters.dedup();
    for c in clusters {
        let audio = cluster_audio(&pcm, &turns, c, req.max_embed_secs);
        if audio.len() < 16_000 {
            continue;
        }
        match eng.embed(&audio) {
            Ok(e) => {
                out.embeddings.insert(c, centroid(&[e]));
            }
            Err(e) => eprintln!("embed cluster {c}: {e}"),
        }
    }
    Ok(out)
}

fn main() {
    let req = match parse_args() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    let out = match run(&req) {
        Ok(o) => o,
        Err(e) => DiarizeOutput { error: Some(e.to_string()), ..Default::default() },
    };
    println!("{}", serde_json::to_string(&out).expect("json"));
}
