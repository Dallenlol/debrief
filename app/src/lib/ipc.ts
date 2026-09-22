// Typed bridge to the Rust side. Every command and event lives here so the
// rest of the UI never touches Tauri APIs directly.
import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { listen as tauriListen, type UnlistenFn } from "@tauri-apps/api/event";
import { isMock, mockInvoke } from "./mock";

// Outside Tauri (plain `pnpm dev` in a browser) fall back to sample data so the UI can be developed and screenshotted.
const invoke: typeof tauriInvoke = isMock ? (mockInvoke as typeof tauriInvoke) : tauriInvoke;
const listen: typeof tauriListen = isMock ? (async () => () => {}) as typeof tauriListen : tauriListen;

export type MeetingStatus = "recording" | "processing" | "ready" | "failed";

export interface CalEvent {
  uid: string;
  title: string;
  start: string;
  end: string;
  attendees: string[];
  url: string | null;
  location: string | null;
}

export interface ParticipantsPayload {
  meeting_id: string;
  participants: string[];
}

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
  error: string | null;
  title_auto: boolean;
  participants: string[];
  calendar_uid: string | null;
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

export interface SpeakerStat {
  label: string;
  segments: number;
  /** Total speaking time in milliseconds. */
  ms: number;
  first_ms: number;
}

export interface RenameOutcome {
  speaker: Speaker | null;
  /** The label every affected segment now carries. */
  name: string;
  /** Set when this rename merged two labels into one person. */
  merged_from: string | null;
  moved_segments: number;
}

export interface MeetingDetail {
  meeting: Meeting;
  segments: Segment[];
  media: { audio: string; video: string | null };
  highlights: number[];
  speakers: MeetingSpeaker[];
  speaker_stats: SpeakerStat[];
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

export interface VideoSource {
  /** Owning process (window sources on Windows); lets the recorder capture only that app's audio. */
  pid: number | null;
  target: VideoTarget;
  label: string;
  is_meeting: boolean;
}

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
  summary_enabled: boolean;
  default_template_id: string;
  share_port: number;
  calendar_sources: string[];
  calendar_refresh_min: number;
  webhook_url: string;
  webhook_enabled: boolean;
  auto_update_check: boolean;
}

export interface Template {
  id: string;
  name: string;
  description: string;
  body: string;
  builtin: boolean;
}

export interface Summary {
  meeting_id: string;
  template_id: string;
  markdown: string;
  structured: {
    summary: string;
    action_items: string[];
    decisions: string[];
    open_questions: string[];
    key_moments: { ms: number | null; text: string }[];
    sections: Record<string, string>;
  } | null;
  model: string;
  created_at: string;
}

export interface Chat {
  id: string;
  scope_kind: "meeting" | "all";
  scope_id: string | null;
  title: string;
  created_at: string;
}

export interface ChatMessage {
  id: string;
  chat_id: string;
  role: "user" | "assistant" | string;
  content: string;
  citations: unknown;
  created_at: string;
}

export interface Folder {
  id: string;
  name: string;
  parent_id: string | null;
  default_template_id: string | null;
  meeting_count: number;
}

export interface Tag {
  id: string;
  name: string;
  meeting_count: number;
}

export interface MeetingFilter {
  /** null = all; [null] = unfiled; [id] = folder subtree */
  folder: null | [string | null];
  tag_id: string | null;
}

export interface Share {
  token: string;
  meeting_id: string;
  meeting_title: string;
  kind: "meeting" | "clip";
  start_ms: number | null;
  end_ms: number | null;
  enabled: boolean;
  created_at: string;
}

export interface ShareInfo {
  share: Share;
  url: string;
}

export interface ImportReport {
  imported: number;
  skipped_existing: number;
  errors: string[];
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
  roles: ("live" | "quality" | "llm" | "speakers" | "search")[];
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
  /** Capture system audio only from this process tree. */
  audio_pid?: number;
}

