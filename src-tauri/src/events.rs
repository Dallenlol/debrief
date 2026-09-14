//! Event names and payloads emitted to every window.

use serde::Serialize;

pub const DETECTION: &str = "detection";
pub const LEVELS: &str = "levels";
pub const CAPTION: &str = "caption";
pub const RECORDING_STATE: &str = "recording_state";
pub const MODEL_PROGRESS: &str = "model_progress";
pub const PROCESSING: &str = "processing";
pub const NOTICE: &str = "notice";

#[derive(Serialize, Clone)]
pub struct DetectionPayload {
    pub app: String,
    pub label: String,
    pub title: String,
    pub confidence: f32,
}

#[derive(Serialize, Clone)]
pub struct LevelsPayload {
    pub mic_db: f32,
    pub sys_db: f32,
}

#[derive(Serialize, Clone)]
pub struct RecordingStatePayload {
    pub state: &'static str,
    pub meeting_id: Option<String>,
    pub elapsed_ms: u64,
}

#[derive(Serialize, Clone)]
pub struct ModelProgressPayload {
    pub id: String,
    pub done: u64,
    pub total: u64,
    pub status: &'static str,
    pub error: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct ProcessingPayload {
    pub meeting_id: String,
    pub stage: &'static str,
    pub progress: f32,
    pub error: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct NoticePayload {
    pub level: &'static str,
    pub message: String,
}
