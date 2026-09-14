import { open, save } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";
import { Download, Trash2, X } from "lucide-react";
import { useEffect, useState } from "react";
import { TemplatesEditor } from "@/components/TemplatesEditor";
import { Button, Card, Input, Meter, SectionTitle, Select, Spinner, Toggle } from "@/components/ui";
import { appLabel, fmtBytes } from "@/lib/format";
import { LANGUAGES } from "@/lib/languages";
import { checkForUpdate } from "@/lib/updates";
import { cmd, subscribe, type AudioDevice, type Hardware, type ModelRow, type Settings, type ShareInfo, type Tier } from "@/lib/ipc";

const TIER_LABEL: Record<Tier, string> = { cpu_low: "CPU, light models", cpu_high: "CPU, bigger models", gpu: "GPU, best models" };

export function SettingsPage() {
  const [s, setS] = useState<Settings | null>(null);
  const [devices, setDevices] = useState<{ inputs: AudioDevice[]; outputs: AudioDevice[] }>({ inputs: [], outputs: [] });
  const [levels, setLevels] = useState<[number, number]>([-100, -100]);
  const [hw, setHw] = useState<{ hardware: Hardware; tier: Tier; effective_tier: Tier } | null>(null);
  const [models, setModels] = useState<ModelRow[]>([]);
  const [calendarDraft, setCalendarDraft] = useState("");
  const [calendarErrors, setCalendarErrors] = useState<string[]>([]);
  const [calendarCount, setCalendarCount] = useState<number | null>(null);
  const [webhookMsg, setWebhookMsg] = useState("");
  const [updateMsg, setUpdateMsg] = useState("");
  const [progress, setProgress] = useState<Record<string, { done: number; total: number; error?: string | null }>>({});
  const [data, setData] = useState<{ data_dir: string; recordings_bytes: number; models_bytes: number } | null>(null);
  const [hotkeyDraft, setHotkeyDraft] = useState("");
  const [saved, setSaved] = useState<string | null>(null);
  const [endpointTest, setEndpointTest] = useState<string | null>(null);
  const [dataMsg, setDataMsg] = useState<string | null>(null);
  const [shares, setShares] = useState<ShareInfo[]>([]);

  const reloadModels = () => void cmd.listModels().then(setModels);

  useEffect(() => {
    void cmd.getSettings().then((x) => {
      setS(x);
      setHotkeyDraft(x.hotkey);
      setCalendarDraft(x.calendar_sources.join("\n"));
    });
    void cmd.listAudioDevices().then(setDevices);
    void cmd.probeHardware().then(setHw);
    void cmd.dataInfo().then(setData);
    void cmd.listShares().then(setShares);
    reloadModels();
    return subscribe("model_progress", (p) => {
      setProgress((m) => ({ ...m, [p.id]: { done: p.done, total: p.total, error: p.error } }));
      if (p.status !== "downloading") {
        reloadModels();
        void cmd.dataInfo().then(setData);
      }
    });
  }, []);

  // Live meters while on this page (paused when recording; the recorder owns the devices then).
  useEffect(() => {
    if (!s) return;
    let stop = false;
    const loop = async () => {
      while (!stop) {
        try {
          const st = await cmd.recordingStatus();
          if (st.state === "idle") setLevels(await cmd.sampleLevels(s.mic_device, s.loopback_device));
        } catch {
          /* ignore */
        }
        await new Promise((r) => setTimeout(r, 600));
      }
    };
    void loop();
    return () => {
      stop = true;
    };
  }, [s?.mic_device, s?.loopback_device, s]);

  const update = async (patch: Partial<Settings>) => {
    if (!s) return;
    const next = { ...s, ...patch };
    setS(next);
    try {
      await cmd.setSettings(next);
      setSaved("Saved");
      setTimeout(() => setSaved(null), 1200);
      if (patch.tier_override !== undefined || patch.live_asr_model !== undefined || patch.quality_asr_model !== undefined || patch.llm_model !== undefined) {
        reloadModels();
        void cmd.probeHardware().then(setHw);
      }
    } catch (e) {
      setSaved(String(e));
    }
  };

  if (!s) return <div className="p-8"><Spinner /></div>;

  const asr = models.filter((m) => m.spec.kind === "asr");
  const llm = models.filter((m) => m.spec.kind === "llm");

  return (
    <div className="mx-auto max-w-3xl px-8 py-8">
      <header className="mb-8 flex items-end justify-between">
        <div>
          <h1 className="font-serif text-[34px] leading-none tracking-tight">Settings</h1>
          <p className="mt-1.5 text-[13px] text-ink-3">Everything stays on this computer.</p>
        </div>
        <span className="text-[12px] text-moss">{saved}</span>
      </header>

      <section className="mb-10">
        <SectionTitle>Audio</SectionTitle>
        <Card className="divide-y divide-line px-5">
          <div className="grid grid-cols-2 gap-4 py-4">
            <label className="block">
              <span className="mb-1 block text-[12px] font-medium text-ink-2">Microphone</span>
              <Select value={s.mic_device ?? ""} onChange={(e) => void update({ mic_device: e.target.value || null })}>
                <option value="">System default</option>
                {devices.inputs.map((d) => (
                  <option key={d.id} value={d.id}>{d.name}</option>
                ))}
              </Select>
              <Meter db={levels[0]} className="mt-2" />
            </label>
            <label className="block">
              <span className="mb-1 block text-[12px] font-medium text-ink-2">System audio (what you hear)</span>
              <Select value={s.loopback_device ?? ""} onChange={(e) => void update({ loopback_device: e.target.value || null })}>
                <option value="">System default output</option>
                {devices.outputs.map((d) => (
                  <option key={d.id} value={d.id}>{d.name}</option>
                ))}
              </Select>
              <Meter db={levels[1]} className="mt-2" />
            </label>
          </div>
          <Toggle label="Capture system audio" description="Records the other participants. Turn off to record only your microphone." checked={s.capture_system} onChange={(v) => void update({ capture_system: v })} />
          <Toggle label="Record screen video by default" description="You can still toggle this on the Record popup each time." checked={s.video_enabled} onChange={(v) => void update({ video_enabled: v })} />
          <div className="py-4">
            <span className="mb-1 block text-[12px] font-medium text-ink-2">Your name (used for your speaker label)</span>
            <Input value={s.user_name} onChange={(e) => setS({ ...s, user_name: e.target.value })} onBlur={() => void update({ user_name: s.user_name })} className="max-w-xs" />
          </div>
          <div className="py-4">
            <span className="mb-1 block text-[12px] font-medium text-ink-2">Spoken language</span>
            <Select value={s.language ?? ""} onChange={(e) => void update({ language: e.target.value || null })} className="max-w-xs">
              {LANGUAGES.map((l) => <option key={l.code ?? "auto"} value={l.code ?? ""}>{l.name}</option>)}
            </Select>
            <span className="mt-1 block text-[12px] text-ink-3">Auto-detect works well for whole meetings in one language; pick one for mixed or short calls. Cleanup, summaries and chat follow the transcript's language.</span>
          </div>
        </Card>
      </section>

      <section className="mb-10">
        <SectionTitle>Detection</SectionTitle>
        <Card className="divide-y divide-line px-5">
          <Toggle label="Offer to record when a call is detected" description="Hark only shows a popup. It never records without you clicking Record." checked={s.detection_enabled} onChange={(v) => void update({ detection_enabled: v })} />
          <Toggle label="Also detect by audio activity" description="Catches apps we don't know about when both mic and speakers are active." checked={s.audio_activity_enabled} onChange={(v) => void update({ audio_activity_enabled: v })} />
          <div className="py-4">
            <span className="mb-1 block text-[12px] font-medium text-ink-2">Global hotkey (start / stop)</span>
            <form
              className="flex max-w-md gap-2"
              onSubmit={(e) => {
                e.preventDefault();
                void update({ hotkey: hotkeyDraft });
              }}
            >
              <Input value={hotkeyDraft} onChange={(e) => setHotkeyDraft(e.target.value)} placeholder="CmdOrCtrl+Shift+R" className="font-mono" />
              <Button type="submit" variant="outline" size="md">Apply</Button>
            </form>
          </div>
          {s.never_apps.length > 0 && (
            <div className="py-4">
              <span className="mb-2 block text-[12px] font-medium text-ink-2">Never ask for</span>
              <div className="flex flex-wrap gap-2">
                {s.never_apps.map((a) => (
                  <button key={a} onClick={() => void update({ never_apps: s.never_apps.filter((x) => x !== a) })} className="inline-flex items-center gap-1 rounded-full bg-canvas-3 px-2.5 py-1 text-[12px] hover:bg-line-2">
                    {appLabel(a)} <X size={12} />
                  </button>
                ))}
              </div>
            </div>
          )}
          <Toggle label="Keep running in the tray when the window is closed" checked={s.close_to_tray} onChange={(v) => void update({ close_to_tray: v })} />
        </Card>
      </section>

      <section className="mb-10">
        <SectionTitle hint={hw ? `${hw.hardware.cpu_name || "CPU"}, ${Math.round(hw.hardware.ram_gb)} GB RAM${hw.hardware.gpu ? `, ${hw.hardware.gpu.name} (${Math.round(hw.hardware.gpu.vram_gb)} GB)` : ""}` : ""}>
          AI models
        </SectionTitle>
        <Card className="px-5">
          <div className="grid grid-cols-2 gap-4 py-4">
            <label className="block">
              <span className="mb-1 block text-[12px] font-medium text-ink-2">Model tier</span>
              <Select value={s.tier_override ?? ""} onChange={(e) => void update({ tier_override: (e.target.value || null) as Tier | null })}>
                <option value="">Auto{hw ? ` (${TIER_LABEL[hw.tier]})` : ""}</option>
                {(["cpu_low", "cpu_high", "gpu"] as Tier[]).map((t) => (
                  <option key={t} value={t}>{TIER_LABEL[t]}</option>
                ))}
              </Select>
            </label>
            <div className="grid grid-cols-1 gap-2">
              <ModelPick label="Live captions" value={s.live_asr_model} options={asr} onChange={(v) => void update({ live_asr_model: v })} />
              <ModelPick label="Final transcript" value={s.quality_asr_model} options={asr} onChange={(v) => void update({ quality_asr_model: v })} />
              <ModelPick label="Summaries & chat" value={s.llm_model} options={llm} onChange={(v) => void update({ llm_model: v })} />
            </div>
          </div>
          <ul className="divide-y divide-line border-t border-line">
            {models.map((m) => {
              const p = progress[m.spec.id];
              const pct = p && p.total ? Math.round((p.done / p.total) * 100) : 0;
              return (
                <li key={m.spec.id} className="flex items-center gap-4 py-3">
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-2 text-[14px] font-medium">
                      {m.spec.name}
                      {m.roles.map((r) => (
                        <span key={r} className="rounded bg-canvas-3 px-1.5 py-px text-[10px] font-semibold tracking-wide text-ink-2 uppercase">{r}</span>
                      ))}
                    </div>
                    <div className="text-[12px] text-ink-3">{m.spec.note} {fmtBytes(m.spec.size_bytes)}</div>
                    {m.downloading && (
                      <div className="mt-1.5 h-1 w-full overflow-hidden rounded-full bg-line">
                        <div className="h-full bg-ink transition-[width]" style={{ width: `${pct}%` }} />
                      </div>
                    )}
                    {p?.error && <div className="mt-1 text-[12px] text-ember">{p.error}</div>}
                  </div>
                  {m.present ? (
                    <Button variant="ghost" size="sm" onClick={() => cmd.removeModel(m.spec.id).then(reloadModels)} title="Delete from disk">
                      <Trash2 size={14} /> Remove
                    </Button>
                  ) : m.downloading ? (
                    <Button variant="ghost" size="sm" onClick={() => void cmd.cancelDownload(m.spec.id)}>
                      <X size={14} /> {pct}%
                    </Button>
                  ) : (
                    <Button variant="outline" size="sm" onClick={() => { void cmd.downloadModel(m.spec.id).catch(() => {}); reloadModels(); }}>
                      <Download size={14} /> Download
                    </Button>
                  )}
                </li>
              );
            })}
          </ul>
        </Card>
      </section>

      <section className="mb-10">
        <SectionTitle>AI assistant</SectionTitle>
        <Card className="divide-y divide-line px-5">
          <div className="py-4">
            <span className="mb-2 block text-[12px] font-medium text-ink-2">Language model runs...</span>
            <div className="grid grid-cols-2 gap-2">
              {([
                ["bundled", "Inside Hark", "Built-in llama.cpp with the downloaded model. Nothing to install."],
                ["openai", "On an endpoint I run", "Ollama, LM Studio or any OpenAI-compatible server on this machine or your network."],
              ] as const).map(([v, title, body]) => (
                <button
                  key={v}
                  onClick={() => void update({ llm_backend: v })}
                  className={`focus-ring rounded-lg border p-3 text-left transition-colors ${s.llm_backend === v ? "border-ink bg-canvas shadow-card" : "border-line hover:bg-canvas-2"}`}
                >
                  <div className="text-[13px] font-semibold">{title}</div>
                  <div className="text-[12px] text-ink-3">{body}</div>
                </button>
              ))}
            </div>
            {s.llm_backend === "openai" && (
              <div className="mt-3 grid grid-cols-[1fr_1fr_auto] gap-2">
                <Input value={s.llm_endpoint} placeholder="http://localhost:11434/v1" onChange={(e) => setS({ ...s, llm_endpoint: e.target.value })} onBlur={() => void update({ llm_endpoint: s.llm_endpoint })} className="font-mono text-[12px]" />
                <Input value={s.llm_endpoint_model} placeholder="model name, e.g. qwen3:8b" onChange={(e) => setS({ ...s, llm_endpoint_model: e.target.value })} onBlur={() => void update({ llm_endpoint_model: s.llm_endpoint_model })} className="font-mono text-[12px]" />
                <Button
                  variant="outline"
                  size="md"
                  onClick={() => {
                    setEndpointTest("Testing...");
                    cmd.testLlmEndpoint(s.llm_endpoint, s.llm_api_key, s.llm_endpoint_model).then(
                      (models) => setEndpointTest(models.includes(s.llm_endpoint_model) ? `OK, ${models.length} models, "${s.llm_endpoint_model}" found` : `Reachable, but "${s.llm_endpoint_model}" not in: ${models.slice(0, 6).join(", ")}`),
                      (e) => setEndpointTest(`Failed: ${e}`),
                    );
                  }}
                >
                  Test
                </Button>
                <Input value={s.llm_api_key ?? ""} type="password" placeholder="API key (optional)" onChange={(e) => setS({ ...s, llm_api_key: e.target.value || null })} onBlur={() => void update({ llm_api_key: s.llm_api_key })} className="col-span-2 font-mono text-[12px]" />
                {endpointTest && <div className="col-span-3 text-[12px] text-ink-2">{endpointTest}</div>}
              </div>
            )}
          </div>
          <Toggle label="Identify speakers" description="Who spoke when, plus voice memory so you only name someone once. Uses two small local models." checked={s.diarize_enabled} onChange={(v) => void update({ diarize_enabled: v })} />
          <Toggle label="Clean up transcripts with AI" description="Fixes garbled words, broken English and filler. The raw transcript is always kept." checked={s.cleanup_enabled} onChange={(v) => void update({ cleanup_enabled: v })} />
          <Toggle label="Write a summary after each recording" description="Uses the default template below. You can regenerate with any template later." checked={s.summary_enabled} onChange={(v) => void update({ summary_enabled: v })} />
        </Card>
      </section>

      <section className="mb-10">
        <SectionTitle hint="Markdown prompts the summary is written from">Summary templates</SectionTitle>
        <Card className="p-5">
          <TemplatesEditor defaultId={s.default_template_id} onDefaultChange={(id) => void update({ default_template_id: id })} />
        </Card>
      </section>

      <section className="mb-10">
        <SectionTitle hint="Names recordings after the event you are in and seeds attendees">Calendar</SectionTitle>
        <Card className="px-5 py-4 text-[13px]">
          <p className="text-ink-3">
            Paste a private .ics link (Google Calendar: Settings &rarr; your calendar &rarr; "Secret address in iCal format"; Outlook: Settings &rarr; Shared calendars &rarr; Publish) or a path to an .ics file. Hark fetches only these addresses, every {s.calendar_refresh_min} minutes.
          </p>
          <textarea
            value={calendarDraft}
            onChange={(e) => setCalendarDraft(e.target.value)}
            onBlur={() => {
              const list = calendarDraft.split("\n").map((l) => l.trim()).filter(Boolean);
              if (JSON.stringify(list) !== JSON.stringify(s.calendar_sources)) void update({ calendar_sources: list }).then(() => cmd.refreshCalendar().then(setCalendarErrors));
            }}
            placeholder={"https://calendar.google.com/calendar/ical/.../private-.../basic.ics\nC:\\Users\\me\\work.ics"}
            rows={3}
            spellCheck={false}
            className="focus-ring mt-3 w-full rounded-md border border-line-2 bg-canvas px-3 py-2 font-mono text-[12px] text-ink placeholder:text-ink-3"
          />
          <div className="mt-2 flex items-center gap-3">
            <Button variant="outline" size="sm" onClick={() => void cmd.refreshCalendar().then((e) => { setCalendarErrors(e); void cmd.listUpcoming(24 * 7).then((ev) => setCalendarCount(ev.length)); })}>Refresh now</Button>
            {calendarCount !== null && <span className="text-ink-3">{calendarCount} events in the next 7 days</span>}
          </div>
          {calendarErrors.map((e) => <div key={e} className="mt-2 text-ember">{e}</div>)}
        </Card>
      </section>

      <section className="mb-10">
        <SectionTitle hint="Push finished meetings into Zapier, n8n, Make, a CRM, or your own script">Webhook</SectionTitle>
        <Card className="divide-y divide-line px-5 text-[13px]">
          <Toggle label="Send each finished meeting to a URL" description="POSTs JSON with the meeting, summary, transcript and attendees once processing ends. Nothing is sent unless this is on." checked={s.webhook_enabled} onChange={(v) => void update({ webhook_enabled: v })} />
          <div className="py-4">
            <span className="mb-1 block text-[12px] font-medium text-ink-2">URL</span>
            <div className="flex gap-2">
              <Input value={s.webhook_url} placeholder="https://hooks.zapier.com/hooks/catch/..." onChange={(e) => setS({ ...s, webhook_url: e.target.value })} onBlur={() => void update({ webhook_url: s.webhook_url })} className="font-mono text-[12px]" />
              <Button variant="outline" size="md" className="shrink-0" disabled={!s.webhook_url.trim()} onClick={() => void cmd.testWebhook(s.webhook_url).then((st) => setWebhookMsg(`OK (HTTP ${st})`), (e) => setWebhookMsg(String(e)))}>Send test</Button>
            </div>
            {webhookMsg && <div className={`mt-1.5 text-[12px] ${webhookMsg.startsWith("OK") ? "text-moss" : "text-ember"}`}>{webhookMsg}</div>}
          </div>
        </Card>
      </section>

      <section className="mb-10">
        <SectionTitle>Updates</SectionTitle>
        <Card className="divide-y divide-line px-5 text-[13px]">
          <Toggle label="Check for updates on startup" description="Fetches one small file from GitHub releases. Installing is always your click." checked={s.auto_update_check} onChange={(v) => void update({ auto_update_check: v })} />
          <div className="flex items-center gap-3 py-4">
            <Button variant="outline" size="sm" onClick={() => { setUpdateMsg("Checking..."); void checkForUpdate().then((u) => setUpdateMsg(u ? `Hark ${u.version} is available.` : "You are on the latest version.")); }}>Check for updates</Button>
            <span className="text-ink-3">{updateMsg}</span>
          </div>
        </Card>
      </section>

      <section className="mb-10">
        <SectionTitle>Data</SectionTitle>
        <Card className="divide-y divide-line px-5 text-[13px]">
          <div className="flex items-center justify-between gap-4 py-4">
            <div className="min-w-0">
              <div className="truncate font-mono text-[12px] text-ink-2">{data?.data_dir}</div>
              <div className="mt-1 text-ink-3">
                Recordings {fmtBytes(data?.recordings_bytes ?? 0)}, models {fmtBytes(data?.models_bytes ?? 0)}
              </div>
            </div>
            <div className="flex shrink-0 gap-2">
              <Button variant="outline" size="sm" onClick={() => data && void openPath(data.data_dir)}>Open folder</Button>
              <Button variant="outline" size="sm" onClick={() => void (async () => {
                const dir = await open({ directory: true, title: "Choose the new Hark data folder" });
                if (!dir) return;
                setDataMsg("Copying...");
                try {
                  const to = await cmd.changeDataDir(dir as string);
                  setDataMsg(`Copied to ${to}. Restart Hark to use it; the old folder was left in place.`);
                } catch (e) {
                  setDataMsg(String(e));
                }
              })()}>Change folder</Button>
            </div>
          </div>
          <div className="flex flex-wrap items-center gap-2 py-4">
            <span className="mr-2 text-ink-2">Move meetings between computers:</span>
            <Button variant="outline" size="sm" onClick={() => void (async () => {
              const out = await save({ defaultPath: "hark-library.hark", filters: [{ name: "Hark bundle", extensions: ["hark"] }] });
              if (!out) return;
              setDataMsg("Exporting...");
              try {
                const n = await cmd.exportMeetings([], out, true);
                setDataMsg(`Exported ${n} meeting${n === 1 ? "" : "s"} to ${out}`);
              } catch (e) {
                setDataMsg(String(e));
              }
            })()}>Export everything</Button>
            <Button variant="outline" size="sm" onClick={() => void (async () => {
              const p = await open({ filters: [{ name: "Hark bundle", extensions: ["hark"] }], multiple: false });
              if (!p) return;
              setDataMsg("Importing...");
              try {
                const r = await cmd.importMeetings(p as string);
                setDataMsg(`Imported ${r.imported}, skipped ${r.skipped_existing} already here${r.errors.length ? `, errors: ${r.errors.join("; ")}` : ""}.`);
                void cmd.dataInfo().then(setData);
              } catch (e) {
                setDataMsg(String(e));
              }
            })()}>Import bundle</Button>
            {dataMsg && <span className="basis-full text-[12px] text-ink-2">{dataMsg}</span>}
          </div>
          <div className="py-4">
            <div className="mb-2 flex items-center justify-between">
              <span className="text-ink-2">Share links (port {s.share_port})</span>
              <span className="text-[12px] text-ink-3">{shares.filter((x) => x.share.enabled).length} active</span>
            </div>
            {shares.length === 0 ? (
              <div className="text-[12px] text-ink-3">No links yet. Use Share on a meeting.</div>
            ) : (
              <ul className="flex flex-col gap-1">
                {shares.map((x) => (
                  <li key={x.share.token} className="flex items-center gap-2 text-[12px]">
                    <span className="min-w-0 flex-1 truncate">{x.share.meeting_title} <span className="text-ink-3">({x.share.kind})</span></span>
                    <span className="truncate font-mono text-ink-3">{x.url}</span>
                    <Button variant="ghost" size="sm" onClick={() => void cmd.setShareEnabled(x.share.token, !x.share.enabled).then(() => cmd.listShares()).then(setShares)}>{x.share.enabled ? "Disable" : "Enable"}</Button>
                    <Button variant="danger" size="sm" onClick={() => void cmd.deleteShare(x.share.token).then(() => cmd.listShares()).then(setShares)}><Trash2 size={13} /></Button>
                  </li>
                ))}
              </ul>
            )}
          </div>
        </Card>
      </section>

      <p className="text-[12px] text-ink-3">Hark 0.1.0. Open source, MIT. No accounts, no telemetry, no cloud.</p>
    </div>
  );
}

function ModelPick({ label, value, options, onChange }: { label: string; value: string | null; options: ModelRow[]; onChange: (v: string | null) => void }) {
  return (
    <label className="flex items-center gap-3">
      <span className="w-28 shrink-0 text-[12px] font-medium text-ink-2">{label}</span>
      <Select value={value ?? ""} onChange={(e) => onChange(e.target.value || null)} className="h-8 text-[13px]">
        <option value="">Tier default</option>
        {options.map((o) => (
          <option key={o.spec.id} value={o.spec.id}>{o.spec.name}{o.present ? "" : " (not downloaded)"}</option>
        ))}
      </Select>
    </label>
  );
}
