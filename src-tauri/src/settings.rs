use hark_capture::VideoTarget;
use hark_models::Tier;
use hark_store::Store;
use serde::{Deserialize, Serialize};

pub const SETTINGS_KEY: &str = "settings";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub onboarded: bool,
    pub user_name: String,
    pub language: Option<String>,
    pub mic_device: Option<String>,
    pub loopback_device: Option<String>,
    pub capture_system: bool,
    pub video_enabled: bool,
    pub video_target: VideoTarget,
    pub video_fps: u32,
    pub video_max_height: u32,
    pub tier_override: Option<Tier>,
    pub live_asr_model: Option<String>,
    pub quality_asr_model: Option<String>,
    pub llm_model: Option<String>,
    pub hotkey: String,
    pub detection_enabled: bool,
    pub audio_activity_enabled: bool,
    pub never_apps: Vec<String>,
    pub popup_timeout_secs: u64,
    pub close_to_tray: bool,
    /// "bundled" (llama.cpp in-process) or "openai" (any OpenAI-compatible endpoint).
    pub llm_backend: String,
    pub llm_endpoint: String,
    pub llm_endpoint_model: String,
    pub llm_api_key: Option<String>,
    pub cleanup_enabled: bool,
    pub diarize_enabled: bool,
    pub summary_enabled: bool,
    pub default_template_id: String,
    pub share_port: u16,
    /// .ics URLs (Google "secret address", Outlook "publish calendar") or local files.
    pub calendar_sources: Vec<String>,
    pub calendar_refresh_min: u64,
    /// POST each finished meeting (summary + transcript) here. Off unless enabled.
    pub webhook_url: String,
    pub webhook_enabled: bool,
    /// Ask GitHub for a newer release on startup (only the release manifest is fetched).
    pub auto_update_check: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            onboarded: false,
            user_name: "Me".into(),
            language: Some("en".into()),
            mic_device: None,
            loopback_device: None,
            capture_system: true,
            video_enabled: true,
            video_target: VideoTarget::default(),
            video_fps: 15,
            video_max_height: 1080,
            tier_override: None,
            live_asr_model: None,
            quality_asr_model: None,
            llm_model: None,
            hotkey: "CmdOrCtrl+Shift+R".into(),
            detection_enabled: true,
            audio_activity_enabled: true,
            never_apps: Vec::new(),
            popup_timeout_secs: 30,
            close_to_tray: true,
            llm_backend: "bundled".into(),
            llm_endpoint: "http://localhost:11434/v1".into(),
            llm_endpoint_model: "qwen3:8b".into(),
            llm_api_key: None,
            cleanup_enabled: true,
            diarize_enabled: true,
            summary_enabled: true,
            default_template_id: "general".into(),
            share_port: 47123,
            calendar_sources: Vec::new(),
            calendar_refresh_min: 15,
            webhook_url: String::new(),
            webhook_enabled: false,
            auto_update_check: true,
        }
    }
}

impl Settings {
    pub fn load(store: &Store) -> Settings {
        store.get_setting::<Settings>(SETTINGS_KEY).ok().flatten().unwrap_or_default()
    }

    pub fn save(&self, store: &Store) -> hark_store::Result<()> {
        store.set_setting(SETTINGS_KEY, self)
    }

    /// Effective tier: explicit override or hardware-selected.
    pub fn tier(&self, hw: &hark_models::Hardware) -> Tier {
        self.tier_override.unwrap_or_else(|| hark_models::select_tier(hw))
    }

    /// Model ids to use, falling back to the tier defaults.
    pub fn model_ids(&self, catalog: &hark_models::Catalog, tier: Tier) -> (String, String, String) {
        let t = catalog.tier_models(tier);
        (
            self.live_asr_model.clone().unwrap_or_else(|| t.asr_live.clone()),
            self.quality_asr_model.clone().unwrap_or_else(|| t.asr_quality.clone()),
            self.llm_model.clone().unwrap_or_else(|| t.llm.clone()),
        )
    }
}
