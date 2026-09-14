// Dev-only stand-in for the Rust backend so the UI can run in a plain browser
// (`pnpm dev` outside Tauri). Enabled automatically when Tauri is absent.
import type { ChatMessage, Meeting, MeetingDetail, Settings, Summary, Template } from "./ipc";

const now = Date.now();
const iso = (minsAgo: number) => new Date(now - minsAgo * 60_000).toISOString();

const meetings: Meeting[] = [
  { id: "m1", title: "Q4 launch planning", app: "zoom", started_at: iso(95), ended_at: iso(50), duration_ms: 45 * 60_000, has_video: true, status: "ready", folder_id: "f1", created_at: iso(95), error: null, title_auto: false, participants: ["Sarah Chen", "Marcus Webb", "Priya Patel"], calendar_uid: null },
  { id: "m2", title: "Priya 1:1", app: "meet", started_at: iso(24 * 60), ended_at: iso(24 * 60 - 28), duration_ms: 28 * 60_000, has_video: false, status: "ready", folder_id: null, created_at: iso(24 * 60), error: null, title_auto: true, participants: ["Priya Patel"], calendar_uid: null },
  { id: "m3", title: "Acme discovery call", app: "teams", started_at: iso(3 * 24 * 60), ended_at: iso(3 * 24 * 60 - 41), duration_ms: 41 * 60_000, has_video: true, status: "ready", folder_id: "f2", created_at: iso(3 * 24 * 60), error: null, title_auto: true, participants: [], calendar_uid: null },
  { id: "m4", title: "Standup", app: "discord", started_at: iso(4 * 24 * 60), ended_at: null, duration_ms: 12 * 60_000, has_video: false, status: "processing", folder_id: null, created_at: iso(4 * 24 * 60), error: null, title_auto: true, participants: [], calendar_uid: null },
];

const lines: [number, string, string, string | null][] = [
  [0, "Alex", "Okay so the main thing today is the launch date for the new site.", null],
  [12_000, "Priya", "right um design is done but the the checkout integration slipped a week i think march third is realistic", "Right. Design is done, but the checkout integration slipped a week. I think March 3rd is realistic."],
  [30_000, "Alex", "Fine, March 3rd it is. Can you own the Stripe piece and have it in staging by Friday?", null],
  [41_000, "Priya", "yes one question though do we still want the annual plan at launch or push it", "Yes. One question though: do we still want the annual plan at launch, or push it?"],
  [55_000, "Alex", "Push it. Monthly only for launch. I'll tell Sam to update the pricing page.", null],
  [68_000, "Priya", "got it also the budget we're at 42k of the 50k so we have room for the QA contractor", "Got it. Also, the budget: we're at 42k of the 50k, so we have room for the QA contractor."],
  [80_000, "Alex", "Approved, bring them in for two weeks.", null],
  [92_000, "Sam", "um I can have the pricing page done by Wednesday if the copy is final", "I can have the pricing page done by Wednesday if the copy is final."],
];

const detail: MeetingDetail = {
  meeting: meetings[0],
  segments: lines.map(([start_ms, speaker, text, clean_text], i) => ({ id: i + 1, meeting_id: "m1", start_ms, end_ms: start_ms + 9000, speaker, text, clean_text })),
  media: { audio: "", video: null },
  highlights: [30_000, 80_000],
  speakers: [
    { meeting_id: "m1", label: "Sam", speaker_id: null, suggested_id: "s1", suggested_name: "Sam Ortiz", suggested_score: 0.83 },
  ],
};

const summary: Summary = {
  meeting_id: "m1",
  template_id: "general",
  model: "Qwen3-8B-Q4_K_M",
  created_at: iso(48),
  structured: null,
  markdown: `## Summary
The team fixed the launch date for the new site at March 3rd after the checkout integration slipped a week. Priya owns the Stripe integration, due in staging by Friday. The annual plan is deferred; launch is monthly-only. A QA contractor was approved for two weeks within the remaining budget.

## Decisions
- Launch date set to March 3rd.
- Monthly plan only at launch; annual plan deferred.
- QA contractor approved for two weeks.

## Action items
- [ ] Stripe integration in staging - Priya (Friday)
- [ ] Update pricing page to monthly-only - Sam (Wednesday)
- [ ] Bring in QA contractor - Alex

## Open questions
- Final copy for the pricing page.

## Key moments
- [0:12] Priya flags the checkout slip and proposes March 3rd
- [0:55] Decision to launch monthly-only
- [1:08] Budget check: 42k of 50k used`,
};

