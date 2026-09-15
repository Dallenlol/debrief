use crate::recorder::ActiveRecording;
use crate::settings::Settings;
use hark_asr::WhisperEngine;
use hark_llm::LlmBackend;
use hark_models::{Catalog, Hardware};
use hark_store::Store;
use parking_lot::{Mutex, RwLock};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Instant;

/// Loaded AI engines, keyed by model id. Loaded lazily, dropped on `unload_all`.
pub const EMBED_MODEL_ID: &str = "bge-small-en-v1.5-q8";

#[derive(Default)]
pub struct Engines {
    pub whisper: HashMap<String, Arc<WhisperEngine>>,
    /// (backend key, backend) - key changes when settings change.
    pub llm: Option<(String, Arc<dyn LlmBackend>)>,
    pub embed: Option<Arc<hark_llm::EmbedEngine>>,
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
    pub diarize_bin: Option<PathBuf>,
    /// Model downloads in flight; value = cancel flag.
    pub downloads: Mutex<HashMap<String, Arc<AtomicBool>>>,
    /// Streaming chat replies in flight, by chat id.
    pub chat_cancels: Mutex<HashMap<String, Arc<AtomicBool>>>,
    /// Per-meeting counter bumped on every speaker rename; lets the delayed
    /// summary refresh run once after a burst of renames.
    pub rename_seq: Mutex<HashMap<String, u64>>,
    /// LAN share server, running only while at least one share is enabled.
    pub share_server: Mutex<Option<hark_share::ShareServerHandle>>,
    /// Cached calendar events from the configured .ics sources.
    pub calendar: RwLock<Vec<hark_calendar::CalEvent>>,
    pub calendar_errors: RwLock<Vec<String>>,
}

impl AppState {
    pub fn new(store: Store) -> Self {
        let settings = Settings::load(&store);
        let builtins: Vec<hark_store::Template> = hark_llm::summary::builtin_templates()
            .into_iter()
            .map(|t| hark_store::Template { id: t.id.to_string(), name: t.name, description: t.description, body: t.body, builtin: true })
            .collect();
        if let Err(e) = store.seed_templates(&builtins) {
            log::error!("seed templates: {e}");
        }
        AppState {
            store: Arc::new(store),
            settings: RwLock::new(settings),
            hardware: hark_models::probe(),
            catalog: Catalog::builtin(),
            recording: Mutex::new(None),
            engines: Mutex::new(Engines::default()),
            detection_paused: AtomicBool::new(false),
            snooze_until: Mutex::new(None),
            ffmpeg: find_sidecar("ffmpeg"),
            diarize_bin: find_sidecar("hark-diarize"),
            downloads: Mutex::new(HashMap::new()),
            chat_cancels: Mutex::new(HashMap::new()),
            rename_seq: Mutex::new(HashMap::new()),
            share_server: Mutex::new(None),
            calendar: RwLock::new(Vec::new()),
            calendar_errors: RwLock::new(Vec::new()),
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
        log::info!("loading whisper {model_id} from {}", path.display());
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

    /// Paths needed to run the diarization sidecar, if the models are downloaded.
    pub fn diarize_models(&self) -> Option<(PathBuf, PathBuf)> {
        Some((self.model_file("pyannote-seg-3")?, self.model_file("titanet-small")?))
    }

    /// The configured chat model: bundled llama.cpp (if the GGUF is downloaded)
    /// or an OpenAI-compatible endpoint. `None` when unavailable.
    pub fn llm(&self) -> Option<Arc<dyn LlmBackend>> {
        let s = self.settings.read().clone();
        let tier = s.tier(&self.hardware);
        let (_, _, llm_id) = s.model_ids(&self.catalog, tier);
        let key = if s.llm_backend == "openai" {
            format!("openai|{}|{}|{}", s.llm_endpoint, s.llm_endpoint_model, s.llm_api_key.is_some())
        } else {
            format!("bundled|{llm_id}")
        };
        {
            let mut e = self.engines.lock();
            let cached = e.llm.as_ref().filter(|(k, _)| *k == key).map(|(_, b)| b.clone());
            if let Some(b) = cached {
                e.last_used = Some(Instant::now());
                return Some(b);
            }
        }
        let backend: Arc<dyn LlmBackend> = if s.llm_backend == "openai" {
            Arc::new(hark_llm::OpenAiCompat::new(&s.llm_endpoint, s.llm_api_key.as_deref(), &s.llm_endpoint_model))
        } else {
            let path = self.model_file(&llm_id)?;
            let gpu_layers = if self.hardware.gpu.is_some() { 999 } else { 0 };
            match hark_llm::LlamaEngine::load(&path, gpu_layers, 8192) {
                Ok(e) => Arc::new(e),
                Err(err) => {
                    log::error!("load llm {llm_id}: {err}");
                    return None;
                }
            }
        };
        let mut e = self.engines.lock();
        e.llm = Some((key, backend.clone()));
        e.last_used = Some(Instant::now());
        Some(backend)
    }

    /// The embedding model for semantic search, if downloaded.
    pub fn embed(&self) -> Option<Arc<hark_llm::EmbedEngine>> {
        {
            let mut e = self.engines.lock();
            if let Some(m) = e.embed.clone() {
                e.last_used = Some(Instant::now());
                return Some(m);
            }
        }
        let path = self.model_file(EMBED_MODEL_ID)?;
        let engine = match hark_llm::EmbedEngine::load(&path) {
            Ok(e) => Arc::new(e),
            Err(err) => {
                log::error!("load embed model: {err}");
                return None;
            }
        };
        let mut e = self.engines.lock();
        e.embed = Some(engine.clone());
        e.last_used = Some(Instant::now());
        Some(engine)
    }

    pub fn unload_engines(&self) {
        let mut e = self.engines.lock();
        e.whisper.clear();
        e.llm = None;
        e.embed = None;
        e.last_used = None;
    }
}

/// Locate a sidecar binary: next to the executable (bundled), then the dev
/// `src-tauri/binaries/` folder, then the cargo target dir, then PATH.
fn find_sidecar(name: &str) -> Option<PathBuf> {
    let exe_name = if cfg!(windows) { format!("{name}.exe") } else { name.to_string() };
    let exe_name = exe_name.as_str();
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
        .join(format!("{name}-{triple}{}", if cfg!(windows) { ".exe" } else { "" }));
    if dev.exists() {
        return Some(dev);
    }
    which(exe_name)
}

fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|d| d.join(name)).find(|p| p.is_file())
}
