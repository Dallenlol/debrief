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
    /// Captions + running notes of the recording in progress (for the Meeting page).
    pub live_feed: Mutex<Option<crate::live_feed::LiveFeed>>,
    /// Pipelines (post-processing, re-runs, chat) currently using the engines;
    /// the idle unloader leaves them alone while this is > 0.
    pub busy: std::sync::atomic::AtomicUsize,
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
            live_feed: Mutex::new(None),
            busy: std::sync::atomic::AtomicUsize::new(0),
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
                let w = Arc::new(w.tagged(model_id));
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

    /// The live-caption engine: `preferred` if downloaded, else the smallest downloaded speech
    /// model. Live passes run every second or two, so a small model beats a big one here (the
    /// transcript you keep comes from the full pass after the call).
    pub fn whisper_live(&self, preferred: &str) -> Option<Arc<WhisperEngine>> {
        if let Some(w) = self.whisper(preferred) {
            return Some(w);
        }
        let dir = self.models_dir();
        let mut candidates: Vec<&hark_models::ModelSpec> = self
            .catalog
            .all()
            .iter()
            .filter(|m| m.kind == hark_models::ModelKind::Asr && hark_models::is_present(&dir, m))
            .collect();
        candidates.sort_by_key(|m| m.size_bytes);
        candidates.into_iter().find_map(|m| self.whisper(&m.id))
    }

    /// Paths needed to run the diarization sidecar, if the models are downloaded.
    pub fn diarize_models(&self) -> Option<(PathBuf, PathBuf)> {
        Some((self.model_file("pyannote-seg-3")?, self.model_file("titanet-small")?))
    }

    /// The configured chat model: bundled llama.cpp (if the GGUF is downloaded), a cloud
    /// model (with the local one as fallback) or an OpenAI-compatible endpoint. `None` when unavailable.
    pub fn llm(&self) -> Option<Arc<dyn LlmBackend>> {
        let s = self.settings.read().clone();
        let cloud = (s.llm_backend == "cloud").then(|| cloud_backend(&s)).flatten();
        let key = match (&cloud, s.llm_backend.as_str()) {
            (Some(_), _) => format!("cloud|{}|{}|{}|{:?}", s.cloud_provider, s.cloud_model, secret_tag(s.cloud_api_key.as_deref()), s.cloud_context_tokens),
            (None, "openai") => format!("openai|{}|{}|{}", s.llm_endpoint, s.llm_endpoint_model, secret_tag(s.llm_api_key.as_deref())),
            _ => {
                let tier = s.tier(&self.hardware);
                let (_, _, configured) = s.model_ids(&self.catalog, tier);
                format!("bundled|{}", self.llm_or_any(&configured)?)
            }
        };
        {
            let mut e = self.engines.lock();
            let cached = e.llm.as_ref().filter(|(k, _)| *k == key).map(|(_, b)| b.clone());
            if let Some(b) = cached {
                e.last_used = Some(Instant::now());
                return Some(b);
            }
        }
        let backend: Arc<dyn LlmBackend> = if let Some(primary) = cloud {
            let local = self.local_llm_spec();
            Arc::new(hark_llm::WithFallback::new(
                primary,
                Box::new(move || {
                    let (path, gpu_layers, n_ctx) = local.clone()?;
                    hark_llm::LlamaEngine::load(&path, gpu_layers, n_ctx).map(|e| Arc::new(e) as Arc<dyn LlmBackend>).map_err(|e| log::error!("fallback llm: {e}")).ok()
                }),
            ))
        } else if s.llm_backend == "openai" {
            Arc::new(hark_llm::OpenAiCompat::new(&s.llm_endpoint, s.llm_api_key.as_deref(), &s.llm_endpoint_model))
        } else {
            let (path, gpu_layers, n_ctx) = self.local_llm_spec()?;
            match hark_llm::LlamaEngine::load(&path, gpu_layers, n_ctx) {
                Ok(e) => Arc::new(e),
                Err(err) => {
                    log::error!("load llm {}: {err}", path.display());
                    return None;
                }
            }
        };
        let mut e = self.engines.lock();
        e.llm = Some((key, backend.clone()));
        e.last_used = Some(Instant::now());
        Some(backend)
    }

    /// GGUF path, GPU layers and context cap for the local chat model, if one is downloaded.
    fn local_llm_spec(&self) -> Option<(PathBuf, u32, u32)> {
        let s = self.settings.read().clone();
        let (_, _, configured) = s.model_ids(&self.catalog, s.tier(&self.hardware));
        let path = self.model_file(&self.llm_or_any(&configured)?)?;
        let gpu = crate::settings::GPU_BUILD && self.hardware.gpu.is_some();
        // A context is only sized this large when a prompt needs it (llama.rs sizes per request).
        Some((path, if gpu { 999 } else { 0 }, if gpu { 32_768 } else { 12_288 }))
    }

    /// `preferred` if its GGUF is downloaded, else the largest downloaded chat model no
    /// bigger than it (or the smallest one, if all are bigger). Mirrors `whisper_or_any`.
    fn llm_or_any(&self, preferred: &str) -> Option<String> {
        if self.model_file(preferred).is_some() {
            return Some(preferred.to_string());
        }
        let dir = self.models_dir();
        let want = self.catalog.get(preferred).map(|m| m.size_bytes).unwrap_or(u64::MAX);
        let mut present: Vec<&hark_models::ModelSpec> = self
            .catalog
            .all()
            .iter()
            .filter(|m| m.kind == hark_models::ModelKind::Llm && hark_models::is_present(&dir, m))
            .collect();
        present.sort_by_key(|m| std::cmp::Reverse(m.size_bytes));
        let pick = present.iter().find(|m| m.size_bytes <= want).or(present.last()).map(|m| m.id.clone());
        let missing = match self.catalog.get(preferred) {
            Some(spec) => format!("llm {preferred} not downloaded (expected {} bytes at {})", spec.size_bytes, hark_models::model_path(&dir, spec).display()),
            None => format!("llm {preferred} is not in the model catalog"),
        };
        let msg = match &pick {
            Some(id) => format!("{missing}; using downloaded {id}"),
            None => format!("{missing}; no chat model downloaded in {}", dir.display()),
        };
        // Every AI request lands here (live notes poll it), so log only when the outcome changes.
        static LOGGED: Mutex<String> = Mutex::new(String::new());
        let mut logged = LOGGED.lock();
        if *logged != msg {
            log::warn!("{msg}");
            *logged = msg;
        }
        pick
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

    /// RAII marker: engines stay loaded while any `BusyGuard` is alive.
    pub fn busy_guard(&self) -> BusyGuard<'_> {
        self.busy.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.refresh_priority();
        BusyGuard(self)
    }

    /// Crunching a finished meeting (transcription, AI) runs at below-normal priority so the
    /// rest of the computer stays responsive; while recording, everything runs at normal
    /// priority so audio capture is never starved.
    pub fn refresh_priority(&self) {
        // Decide and apply under one lock: two callers racing (a job finishing as Record is
        // pressed) must not apply a stale decision last. Every caller re-reads the state, so
        // whoever runs last applies the current answer.
        static APPLY: parking_lot::Mutex<()> = parking_lot::Mutex::new(());
        let _g = APPLY.lock();
        let background = self.busy.load(std::sync::atomic::Ordering::SeqCst) > 0 && self.recording.lock().is_none();
        set_background_priority(background);
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

pub struct BusyGuard<'a>(&'a AppState);

impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        self.0.busy.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        self.0.engines.lock().last_used = Some(Instant::now());
        self.0.refresh_priority();
    }
}

#[cfg(windows)]
fn set_background_priority(background: bool) {
    use windows::Win32::System::Threading::{GetCurrentProcess, SetPriorityClass, BELOW_NORMAL_PRIORITY_CLASS, NORMAL_PRIORITY_CLASS};
    let class = if background { BELOW_NORMAL_PRIORITY_CLASS } else { NORMAL_PRIORITY_CLASS };
    // SAFETY: GetCurrentProcess returns a pseudo-handle that needs no closing.
    let _ = unsafe { SetPriorityClass(GetCurrentProcess(), class) };
}

#[cfg(not(windows))]
fn set_background_priority(_background: bool) {}

/// Cache-key tag for a secret: changes when the key changes, without keeping it in the key.
fn secret_tag(k: Option<&str>) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    k.hash(&mut h);
    h.finish()
}

pub const OPENAI_BASE: &str = "https://api.openai.com/v1";
pub const OPENROUTER_BASE: &str = "https://openrouter.ai/api/v1";

/// The cloud model the settings describe, or `None` when the key or model is missing.
pub fn cloud_backend(s: &Settings) -> Option<Arc<dyn LlmBackend>> {
    let key = s.cloud_api_key.as_deref().map(str::trim).filter(|k| !k.is_empty())?;
    let ctx = s.cloud_context_tokens.map(|c| c as usize);
    match s.cloud_provider.as_str() {
        "anthropic" => Some(Arc::new(hark_llm::AnthropicApi::new(key, &s.cloud_model, ctx))),
        p @ ("openai" | "openrouter") => {
            let model = s.cloud_model.trim();
            if model.is_empty() {
                return None;
            }
            let base = if p == "openai" { OPENAI_BASE } else { OPENROUTER_BASE };
            Some(Arc::new(hark_llm::OpenAiCompat::new(base, Some(key), model).with_context(ctx.unwrap_or(128_000))))
        }
        _ => None,
    }
}
