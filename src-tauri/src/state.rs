use crate::recorder::ActiveRecording;
use crate::settings::Settings;
use hark_asr::WhisperEngine;
use hark_models::{Catalog, Hardware};
use hark_store::Store;
use parking_lot::{Mutex, RwLock};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Instant;

/// Loaded AI engines, keyed by model id. Loaded lazily, dropped on `unload_all`.
#[derive(Default)]
pub struct Engines {
    pub whisper: HashMap<String, Arc<WhisperEngine>>,
    pub last_used: Option<Instant>,
}

pub struct AppState {
    pub store: Arc<Store>,
    pub settings: RwLock<Settings>,
    pub hardware: Hardware,
    pub catalog: Catalog,
    pub recording: Mutex<Option<ActiveRecording>>,
    pub engines: Mutex<Engines>,
    /// Detector is paused (user toggle or while recording).
    pub detection_paused: AtomicBool,
    pub snooze_until: Mutex<Option<Instant>>,
    pub ffmpeg: Option<PathBuf>,
    /// Model downloads in flight; value = cancel flag.
    pub downloads: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

impl AppState {
    pub fn new(store: Store) -> Self {
        let settings = Settings::load(&store);
        AppState {
            store: Arc::new(store),
            settings: RwLock::new(settings),
            hardware: hark_models::probe(),
            catalog: Catalog::builtin(),
            recording: Mutex::new(None),
            engines: Mutex::new(Engines::default()),
            detection_paused: AtomicBool::new(false),
            snooze_until: Mutex::new(None),
            ffmpeg: find_ffmpeg(),
            downloads: Mutex::new(HashMap::new()),
        }
    }

    pub fn models_dir(&self) -> PathBuf {
        self.store.models_dir()
    }

    /// Path of a catalog model if downloaded.
    pub fn model_file(&self, id: &str) -> Option<PathBuf> {
        let spec = self.catalog.get(id)?;
        let dir = self.models_dir();
        hark_models::is_present(&dir, spec).then(|| hark_models::model_path(&dir, spec))
    }

    /// Get or load a whisper engine for `model_id`. `None` if the model isn't downloaded.
    pub fn whisper(&self, model_id: &str) -> Option<Arc<WhisperEngine>> {
        {
            let mut e = self.engines.lock();
            if let Some(w) = e.whisper.get(model_id).cloned() {
                e.last_used = Some(Instant::now());
                return Some(w);
            }
        }
        let path = self.model_file(model_id)?;
        let use_gpu = self.hardware.gpu.is_some();
        match WhisperEngine::load(&path, use_gpu) {
            Ok(w) => {
                let w = Arc::new(w);
                let mut e = self.engines.lock();
                e.whisper.insert(model_id.to_string(), w.clone());
                e.last_used = Some(Instant::now());
                Some(w)
            }
            Err(err) => {
                log::error!("load whisper {model_id}: {err}");
                None
            }
        }
    }

    /// Preferred model if downloaded, else any downloaded ASR model (largest first).
    pub fn whisper_or_any(&self, preferred: &[&str]) -> Option<Arc<WhisperEngine>> {
        for id in preferred {
            if let Some(w) = self.whisper(id) {
                return Some(w);
            }
        }
        let dir = self.models_dir();
        let mut candidates: Vec<&hark_models::ModelSpec> = self
            .catalog
            .all()
            .iter()
            .filter(|m| m.kind == hark_models::ModelKind::Asr && hark_models::is_present(&dir, m))
            .collect();
        candidates.sort_by_key(|m| std::cmp::Reverse(m.size_bytes));
        candidates.into_iter().find_map(|m| self.whisper(&m.id))
    }

    pub fn unload_engines(&self) {
        let mut e = self.engines.lock();
        e.whisper.clear();
        e.last_used = None;
    }
}

/// Locate the ffmpeg sidecar: next to the executable (bundled), then the dev
/// `src-tauri/binaries/` folder, then PATH.
fn find_ffmpeg() -> Option<PathBuf> {
    let exe_name = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join(exe_name);
            if p.exists() {
                return Some(p);
            }
        }
    }
    let triple = env!("HARK_TARGET_TRIPLE");
    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("binaries")
        .join(format!("ffmpeg-{triple}{}", if cfg!(windows) { ".exe" } else { "" }));
    if dev.exists() {
        return Some(dev);
    }
    which(exe_name)
}

fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|d| d.join(name)).find(|p| p.is_file())
}
