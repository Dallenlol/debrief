import { open, save } from "@tauri-apps/plugin-dialog";
import { Check, Copy, Link2, Scissors, X } from "lucide-react";
import { useEffect, useState } from "react";
import { fmtDuration } from "@/lib/format";
import { cmd, type ShareInfo } from "@/lib/ipc";
import { Button, Input, Toggle } from "./ui";

interface ShareDialogProps {
  meetingId: string;
  title: string;
  durationMs: number;
  hasVideo: boolean;
  /** Current player position, used as the default clip start. */
  currentMs: number;
  onClose: () => void;
}

const fmtInput = (ms: number) => fmtDuration(ms);
const parseInput = (s: string): number | null => {
  const parts = s.trim().split(":").map(Number);
  if (parts.some(Number.isNaN)) return null;
  if (parts.length === 2) return (parts[0] * 60 + parts[1]) * 1000;
  if (parts.length === 3) return (parts[0] * 3600 + parts[1] * 60 + parts[2]) * 1000;
  return null;
};

/** Share links (LAN), clip export, bundle export and standalone HTML. */
export function ShareDialog({ meetingId, title, durationMs, hasVideo, currentMs, onClose }: ShareDialogProps) {
  const [shares, setShares] = useState<ShareInfo[]>([]);
  const [busy, setBusy] = useState<string | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const [copied, setCopied] = useState<string | null>(null);
  const [clipStart, setClipStart] = useState(fmtInput(Math.max(0, currentMs - 15000)));
  const [clipEnd, setClipEnd] = useState(fmtInput(Math.min(durationMs, currentMs + 30000)));
  const [includeVideo, setIncludeVideo] = useState(true);

  const reload = () => void cmd.listShares().then((all) => setShares(all.filter((s) => s.share.meeting_id === meetingId)));
  useEffect(() => reload(), [meetingId]);

  const run = async (label: string, fn: () => Promise<string | void>) => {
    setBusy(label);
    setMsg(null);
    try {
      const r = await fn();
      if (r) setMsg(r);
      reload();
    } catch (e) {
      setMsg(String(e));
    } finally {
      setBusy(null);
    }
  };

  const copy = async (text: string) => {
    await navigator.clipboard.writeText(text);
    setCopied(text);
    setTimeout(() => setCopied(null), 1500);
  };

  const clipRange = (): [number, number] | null => {
    const a = parseInput(clipStart);
    const b = parseInput(clipEnd);
    if (a == null || b == null || b <= a) {
      setMsg("Enter a clip range like 1:05 to 2:30.");
      return null;
    }
    return [a, Math.min(b, durationMs || b)];
  };

  return (
    <div className="fixed inset-0 z-40 flex items-center justify-center bg-ink/30 p-6" onClick={onClose}>
      <div className="rise w-full max-w-lg rounded-xl border border-line bg-canvas p-5 shadow-float" onClick={(e) => e.stopPropagation()}>
        <div className="mb-4 flex items-start justify-between">
          <div>
            <h2 className="font-serif text-[22px] leading-tight">Share or export</h2>
            <p className="text-[12px] text-ink-3">Links only work on your network while Hark is open. Files work anywhere.</p>
          </div>
          <button onClick={onClose} aria-label="Close" className="focus-ring rounded text-ink-3 hover:text-ink"><X size={18} /></button>
        </div>

        <section className="mb-5">
          <h3 className="mb-2 text-[11px] font-semibold tracking-[0.12em] text-ink-3 uppercase">Link on this network</h3>
          <div className="flex gap-2">
            <Button variant="primary" size="sm" disabled={busy !== null} onClick={() => void run("link", async () => { await cmd.createShare(meetingId); })}>
              <Link2 size={14} /> Share whole meeting
            </Button>
          </div>
          {shares.length > 0 && (
            <ul className="mt-3 flex flex-col gap-1.5">
              {shares.map((s) => (
                <li key={s.share.token} className="flex items-center gap-2 rounded-md border border-line px-2 py-1.5 text-[12px]">
                  <span className="shrink-0 rounded bg-canvas-3 px-1.5 py-px text-[10px] font-semibold uppercase">{s.share.kind}</span>
                  <span className="min-w-0 flex-1 truncate font-mono text-ink-2">{s.url}</span>
                  {s.share.kind === "clip" && <span className="shrink-0 text-ink-3">{fmtDuration(s.share.start_ms ?? 0)}-{fmtDuration(s.share.end_ms ?? 0)}</span>}
                  <button onClick={() => void copy(s.url)} className="text-ink-3 hover:text-ink" title="Copy link">{copied === s.url ? <Check size={14} className="text-moss" /> : <Copy size={14} />}</button>
                  <Toggle checked={s.share.enabled} onChange={(v) => void cmd.setShareEnabled(s.share.token, v).then(reload)} label="" />
                  <button onClick={() => void cmd.deleteShare(s.share.token).then(reload)} className="text-ink-3 hover:text-ember" title="Delete link"><X size={14} /></button>
                </li>
              ))}
            </ul>
          )}
        </section>

        <section className="mb-5">
          <h3 className="mb-2 text-[11px] font-semibold tracking-[0.12em] text-ink-3 uppercase">Clip</h3>
          <div className="flex items-center gap-2">
            <Input value={clipStart} onChange={(e) => setClipStart(e.target.value)} className="w-24 font-mono" aria-label="Clip start" />
            <span className="text-ink-3">to</span>
            <Input value={clipEnd} onChange={(e) => setClipEnd(e.target.value)} className="w-24 font-mono" aria-label="Clip end" />
            <Button variant="outline" size="sm" disabled={busy !== null} onClick={() => { const r = clipRange(); if (r) void run("cliplink", async () => { await cmd.createShare(meetingId, r[0], r[1]); }); }}>
              <Link2 size={14} /> Link
            </Button>
            <Button
              variant="outline"
              size="sm"
              disabled={busy !== null}
              onClick={() => {
                const r = clipRange();
                if (!r) return;
                void run("clipfile", async () => {
                  const ext = hasVideo ? "mp4" : "mp3";
                  const out = await save({ defaultPath: `${safe(title)}-clip.${ext}`, filters: [{ name: ext.toUpperCase(), extensions: [ext] }] });
                  if (!out) return;
                  await cmd.exportClip(meetingId, r[0], r[1], out);
                  return `Saved ${out}`;
                });
              }}
            >
              <Scissors size={14} /> Save file
            </Button>
          </div>
        </section>

        <section>
          <h3 className="mb-2 text-[11px] font-semibold tracking-[0.12em] text-ink-3 uppercase">Export</h3>
          <div className="flex flex-wrap items-center gap-2">
            <Button variant="outline" size="sm" disabled={busy !== null} onClick={() => void run("bundle", async () => {
              const out = await save({ defaultPath: `${safe(title)}.hark`, filters: [{ name: "Hark bundle", extensions: ["hark"] }] });
              if (!out) return;
              await cmd.exportMeetings([meetingId], out, includeVideo);
              return `Saved ${out}. Import it on another computer from Settings > Data.`;
            })}>
              .hark bundle
            </Button>
            <Button variant="outline" size="sm" disabled={busy !== null} onClick={() => void run("html", async () => {
              const dir = await open({ directory: true, title: "Choose a folder for the web page" });
              if (!dir) return;
              const page = await cmd.exportHtml(meetingId, dir as string);
              return `Saved ${page}`;
            })}>
              Web page (HTML)
            </Button>
            {hasVideo && (
              <label className="ml-auto flex items-center gap-2 text-[12px] text-ink-2">
                <input type="checkbox" checked={includeVideo} onChange={(e) => setIncludeVideo(e.target.checked)} /> include video in bundle
              </label>
            )}
          </div>
        </section>

        {(busy || msg) && <div className="mt-4 text-[12px] text-ink-2">{busy ? "Working..." : msg}</div>}
      </div>
    </div>
  );
}

function safe(s: string) {
  return s.replace(/[^\w\- ]+/g, "").trim().replace(/\s+/g, "-").slice(0, 60) || "meeting";
}
