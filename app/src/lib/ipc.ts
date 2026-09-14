// Typed bridge to the Rust side. Every command and event lives here so the
// rest of the UI never touches Tauri APIs directly.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type MeetingStatus = "recording" | "processing" | "ready" | "failed";

export interface Meeting {
  id: string;
  title: string;
  app: string | null;
  started_at: string;
  ended_at: string | null;
  duration_ms: number;
  has_video: boolean;
  status: MeetingStatus;
  folder_id: string | null;
  created_at: string;
}

export interface Segment {
  id: number;
  meeting_id: string;
  start_ms: number;
  end_ms: number;
  speaker: string | null;
  text: string;
  clean_text: string | null;
}

export interface MeetingSpeaker {
  meeting_id: string;
  label: string;
  speaker_id: string | null;
  suggested_id: string | null;
  suggested_name: string | null;
  suggested_score: number | null;
}

export interface Speaker {
  id: string;
  name: string;
}

export interface MeetingDetail {
  meeting: Meeting;
  segments: Segment[];
  media: { audio: string; video: string | null };
  highlights: number[];
  speakers: MeetingSpeaker[];
}

export interface SearchHit {
  meeting_id: string;
  meeting_title: string;
  segment_id: number;
  start_ms: number;
  snippet: string;
}

export type VideoTarget = { kind: "monitor"; index: number } | { kind: "window"; title: string };
export type Tier = "cpu_low" | "cpu_high" | "gpu";

export interface Settings {
  onboarded: boolean;
  user_name: string;
  language: string | null;
  mic_device: string | null;
  loopback_device: string | null;
  capture_system: boolean;
  video_enabled: boolean;
  video_target: VideoTarget;
  video_fps: number;
  video_max_height: number;
  tier_override: Tier | null;
  live_asr_model: string | null;
  quality_asr_model: string | null;
  llm_model: string | null;
  hotkey: string;
  detection_enabled: boolean;
  audio_activity_enabled: boolean;
  never_apps: string[];
  popup_timeout_secs: number;
  close_to_tray: boolean;
  llm_backend: "bundled" | "openai";
  llm_endpoint: string;
  llm_endpoint_model: string;
  llm_api_key: string | null;
  cleanup_enabled: boolean;
  diarize_enabled: boolean;
}

export interface AudioDevice {
  id: string;
  name: string;
  is_default: boolean;
}

export interface Hardware {
  cpu_cores: number;
  cpu_name: string;
  ram_gb: number;
  gpu: { name: string; vram_gb: number; backend: "cuda" | "metal" } | null;
  os: string;
  arch: string;
}

export interface ModelSpec {
  id: string;
  kind: "asr" | "llm" | "embedding" | "diarize";
  name: string;
  file: string;
  url: string;
  sha256: string;
  size_bytes: number;
  note: string;
}

export interface ModelRow {
  spec: ModelSpec;
  present: boolean;
  downloading: boolean;
  roles: ("live" | "quality" | "llm")[];
}

export interface RecordingState {
  state: "idle" | "recording" | "paused";
  meeting_id: string | null;
  elapsed_ms: number;
}

export interface StartOptions {
  title?: string;
  app?: string;
  video?: boolean;
  target?: VideoTarget;
}

export const cmd = {
  listMeetings: () => invoke<Meeting[]>("list_meetings"),
  getMeeting: (id: string) => invoke<MeetingDetail>("get_meeting", { id }),
  renameMeeting: (id: string, title: string) => invoke<Meeting>("rename_meeting", { id, title }),
  deleteMeeting: (id: string) => invoke<void>("delete_meeting", { id }),
  search: (query: string, limit = 50) => invoke<SearchHit[]>("search", { query, limit }),
  retranscribe: (id: string) => invoke<void>("retranscribe", { id }),

  startRecording: (opts?: StartOptions) => invoke<Meeting>("start_recording", { opts }),
  stopRecording: () => invoke<Meeting>("stop_recording"),
  pauseRecording: () => invoke<void>("pause_recording"),
  resumeRecording: () => invoke<void>("resume_recording"),
  markHighlight: () => invoke<number>("mark_highlight"),
  recordingStatus: () => invoke<RecordingState>("recording_status"),
  dismissDetection: (appId: string, mode: "now" | "never" | "snooze") =>
    invoke<void>("dismiss_detection", { appId, mode }),
  openMain: () => invoke<void>("open_main"),

  getSettings: () => invoke<Settings>("get_settings"),
  setSettings: (s: Settings) => invoke<Settings>("set_settings", { new: s }),
  dataInfo: () => invoke<{ data_dir: string; recordings_bytes: number; models_bytes: number }>("data_info"),

  probeHardware: () => invoke<{ hardware: Hardware; tier: Tier; effective_tier: Tier }>("probe_hardware"),
  listModels: () => invoke<ModelRow[]>("list_models"),
  downloadModel: (id: string) => invoke<void>("download_model", { id }),
  cancelDownload: (id: string) => invoke<void>("cancel_download", { id }),
  removeModel: (id: string) => invoke<void>("remove_model", { id }),

  meetingSpeakers: (id: string) => invoke<MeetingSpeaker[]>("meeting_speakers", { id }),
  renameSpeaker: (id: string, label: string, name: string) => invoke<Speaker>("rename_speaker", { id, label, name }),
  acceptSpeakerSuggestion: (id: string, label: string) => invoke<Speaker>("accept_speaker_suggestion", { id, label }),
  listKnownSpeakers: () => invoke<Speaker[]>("list_known_speakers"),
  deleteKnownSpeaker: (id: string) => invoke<void>("delete_known_speaker", { id }),
  rerunCleanup: (id: string) => invoke<void>("rerun_cleanup", { id }),
  testLlmEndpoint: (url: string, apiKey: string | null, model: string) =>
    invoke<string[]>("test_llm_endpoint", { url, apiKey, model }),

  listAudioDevices: () => invoke<{ inputs: AudioDevice[]; outputs: AudioDevice[] }>("list_audio_devices"),
  sampleLevels: (mic: string | null, loopback: string | null) =>
    invoke<[number, number]>("sample_levels", { mic, loopback }),
};

export interface Events {
  detection: { app: string; label: string; title: string; confidence: number };
  levels: { mic_db: number; sys_db: number };
  caption: { start_ms: number; end_ms: number; text: string; is_final: boolean };
  recording_state: RecordingState;
  model_progress: { id: string; done: number; total: number; status: "downloading" | "done" | "failed"; error: string | null };
  processing: { meeting_id: string; stage: string; progress: number; error: string | null };
  notice: { level: "info" | "warning" | "error"; message: string };
}

export function on<K extends keyof Events>(name: K, handler: (payload: Events[K]) => void): Promise<UnlistenFn> {
  return listen<Events[K]>(name, (e) => handler(e.payload));
}

/** Subscribe inside a React effect; returns a cleanup that unsubscribes. */
export function subscribe<K extends keyof Events>(name: K, handler: (payload: Events[K]) => void): () => void {
  let un: UnlistenFn | null = null;
  let cancelled = false;
  on(name, handler).then((f) => {
    if (cancelled) f();
    else un = f;
  });
  return () => {
    cancelled = true;
    un?.();
  };
}
