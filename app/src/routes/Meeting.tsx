import { save } from "@tauri-apps/plugin-dialog";
import { AlertTriangle, ArrowLeft, Check, Copy, Download, FolderOpen, Mail, Pencil, RefreshCw, Share2, Sparkles, Tag as TagIcon, Trash2, Users, X } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { Link, useNavigate, useParams, useSearchParams } from "react-router-dom";
import { ChatPanel } from "@/components/ChatPanel";
import { LiveNotes, LiveTranscript, useLiveFeed } from "@/components/LiveView";
import { Markdown } from "@/components/Markdown";
import { Player, type PlayerHandle } from "@/components/Player";
import { ShareDialog } from "@/components/ShareDialog";
import { SpeakersPanel } from "@/components/SpeakersPanel";
import { Transcript } from "@/components/Transcript";
import { Badge, Button, Input, Menu, SectionTitle, Spinner } from "@/components/ui";
import { safeFileName, transcriptMd, transcriptSrt, transcriptTxt } from "@/lib/export";
import { appLabel, fmtDate, fmtDuration, fmtTime } from "@/lib/format";
import { cmd, subscribe, type Folder, type MeetingDetail, type ModelRow, type Summary, type Tag, type Template } from "@/lib/ipc";
import { talkTime } from "@/lib/talktime";

const TALK_COLORS = ["bg-ink", "bg-moss", "bg-amber", "bg-ember", "bg-ink-3", "bg-line-2"];

const STAGE_LABEL: Record<string, string> = {
  mux: "Finalising video",
  transcribe: "Transcribing",
  diarize: "Detecting speakers",
  cleanup: "Cleaning up the transcript",
  embed: "Indexing for search",
  title: "Naming the meeting",
  summary: "Writing the summary",
};