export const cmd = {
  listMeetings: () => invoke<Meeting[]>("list_meetings"),
  getMeeting: (id: string) => invoke<MeetingDetail>("get_meeting", { id }),
  renameMeeting: (id: string, title: string) => invoke<Meeting>("rename_meeting", { id, title }),
  deleteMeeting: (id: string) => invoke<void>("delete_meeting", { id }),
  search: (query: string, limit = 50) => invoke<SearchHit[]>("search", { query, limit }),
  testWebhook: (url: string) => invoke<number>("test_webhook", { url }),
  listVideoSources: (meetingApp?: string | null) => invoke<VideoSource[]>("list_video_sources", { meetingApp: meetingApp ?? null }),
  listUpcoming: (hours?: number) => invoke<CalEvent[]>("list_upcoming", { hours }),
  refreshCalendar: () => invoke<string[]>("refresh_calendar"),
  retranscribe: (id: string, model?: string | null) => invoke<void>("retranscribe", { id, model: model ?? null }),
  rediarize: (id: string) => invoke<void>("rediarize", { id }),
  reembed: (id: string) => invoke<void>("reembed", { id }),
  importRecording: (path: string, title?: string) => invoke<Meeting>("import_recording", { path, title: title ?? null }),
  saveTextFile: (path: string, text: string) => invoke<void>("save_text_file", { path, text }),

  startRecording: (opts?: StartOptions) => invoke<Meeting>("start_recording", { opts }),
  stopRecording: () => invoke<Meeting>("stop_recording"),
  pauseRecording: () => invoke<void>("pause_recording"),
  resumeRecording: () => invoke<void>("resume_recording"),
  markHighlight: () => invoke<number>("mark_highlight"),
  recordingStatus: () => invoke<RecordingState>("recording_status"),
  liveSnapshot: () => invoke<LiveSnapshot | null>("live_snapshot"),
  openRecordPicker: () => invoke<void>("open_record_picker"),
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
  renameSpeaker: (id: string, label: string, name: string) =>
    invoke<RenameOutcome>("rename_speaker", { id, label, name }),
  acceptSpeakerSuggestion: (id: string, label: string) => invoke<Speaker>("accept_speaker_suggestion", { id, label }),
  listKnownSpeakers: () => invoke<Speaker[]>("list_known_speakers"),
  deleteKnownSpeaker: (id: string) => invoke<void>("delete_known_speaker", { id }),
  rerunCleanup: (id: string) => invoke<void>("rerun_cleanup", { id }),
  testLlmEndpoint: (url: string, apiKey: string | null, model: string) =>
    invoke<string[]>("test_llm_endpoint", { url, apiKey, model }),

  listTemplates: () => invoke<Template[]>("list_templates"),
  saveTemplate: (template: Template) => invoke<Template>("save_template", { template }),
  deleteTemplate: (id: string) => invoke<void>("delete_template", { id }),
  getSummary: (id: string) => invoke<Summary | null>("get_summary", { id }),
  generateSummary: (id: string, templateId?: string) => invoke<void>("generate_summary", { id, templateId }),
  draftFollowup: (id: string) => invoke<string>("draft_followup", { id }),
  listChats: (scopeKind: "meeting" | "all", scopeId: string | null) => invoke<Chat[]>("list_chats", { scopeKind, scopeId }),
  createChat: (scopeKind: "meeting" | "all", scopeId: string | null, title?: string) => invoke<Chat>("create_chat", { scopeKind, scopeId, title }),
  deleteChat: (id: string) => invoke<void>("delete_chat", { id }),
  chatMessages: (chatId: string) => invoke<ChatMessage[]>("chat_messages", { chatId }),
  sendChat: (chatId: string, text: string) => invoke<ChatMessage>("send_chat", { chatId, text }),
  cancelChat: (chatId: string) => invoke<void>("cancel_chat", { chatId }),

  listFolders: () => invoke<Folder[]>("list_folders"),
  createFolder: (name: string, parentId: string | null) => invoke<Folder>("create_folder", { name, parentId }),
  updateFolder: (id: string, name: string, parentId: string | null, defaultTemplateId: string | null) =>
    invoke<void>("update_folder", { id, name, parentId, defaultTemplateId }),
  deleteFolder: (id: string) => invoke<void>("delete_folder", { id }),
  moveMeeting: (id: string, folderId: string | null) => invoke<void>("move_meeting", { id, folderId }),
  listMeetingsFiltered: (filter: { folder: null | (string | null)[]; tag_id: string | null }) =>
    invoke<Meeting[]>("list_meetings_filtered", { filter: { folder: filter.folder === null ? null : filter.folder[0], tag_id: filter.tag_id } }),
  listTags: () => invoke<Tag[]>("list_tags"),
  meetingTags: (id: string) => invoke<Tag[]>("meeting_tags", { id }),
  tagMeeting: (id: string, name: string) => invoke<Tag>("tag_meeting", { id, name }),
  untagMeeting: (id: string, tagId: string) => invoke<void>("untag_meeting", { id, tagId }),
  exportClip: (id: string, startMs: number, endMs: number, out: string) => invoke<string>("export_clip", { id, startMs, endMs, out }),
  createShare: (id: string, startMs?: number, endMs?: number) => invoke<ShareInfo>("create_share", { id, startMs, endMs }),
  listShares: () => invoke<ShareInfo[]>("list_shares"),
  setShareEnabled: (token: string, enabled: boolean) => invoke<void>("set_share_enabled", { token, enabled }),
  deleteShare: (token: string) => invoke<void>("delete_share", { token }),
  exportMeetings: (ids: string[], out: string, includeVideo: boolean) => invoke<number>("export_meetings", { ids, out, includeVideo }),
  importMeetings: (path: string) => invoke<ImportReport>("import_meetings", { path }),
  exportHtml: (id: string, outDir: string) => invoke<string>("export_html", { id, outDir }),
  changeDataDir: (newDir: string) => invoke<string>("change_data_dir", { newDir }),

  listAudioDevices: () => invoke<{ inputs: AudioDevice[]; outputs: AudioDevice[] }>("list_audio_devices"),
  sampleLevels: (mic: string | null, loopback: string | null) =>
    invoke<[number, number]>("sample_levels", { mic, loopback }),
};

export interface Caption {
  start_ms: number;
  end_ms: number;
  text: string;
  is_final: boolean;
}

/** Captions and running notes of the recording in progress. */
export interface LiveSnapshot {
  meeting_id: string;
  finals: Caption[];
  partial: Caption | null;
  notes: string;
  notes_updated_ms: number | null;
}

export interface Events {
  detection: { app: string; label: string; title: string; confidence: number; event: string | null };
  levels: { mic_db: number; sys_db: number };
  caption: Caption;
  live_notes: { meeting_id: string; notes: string; updated_ms: number };
  recording_state: RecordingState;
  model_progress: { id: string; done: number; total: number; status: "downloading" | "done" | "failed"; error: string | null };
  processing: { meeting_id: string; stage: string; progress: number; error: string | null };
  notice: { level: "info" | "warning" | "error"; message: string };
  chat_token: { chat_id: string; message_id: string; delta: string };
  chat_done: { chat_id: string; message: ChatMessage; error: string | null };
  participants: ParticipantsPayload;
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
