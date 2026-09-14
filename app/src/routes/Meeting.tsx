import { ArrowLeft, Check, Pencil, RefreshCw, Sparkles, Trash2 } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { Link, useNavigate, useParams, useSearchParams } from "react-router-dom";
import { Player, type PlayerHandle } from "@/components/Player";
import { Transcript } from "@/components/Transcript";
import { Badge, Button, Input, SectionTitle, Spinner } from "@/components/ui";
import { appLabel, fmtDate, fmtDuration, fmtTime } from "@/lib/format";
import { cmd, subscribe, type MeetingDetail } from "@/lib/ipc";

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
  const [view, setView] = useState<"clean" | "raw" | null>(null);
  const [dismissed, setDismissed] = useState<string[]>([]);
  const player = useRef<PlayerHandle>(null);

  const load = useCallback(() => {
    cmd.getMeeting(id).then((d) => {
      setDetail(d);
      setTitle(d.meeting.title);
      setError(null);
    }, (e) => setError(String(e)));
  }, [id]);

  useEffect(() => {
    load();
    return subscribe("processing", (p) => {
      if (p.meeting_id !== id) return;
      setStage(p.stage === "done" || p.stage === "failed" ? null : p.stage);
      if (p.stage === "done" || p.stage === "failed") load();
    });
  }, [id, load]);

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
  const hasClean = detail.segments.some((s) => s.clean_text);
  const useClean = (view ?? "clean") === "clean" && hasClean;
  const speakers = detail.speakers.filter((sp) => !dismissed.includes(sp.label));

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

  return (
    <div className="mx-auto max-w-5xl px-8 py-6">
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
              <button className="text-ink-3 opacity-0 transition-opacity group-hover:opacity-100 hover:text-ink" onClick={() => setEditing(true)} title="Rename">
                <Pencil size={16} />
              </button>
            </h1>
          )}
          <div className="mt-1 flex items-center gap-3 text-[13px] text-ink-3">
            <span>{appLabel(m.app)}</span>
            <span>{fmtDate(m.started_at)} at {fmtTime(m.started_at)}</span>
            <span className="font-mono">{fmtDuration(m.duration_ms)}</span>
            <Badge tone={m.status === "ready" ? "moss" : m.status === "failed" ? "ember" : "amber"}>{m.status}</Badge>
          </div>
        </div>
        <div className="flex shrink-0 items-center gap-1">
          <Button variant="ghost" size="sm" onClick={() => void cmd.retranscribe(m.id)} disabled={processing} title="Run transcription and speaker detection again">
            <RefreshCw size={14} /> Re-transcribe
          </Button>
          <Button variant="ghost" size="sm" onClick={() => void cmd.rerunCleanup(m.id)} disabled={processing || detail.segments.length === 0} title="Run the AI cleanup pass again">
            <Sparkles size={14} /> Clean up
          </Button>
          <Button variant="danger" size="sm" onClick={() => void remove()}>
            <Trash2 size={14} /> Delete
          </Button>
        </div>
      </header>

      <div className="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)] gap-8">
        <div className="sticky top-6 self-start">
          <Player ref={player} audio={detail.media.audio} video={detail.media.video} highlights={detail.highlights} onTime={onTime} />
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

        <div>
          <SectionTitle
            hint={
              processing ? (
                <span className="inline-flex items-center gap-2"><Spinner className="h-3 w-3" /> {stage ?? "processing"}</span>
              ) : hasClean ? (
                <span className="inline-flex rounded-md bg-canvas-3 p-0.5 text-[11px] font-medium">
                  {(["clean", "raw"] as const).map((v) => (
                    <button
                      key={v}
                      onClick={() => setView(v)}
                      className={`rounded px-2 py-0.5 capitalize ${useClean === (v === "clean") ? "bg-canvas text-ink shadow-card" : "text-ink-3"}`}
                    >
                      {v === "clean" ? "Cleaned" : "Raw"}
                    </button>
                  ))}
                </span>
              ) : (
                `${detail.segments.length} segments`
              )
            }
          >
            Transcript
          </SectionTitle>
          {detail.segments.length === 0 && processing ? (
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
              onRenameSpeaker={renameSpeaker}
              onAcceptSuggestion={acceptSuggestion}
              onDismissSuggestion={(label) => setDismissed((d) => [...d, label])}
            />
          )}
        </div>
      </div>
    </div>
  );
}
