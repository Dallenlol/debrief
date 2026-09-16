//! Choose the transcription model for a recording from what this machine can
//! actually do: every run records the model's measured real-time factor, and
//! the next pick is the best model that will finish within the recording's own
//! length. A GPU makes every model fast, so the configured one is used as is.

use crate::settings::GPU_BUILD;
use crate::state::AppState;
use hark_store::Store;

/// Seconds of processing per second of audio, before this machine has measured anything
/// (8-thread AVX2 desktop CPU; laptops are slower and learn it after one run).
fn default_factor(model_id: &str) -> f32 {
    match model_id {
        "whisper-large-v3-turbo" => 3.0,
        "whisper-medium" => 2.3,
        "whisper-small" => 0.8,
        "whisper-base" => 0.3,
        _ => 1.5,
    }
}

/// Bigger is better; used to order candidates.
fn quality_rank(model_id: &str) -> u8 {
    match model_id {
        "whisper-large-v3-turbo" => 4,
        "whisper-medium" => 3,
        "whisper-small" => 2,
        "whisper-base" => 1,
        _ => 0,
    }
}

fn speed_key(model_id: &str) -> String {
    format!("asr_speed:{model_id}")
}

/// Measured real-time factor for `model_id` on this machine, if any.
pub fn measured_factor(store: &Store, model_id: &str) -> Option<f32> {
    store.get_setting::<f32>(&speed_key(model_id)).ok().flatten()
}

/// Record how long `model_id` took for `audio_ms` of audio (exponential average).
pub fn record_run(store: &Store, model_id: &str, audio_ms: i64, took_secs: f32) {
    if audio_ms < 5_000 || took_secs <= 0.0 {
        return;
    }
    let factor = took_secs / (audio_ms as f32 / 1000.0);
    let blended = match measured_factor(store, model_id) {
        Some(prev) => prev * 0.5 + factor * 0.5,
        None => factor,
    };
    let _ = store.set_setting(&speed_key(model_id), &blended);
    log::info!("asr speed: {model_id} {factor:.2}x realtime (avg {blended:.2}x)");
}

/// The model to transcribe `audio_ms` with. `preferred` is the tier's quality
/// model; the answer is `preferred` unless it is downloaded-but-too-slow for a
/// CPU-only machine, in which case the best downloaded model that finishes
/// within the recording's length wins (or the fastest one, if none does).
pub fn pick(state: &AppState, preferred: &str, audio_ms: i64) -> String {
    let store = &state.store;
    if GPU_BUILD && state.hardware.gpu.is_some() {
        return preferred.to_string();
    }
    let dir = state.models_dir();
    let mut present: Vec<String> = state
        .catalog
        .all()
        .iter()
        .filter(|m| m.kind == hark_models::ModelKind::Asr && hark_models::is_present(&dir, m))
        .map(|m| m.id.clone())
        .collect();
    if !present.iter().any(|id| id == preferred) {
        return preferred.to_string(); // not downloaded: the caller falls back to whatever exists
    }
    let factor = |id: &str| measured_factor(store, id).unwrap_or_else(|| default_factor(id));
    let budget_secs = (audio_ms as f32 / 1000.0).max(90.0);
    let audio_secs = audio_ms as f32 / 1000.0;
    present.sort_by_key(|id| std::cmp::Reverse(quality_rank(id)));
    let fits = |id: &str| factor(id) * audio_secs <= budget_secs;
    if fits(preferred) {
        return preferred.to_string();
    }
    let pick = present
        .iter()
        .filter(|id| quality_rank(id) < quality_rank(preferred))
        .find(|id| fits(id))
        .cloned()
        .or_else(|| present.iter().min_by(|a, b| factor(a).total_cmp(&factor(b))).cloned())
        .unwrap_or_else(|| preferred.to_string());
    if pick != preferred {
        log::info!(
            "asr pick: {pick} instead of {preferred} for {:.0} s of audio ({:.1}x vs {:.1}x realtime on this CPU)",
            audio_secs,
            factor(&pick),
            factor(preferred)
        );
    }
    pick
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_rank_speed_and_quality_consistently() {
        assert!(default_factor("whisper-base") < default_factor("whisper-small"));
        assert!(default_factor("whisper-small") < default_factor("whisper-medium"));
        assert!(quality_rank("whisper-medium") > quality_rank("whisper-small"));
    }

    #[test]
    fn measurements_blend_and_persist() {
        let st = Store::open_in_memory().unwrap();
        record_run(&st, "whisper-small", 60_000, 30.0); // 0.5x
        assert!((measured_factor(&st, "whisper-small").unwrap() - 0.5).abs() < 1e-3);
        record_run(&st, "whisper-small", 60_000, 90.0); // 1.5x -> avg 1.0
        assert!((measured_factor(&st, "whisper-small").unwrap() - 1.0).abs() < 1e-3);
        record_run(&st, "whisper-small", 1_000, 90.0); // too short to count
        assert!((measured_factor(&st, "whisper-small").unwrap() - 1.0).abs() < 1e-3);
    }
}