const settings: Settings = {
  onboarded: true, user_name: "Alex", language: "en", mic_device: null, loopback_device: null, capture_system: true, video_enabled: true,
  video_target: { kind: "monitor", index: 0 }, video_fps: 15, video_max_height: 1080, tier_override: null, live_asr_model: null, quality_asr_model: null, llm_model: null,
  hotkey: "CmdOrCtrl+Shift+R", detection_enabled: true, audio_activity_enabled: true, never_apps: [], popup_timeout_secs: 30, close_to_tray: true,
  llm_backend: "bundled", llm_endpoint: "http://localhost:11434/v1", llm_endpoint_model: "qwen3:8b", llm_api_key: null, cleanup_enabled: true, diarize_enabled: true,
  summary_enabled: true, default_template_id: "general", share_port: 47123, calendar_sources: [], calendar_refresh_min: 15, webhook_url: "", webhook_enabled: false, auto_update_check: true,
};

const templates: Template[] = [
  { id: "general", name: "General meeting", description: "Balanced notes", body: "...{{transcript}}", builtin: true },
  { id: "sales-call", name: "Sales call", description: "Deal notes", body: "...{{transcript}}", builtin: true },
];

const chatMsgs: ChatMessage[] = [
  { id: "c1", chat_id: "chat1", role: "user", content: "What did we decide about pricing?", citations: [], created_at: iso(10) },
  { id: "c2", chat_id: "chat1", role: "assistant", content: "You decided to launch with the **monthly plan only** and push the annual plan to after launch [0:55]. Sam is updating the pricing page by Wednesday, pending final copy [1:32].", citations: [{ ms: 55000, meeting_id: "m1", label: "0:55" }, { ms: 92000, meeting_id: "m1", label: "1:32" }], created_at: iso(10) },
];

const table: Record<string, (args: Record<string, unknown>) => unknown> = {
  list_meetings: () => meetings,
  list_meetings_filtered: () => meetings,
  get_meeting: ({ id }) => ({ ...detail, meeting: meetings.find((m) => m.id === id) ?? meetings[0] }),
  get_settings: () => settings,
  set_settings: ({ new: s }) => s,
  recording_status: () => ({ state: "idle", meeting_id: null, elapsed_ms: 0 }),
  list_upcoming: () => [
    { uid: "e1", title: "Design review with Acme", start: iso(-20), end: iso(-50), attendees: ["Sarah Chen", "Marcus Webb"], url: "https://meet.google.com/abc-defg-hij", location: null },
    { uid: "e2", title: "Weekly standup", start: iso(-150), end: iso(-165), attendees: [], url: null, location: null },
  ],
  refresh_calendar: () => [],
  search: () => [],
  list_folders: () => [
    { id: "f1", name: "Launch", parent_id: null, default_template_id: null, meeting_count: 1 },
    { id: "f2", name: "Clients", parent_id: null, default_template_id: null, meeting_count: 1 },
    { id: "f3", name: "Acme", parent_id: "f2", default_template_id: null, meeting_count: 0 },
  ],
  list_tags: () => [{ id: "t1", name: "sales", meeting_count: 1 }, { id: "t2", name: "planning", meeting_count: 2 }],
  meeting_tags: () => [{ id: "t2", name: "planning", meeting_count: 2 }],
  get_summary: () => summary,
  list_templates: () => templates,
  list_chats: () => [{ id: "chat1", scope_kind: "meeting", scope_id: "m1", title: "What did we decide about pricing?", created_at: iso(10) }],
  chat_messages: () => chatMsgs,
  list_shares: () => [],
  list_audio_devices: () => ({ inputs: [], outputs: [] }),
  list_video_sources: () => [
    { target: { kind: "monitor", index: 0 }, label: "DISPLAY1 (2560x1440), primary", is_meeting: false },
    { target: { kind: "monitor", index: 1 }, label: "DISPLAY2 (1920x1080)", is_meeting: false },
    { target: { kind: "window", title: "Zoom Meeting" }, label: "Zoom Meeting", is_meeting: true },
  ],
  sample_levels: () => [-30 + Math.random() * 10, -28 + Math.random() * 12],
  probe_hardware: () => ({ hardware: { cpu_cores: 16, cpu_name: "Ryzen 9 5900X", ram_gb: 31.2, gpu: { name: "NVIDIA GeForce RTX 3080", vram_gb: 10, backend: "cuda" }, os: "windows", arch: "x86_64" }, tier: "gpu", effective_tier: "gpu" }),
  list_models: () => [],
  data_info: () => ({ data_dir: "C:\\Users\\alex\\AppData\\Roaming\\Hark", recordings_bytes: 1.4e9, models_bytes: 6.9e9 }),
};

export async function mockInvoke<T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
  const fn = table[cmd];
  if (!fn) return undefined as T;
  return fn(args) as T;
}

export const isMock = typeof window !== "undefined" && !("__TAURI_INTERNALS__" in window);