export function MeetingPage() {
  const { id = "" } = useParams();
  const [params] = useSearchParams();
  const navigate = useNavigate();
  const [detail, setDetail] = useState<MeetingDetail | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [currentMs, setCurrentMs] = useState(0);
  const [editing, setEditing] = useState(false);
  const [title, setTitle] = useState("");
  const [stage, setStage] = useState<string | null>(null);
  const [progress, setProgress] = useState(0);
  const [view, setView] = useState<"clean" | "raw" | null>(null);
  const [dismissed, setDismissed] = useState<string[]>([]);
  const [tab, setTab] = useState<"transcript" | "summary" | "chat">("transcript");
  const [summary, setSummary] = useState<Summary | null>(null);
  const [templates, setTemplates] = useState<Template[]>([]);
  const [templateId, setTemplateId] = useState<string>("");
  const [share, setShare] = useState(false);
  const [tags, setTags] = useState<Tag[]>([]);
  const [folders, setFolders] = useState<Folder[]>([]);
  const [tagDraft, setTagDraft] = useState("");
  const [asrModels, setAsrModels] = useState<ModelRow[]>([]);
  const [knownSpeakers, setKnownSpeakers] = useState<string[]>([]);
  const [followup, setFollowup] = useState<string | null>(null);
  const [followupBusy, setFollowupBusy] = useState(false);
  const [flash, setFlash] = useState<string | null>(null);
  const [stopping, setStopping] = useState(false);
  const player = useRef<PlayerHandle>(null);
  const live = useLiveFeed(id, detail?.meeting.status === "recording");

  const say = (msg: string) => {
    setFlash(msg);
    setTimeout(() => setFlash(null), 1800);
  };

  const load = useCallback(() => {
    cmd.getMeeting(id).then((d) => {
      setDetail(d);
      setTitle(d.meeting.title);
      setError(null);
    }, (e) => setError(String(e)));
    void cmd.getSummary(id).then((s) => {
      setSummary(s);
      if (s) setTemplateId((t) => t || s.template_id);
    });
    void cmd.meetingTags(id).then(setTags);
    void cmd.listFolders().then(setFolders);
  }, [id]);

  useEffect(() => {
    void cmd.listTemplates().then(setTemplates);
    void cmd.listModels().then((ms) => setAsrModels(ms.filter((m) => m.spec.kind === "asr" && m.present)));
    void cmd.listKnownSpeakers().then((ks) => setKnownSpeakers((ks ?? []).map((k) => k.name))).catch(() => {});
  }, []);

  useEffect(() => subscribe("participants", (p) => {
    if (p.meeting_id === id) setDetail((d) => (d ? { ...d, meeting: { ...d.meeting, participants: p.participants } } : d));
  }), [id]);

  // Stop takes a few seconds (last captions, video trailer); the page says so until the status flips.
  useEffect(() => subscribe("recording_state", (s) => {
    if (s.state === "idle") {
      setStopping(true);
      load();
    }
  }), [load]);
  useEffect(() => {
    if (detail && detail.meeting.status !== "recording") setStopping(false);
  }, [detail]);

  useEffect(() => {
    load();
    return subscribe("processing", (p) => {
      if (p.meeting_id !== id) return;
      setStage(p.stage === "done" || p.stage === "failed" ? null : p.stage);
      setProgress(p.progress);
      if (p.stage === "done" || p.stage === "failed" || p.stage === "title") load();
    });
  }, [id, load]);

  useEffect(() => {
    if (params.get("tab") === "chat") setTab("chat");
  }, [params]);

  // Deep link from search: ?t=<ms>
  useEffect(() => {
    const t = Number(params.get("t"));
    if (detail && t > 0) setTimeout(() => player.current?.seek(t), 300);
  }, [detail, params]);

  const onTime = useCallback((ms: number) => setCurrentMs(ms), []);

  if (error) {
    return (
      <div className="p-8 text-sm text-ember">
        {error} <Link to="/" className="underline">Back to library</Link>
      </div>
    );
  }
  if (!detail) return <div className="p-8"><Spinner /></div>;

  const m = detail.meeting;
  const processing = m.status === "processing" || stage !== null;
  const recording = m.status === "recording";
  const hasClean = detail.segments.some((s) => s.clean_text);
  const useClean = (view ?? "clean") === "clean" && hasClean;
  const speakers = detail.speakers.filter((sp) => !dismissed.includes(sp.label));
  const nameOptions = Array.from(new Set([...m.participants, ...knownSpeakers])).filter((n) => n.toLowerCase() !== "me");

  const renameSpeaker = async (label: string, name: string) => {
    await cmd.renameSpeaker(m.id, label, name);
    load();
  };
  const acceptSuggestion = async (label: string) => {
    await cmd.acceptSpeakerSuggestion(m.id, label);
    load();
  };

  const saveTitle = async () => {
    const next = title.trim();
    if (next && next !== m.title) {
      const updated = await cmd.renameMeeting(m.id, next);
      setDetail({ ...detail, meeting: updated });
    }
    setEditing(false);
  };

  const remove = async () => {
    if (!confirm(`Delete "${m.title}"? This removes the recording and transcript from disk.`)) return;
    await cmd.deleteMeeting(m.id);
    navigate("/");
  };

  const dateLine = `${fmtDate(m.started_at)} at ${fmtTime(m.started_at)}`;
  const copyText = async (text: string, what: string) => {
    try {
      await navigator.clipboard.writeText(text);
      say(`${what} copied`);
    } catch (e) {
      alert(`Could not copy: ${String(e)}`);
    }
  };
  const saveText = async (ext: string, text: string, what: string) => {
    const out = await save({ defaultPath: `${safeFileName(m.title)}.${ext}`, filters: [{ name: ext.toUpperCase(), extensions: [ext] }] });
    if (!out) return;
    try {
      await cmd.saveTextFile(out, text);
      say(`${what} saved`);
    } catch (e) {
      alert(String(e));
    }
  };
  const draftFollowup = async () => {
    setFollowupBusy(true);
    try {
      setFollowup(await cmd.draftFollowup(m.id));
    } catch (e) {
      alert(String(e));
    } finally {
      setFollowupBusy(false);
    }
  };
  const shares = talkTime(detail.segments);
  const hasText = detail.segments.length > 0;

  return (
    <div className="mx-auto max-w-5xl px-8 py-6">
      {share && <ShareDialog meetingId={m.id} title={m.title} durationMs={m.duration_ms} hasVideo={!!detail.media.video} currentMs={currentMs} onClose={() => setShare(false)} />}
      <Link to="/" className="focus-ring inline-flex items-center gap-1.5 rounded text-[13px] text-ink-3 hover:text-ink">
        <ArrowLeft size={14} /> Library
      </Link>

      <header className="mt-3 mb-6 flex items-start justify-between gap-6">
        <div className="min-w-0 flex-1">
          {editing ? (
            <form
              className="flex items-center gap-2"
              onSubmit={(e) => {
                e.preventDefault();
                void saveTitle();
              }}
            >
              <Input autoFocus value={title} onChange={(e) => setTitle(e.target.value)} className="h-10 max-w-lg font-serif text-[22px]" />
              <Button variant="primary" size="sm" type="submit">
                <Check size={14} /> Save
              </Button>
            </form>
          ) : (
            <h1 className="group flex items-center gap-2 font-serif text-[32px] leading-tight tracking-tight">
              <span className="truncate">{m.title}</span>
              <button className="focus-ring rounded text-ink-3 opacity-0 transition-opacity group-hover:opacity-100 focus-visible:opacity-100 hover:text-ink" onClick={() => setEditing(true)} title="Rename" aria-label="Rename meeting">
                <Pencil size={16} />
              </button>
            </h1>
          )}
          <div className="mt-1 flex flex-wrap items-center gap-3 text-[13px] text-ink-3">
            <span>{appLabel(m.app)}</span>
            <span>{fmtDate(m.started_at)} at {fmtTime(m.started_at)}</span>
            <span className="font-mono">{fmtDuration(m.duration_ms)}</span>
            <Badge tone={m.status === "ready" ? "moss" : m.status === "failed" ? "ember" : "amber"}>{m.status}</Badge>
            <label className="inline-flex items-center gap-1">
              <FolderOpen size={13} />
              <select
                value={m.folder_id ?? ""}
                onChange={(e) => void cmd.moveMeeting(m.id, e.target.value || null).then(load)}
                className="focus-ring rounded border border-transparent bg-transparent text-[12px] hover:border-line-2"
              >
                <option value="">Unfiled</option>
                {folders.map((f) => <option key={f.id} value={f.id}>{f.name}</option>)}
              </select>
            </label>
          </div>
          <div className="mt-2 flex flex-wrap items-center gap-1.5">
            {tags.map((t) => (
              <span key={t.id} className="inline-flex items-center gap-1 rounded-full bg-canvas-3 px-2 py-0.5 text-[12px] text-ink-2">
                {t.name}
                <button onClick={() => void cmd.untagMeeting(m.id, t.id).then(() => cmd.meetingTags(m.id)).then(setTags)} aria-label={`Remove tag ${t.name}`} className="focus-ring rounded text-ink-3 hover:text-ember"><X size={11} /></button>
              </span>
            ))}
            <form
              className="inline-flex items-center gap-1"
              onSubmit={(e) => {
                e.preventDefault();
                const name = tagDraft.trim();
                if (!name) return;
                void cmd.tagMeeting(m.id, name).then(() => cmd.meetingTags(m.id)).then((t) => { setTags(t); setTagDraft(""); });
              }}
            >
              <TagIcon size={12} className="text-ink-3" />
              <input value={tagDraft} onChange={(e) => setTagDraft(e.target.value)} placeholder="add tag" className="w-20 bg-transparent text-[12px] outline-none placeholder:text-ink-3" />
            </form>
          </div>
          {m.participants.length > 0 && (
            <div className="mt-2 flex flex-wrap items-center gap-1.5 text-[12px] text-ink-3" title="Attendees seen in the meeting window or calendar event">
              <Users size={12} />
              {m.participants.map((p) => (
                <span key={p} className="rounded-full border border-line px-2 py-0.5 text-ink-2">{p}</span>
              ))}
            </div>
          )}
          {shares.length >= 2 && (
            <div className="mt-3 max-w-lg" title="Share of speaking time, from the transcript">
              <div className="flex h-1.5 w-full overflow-hidden rounded-full bg-line" role="img" aria-label={`Talk time: ${shares.map((s) => `${s.name} ${s.pct}%`).join(", ")}`}>
                {shares.map((s, i) => (
                  <span key={s.name} className={`block h-full ${TALK_COLORS[i % TALK_COLORS.length]}`} style={{ width: `${s.pct}%` }} />
                ))}
              </div>
              <div className="mt-1 flex flex-wrap gap-x-3 gap-y-0.5 text-[11px] text-ink-3">
                {shares.map((s, i) => (
                  <span key={s.name} className="inline-flex items-center gap-1">
                    <span className={`inline-block h-2 w-2 rounded-full ${TALK_COLORS[i % TALK_COLORS.length]}`} />
                    {s.name} {s.pct}% <span className="font-mono">({fmtDuration(s.ms)})</span>
                  </span>
                ))}
              </div>
            </div>
          )}
        </div>
        <div className="flex shrink-0 items-center gap-1">
          {flash && <span className="mr-1 text-[12px] text-moss">{flash}</span>}
          <Button variant="outline" size="sm" onClick={() => setShare(true)} disabled={m.status === "recording"}>
            <Share2 size={14} /> Share
          </Button>
          <Menu
            label={<><Download size={14} /> Export</>}
            disabled={!hasText}
            items={[
              { label: <><Copy size={13} /> Copy transcript</>, onSelect: () => void copyText(transcriptTxt(detail.segments, useClean), "Transcript") },
              { label: <><Copy size={13} /> Copy summary</>, onSelect: () => void copyText(summary?.markdown ?? "", "Summary"), disabled: !summary },
              { label: "Save transcript as .txt", onSelect: () => void saveText("txt", transcriptTxt(detail.segments, useClean), "Transcript") },
              { label: "Save transcript as .srt (subtitles)", onSelect: () => void saveText("srt", transcriptSrt(detail.segments, useClean), "Subtitles") },
              { label: "Save notes + transcript as .md", onSelect: () => void saveText("md", transcriptMd(m.title, dateLine, detail.segments, useClean, summary?.markdown), "Markdown") },
            ]}
          />
          <Menu
            label={<><RefreshCw size={14} /> Re-run</>}
            disabled={processing || m.status === "recording"}
            items={[
              { label: "Everything (transcribe, speakers, clean up, summary)", onSelect: () => void cmd.retranscribe(m.id) },
              ...asrModels.map((am) => ({ label: `Transcribe with ${am.spec.name}`, onSelect: () => void cmd.retranscribe(m.id, am.spec.id) })),
              { label: "Speaker detection only", onSelect: () => void cmd.rediarize(m.id), disabled: detail.segments.length === 0 },
              { label: "AI clean-up only", onSelect: () => void cmd.rerunCleanup(m.id), disabled: detail.segments.length === 0 },
              { label: "Search index (embeddings)", onSelect: () => void cmd.reembed(m.id), disabled: detail.segments.length === 0 },
              { label: "Summary", onSelect: () => { setTab("summary"); void cmd.generateSummary(m.id, templateId || undefined); }, disabled: detail.segments.length === 0 },
            ]}
          />
          <Button variant="danger" size="sm" onClick={() => void remove()}>
            <Trash2 size={14} /> Delete
          </Button>
        </div>
      </header>

      {m.error?.startsWith("no-llm:") && !processing && (
        <div className="mb-5 flex items-center gap-3 rounded-lg border border-line bg-canvas-2 px-4 py-2.5 text-[13px] text-ink-2">
          <Spinner className="h-3.5 w-3.5" /> Clean-up and summary will run automatically once the language model finishes downloading. <Link to="/settings" className="underline">Models</Link>
        </div>
      )}
      {(m.status === "failed" || (m.error && !m.error.startsWith("no-llm:"))) && !processing && (
        <div className="mb-5 flex items-start gap-3 rounded-lg border border-ember/30 bg-ember-soft/40 px-4 py-3 text-[13px]">
          <AlertTriangle size={16} className="mt-0.5 shrink-0 text-ember" />
          <div className="min-w-0 flex-1">
            <div className="font-medium text-ink">{m.status === "failed" ? "Processing failed" : "One step had a problem"}</div>
            <div className="mt-0.5 break-words text-ink-2">{m.error ?? "Unknown error"}</div>
            <div className="mt-1 text-[12px] text-ink-3">Logs are in the Hark data folder under <code>logs/</code>. Missing models can be downloaded in Settings.</div>
          </div>
          <Button variant="outline" size="sm" onClick={() => void cmd.retranscribe(m.id)}>
            <RefreshCw size={14} /> Retry
          </Button>
        </div>
      )}
      {processing && (
        <div className="mb-5 flex items-center gap-3 rounded-lg border border-line bg-canvas-2 px-4 py-2.5 text-[13px] text-ink-2">
          <Spinner className="h-3.5 w-3.5" />
          <span>{STAGE_LABEL[stage ?? ""] ?? stage ?? "Processing"}</span>
          <span className="ml-auto h-1.5 w-40 overflow-hidden rounded-full bg-line"><span className="block h-full bg-ink transition-[width]" style={{ width: `${Math.round(progress * 100)}%` }} /></span>
        </div>
      )}

      <div className="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)] gap-8">
        <div className="sticky top-6 self-start">
          {recording ? (
            <div className="rounded-lg border border-line bg-canvas-2 p-5">
              <div className="inline-flex items-center gap-2 text-[13px] font-medium text-ember"><span className="h-2 w-2 animate-pulse rounded-full bg-ember" /> Recording in progress</div>
              <p className="mt-2 text-[13px] text-ink-2">Captions and notes update as people talk. The full transcript with speaker names, the summary and Ask Hark arrive once you stop.</p>
              {stopping ? (
                <div className="mt-4 inline-flex items-center gap-2 text-[13px] text-ink-2"><Spinner className="h-3.5 w-3.5" /> Finishing the recording...</div>
              ) : (
                <Button variant="ember" size="sm" className="mt-4" onClick={() => { setStopping(true); void cmd.stopRecording().catch((e) => { setStopping(false); alert(String(e)); }); }}>Stop recording</Button>
              )}
            </div>
          ) : (
            <Player ref={player} audio={detail.media.audio} video={detail.media.video} highlights={detail.highlights} onTime={onTime} />
          )}
          {!recording && detail.speaker_stats.length > 0 && (
            <SpeakersPanel
              className="mt-6"
              stats={detail.speaker_stats}
              speakers={speakers}
              nameOptions={nameOptions}
              onRename={renameSpeaker}
              onAcceptSuggestion={acceptSuggestion}
              onDismissSuggestion={(label) => setDismissed((d) => [...d, label])}
            />
          )}
          {detail.highlights.length > 0 && (
            <div className="mt-6">
              <SectionTitle>Highlights</SectionTitle>
              <div className="flex flex-wrap gap-2">
                {detail.highlights.map((h) => (
                  <button key={h} onClick={() => player.current?.seek(Math.max(0, h - 5000))} className="focus-ring rounded-full bg-amber-soft px-2.5 py-1 font-mono text-[12px] text-ink hover:bg-amber/40">
                    {fmtDuration(h)}
                  </button>
                ))}
              </div>
            </div>
          )}
        </div>

        <div className="flex min-h-[60vh] flex-col">
          <div className="mb-3 flex items-center justify-between">
            <div className="inline-flex rounded-md bg-canvas-3 p-0.5 text-[12px] font-medium">
              {(["transcript", "summary", "chat"] as const).map((t) => (
                <button key={t} onClick={() => setTab(t)} className={`rounded px-3 py-1 capitalize ${tab === t ? "bg-canvas text-ink shadow-card" : "text-ink-3 hover:text-ink"}`}>
                  {t}
                </button>
              ))}
            </div>
            {tab === "transcript" && recording && (
              <span className="inline-flex items-center gap-2 text-[12px] text-ember"><span className="h-2 w-2 animate-pulse rounded-full bg-ember" /> recording</span>
            )}
            {tab === "transcript" && !recording && (processing ? (
              <span className="inline-flex items-center gap-2 text-[12px] text-ink-3"><Spinner className="h-3 w-3" /> {stage ?? "processing"}</span>
            ) : hasClean ? (
              <span className="inline-flex rounded-md bg-canvas-3 p-0.5 text-[11px] font-medium">
                {(["clean", "raw"] as const).map((v) => (
                  <button key={v} onClick={() => setView(v)} className={`rounded px-2 py-0.5 ${useClean === (v === "clean") ? "bg-canvas text-ink shadow-card" : "text-ink-3"}`}>
                    {v === "clean" ? "Cleaned" : "Raw"}
                  </button>
                ))}
              </span>
            ) : (
              <span className="text-[12px] text-ink-3">{detail.segments.length} segments</span>
            ))}
          </div>

          {tab === "transcript" && recording && <LiveTranscript finals={live?.finals ?? []} partial={live?.partial ?? null} />}
          {tab === "summary" && recording && <LiveNotes notes={live?.notes ?? ""} updatedMs={live?.notes_updated_ms ?? null} />}
          {tab === "chat" && recording && (
            <p className="text-sm text-ink-3">Ask Hark opens once the recording stops and the transcript is ready.</p>
          )}

          {tab === "transcript" && !recording && (
            detail.segments.length === 0 && processing ? (
              <p className="text-sm text-ink-3">Transcribing... this usually takes well under a minute.</p>
            ) : detail.segments.length === 0 ? (
              <p className="text-sm text-ink-3">
                No transcript. Download a speech model in <Link to="/settings" className="underline">Settings</Link>, then Re-transcribe.
              </p>
            ) : (
              <Transcript
                segments={detail.segments}
                currentMs={currentMs}
                onSeek={(ms) => player.current?.seek(ms)}
                useClean={useClean}
                speakers={speakers}
                nameOptions={nameOptions}
                onRenameSpeaker={renameSpeaker}
                onAcceptSuggestion={acceptSuggestion}
                onDismissSuggestion={(label) => setDismissed((d) => [...d, label])}
              />
            )
          )}

          {tab === "summary" && !recording && (
            <div>
              <div className="mb-4 flex items-center gap-2">
                <select
                  value={templateId || summary?.template_id || "general"}
                  onChange={(e) => setTemplateId(e.target.value)}
                  className="focus-ring h-8 rounded-md border border-line-2 bg-canvas px-2 text-[13px]"
                >
                  {templates.map((t) => (
                    <option key={t.id} value={t.id}>{t.name}</option>
                  ))}
                </select>
                <Button variant="outline" size="sm" disabled={stage === "summary" || detail.segments.length === 0} onClick={() => { setStage("summary"); void cmd.generateSummary(m.id, templateId || undefined).catch((e) => { setStage(null); alert(String(e)); }); }}>
                  <Sparkles size={14} /> {summary ? "Regenerate" : "Generate"}
                </Button>
                {stage === "summary" && <span className="inline-flex items-center gap-2 text-[12px] text-ink-3"><Spinner className="h-3 w-3" /> writing...</span>}
                <Button variant="outline" size="sm" disabled={followupBusy || !hasText} onClick={() => void draftFollowup()} title="Draft a follow-up email from these notes with the local AI">
                  {followupBusy ? <Spinner className="h-3.5 w-3.5" /> : <Mail size={14} />} Follow-up email
                </Button>
                {summary && <span className="ml-auto text-[11px] text-ink-3">{summary.model}</span>}
              </div>
              {followup !== null && (
                <div className="mb-5 rounded-lg border border-line bg-canvas-2 p-3">
                  <div className="mb-2 flex items-center gap-2 text-[12px] text-ink-3">
                    <Mail size={13} /> Follow-up email draft - edit freely, then copy it into your mail app.
                    <div className="ml-auto flex items-center gap-1">
                      <Button variant="outline" size="sm" onClick={() => void copyText(followup, "Email")}><Copy size={13} /> Copy</Button>
                      <Button variant="outline" size="sm" disabled={followupBusy} onClick={() => void draftFollowup()}><RefreshCw size={13} /> Redo</Button>
                      <Button variant="outline" size="sm" onClick={() => setFollowup(null)} aria-label="Close draft"><X size={13} /></Button>
                    </div>
                  </div>
                  <textarea
                    value={followup}
                    onChange={(e) => setFollowup(e.target.value)}
                    rows={Math.min(18, Math.max(6, followup.split("\n").length + 1))}
                    className="focus-ring w-full resize-y rounded-md border border-line-2 bg-canvas p-3 font-sans text-[13px] leading-relaxed text-ink"
                    aria-label="Follow-up email draft"
                  />
                </div>
              )}
              {summary ? (
                <Markdown text={summary.markdown} onSeek={(ms) => player.current?.seek(ms)} />
              ) : (
                <p className="text-sm text-ink-3">No summary yet. Pick a template and generate one.</p>
              )}
            </div>
          )}

          {tab === "chat" && !recording && (
            <ChatPanel scopeKind="meeting" scopeId={m.id} onSeek={(ms) => player.current?.seek(ms)} className="min-h-[60vh] flex-1" />
          )}
        </div>
      </div>
    </div>
  );
}
