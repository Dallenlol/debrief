import { Circle, Video, VideoOff } from "lucide-react";
import { useEffect, useState } from "react";
import { Button } from "@/components/ui";
import { cmd, subscribe, type Events, type VideoSource, type VideoTarget } from "@/lib/ipc";

/** "Call detected - Record?" window. Nothing happens unless Record is clicked. */
export function Popup() {
  const [det, setDet] = useState<Events["detection"] | null>(null);
  const [video, setVideo] = useState(true);
  const [left, setLeft] = useState(30);
  const [sources, setSources] = useState<VideoSource[]>([]);
  const [target, setTarget] = useState<VideoTarget | null>(null);
  // Audio: only the chosen window's app (Windows), or everything the computer plays.
  const [appAudio, setAppAudio] = useState(true);
  const manual = det?.app === "manual";
  const isWindows = navigator.userAgent.includes("Windows");

  const keyOf = (t: VideoTarget) => (t.kind === "monitor" ? `m:${t.index}` : `w:${t.title}`);
  const selected = sources.find((s) => target && keyOf(s.target) === keyOf(target)) ?? null;
  const canAppAudio = isWindows && selected?.pid != null;

  useEffect(() => {
    void cmd.getSettings().then((s) => setVideo(s.video_enabled));
    return subscribe("detection", (d) => {
      setDet(d);
      setLeft(30);
      void cmd.listVideoSources(d.app === "unknown" || d.app === "manual" ? null : d.app).then((s) => {
        setSources(s);
        // Prefer the meeting window, else the first window (so per-app audio works), else a display.
        setTarget(s.find((x) => x.is_meeting)?.target ?? s.find((x) => x.target.kind === "window")?.target ?? s[0]?.target ?? null);
      });
    });
  }, []);

  // Auto-hide countdown (detections only; a manual picker waits for you).
  useEffect(() => {
    if (!det || manual) return;
    const t = setInterval(() => setLeft((n) => n - 1), 1000);
    return () => clearInterval(t);
  }, [det, manual]);
  useEffect(() => {
    if (det && !manual && left <= 0) void cmd.dismissDetection(det.app, "now");
  }, [left, det, manual]);

  // Escape = "Not now"; Enter = Record.
  useEffect(() => {
    if (!det) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") void cmd.dismissDetection(det.app, "now");
      else if (e.key === "Enter" && !(e.target instanceof HTMLSelectElement)) void record();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [det, video, target]);

  const record = async () => {
    if (!det) return;
    const app = det.app === "unknown" || det.app === "manual" ? undefined : det.app;
    // The picked window always drives the audio; video only when screen capture is on.
    const audio_pid = canAppAudio && appAudio && selected?.pid != null ? selected.pid : undefined;
    try {
      await cmd.startRecording({ app, video, target: video && target ? target : undefined, audio_pid });
    } catch (e) {
      alert(String(e));
    }
  };

  if (!det) return <div className="h-full" />;

  return (
    <div role="dialog" aria-label={`${det.label} detected. Record?`} className="flex h-full flex-col rounded-xl border border-line bg-canvas p-4 shadow-float" data-tauri-drag-region>
      <div className="flex items-start justify-between gap-3" data-tauri-drag-region>
        <div className="min-w-0" data-tauri-drag-region>
          <div className="flex items-center gap-2 text-[14px] font-semibold text-ink">
            <span className={`h-2 w-2 rounded-full ${manual ? "bg-ember" : "bg-moss"}`} />
            {manual ? det.label : `${det.label} detected`}
          </div>
          {det.event && <div className="mt-0.5 truncate text-[12px] font-medium text-ink-2" title="From your calendar">{det.event}</div>}
          <div className="mt-0.5 truncate text-[12px] text-ink-3" title={det.title}>{det.title}</div>
        </div>
        <button
          aria-pressed={video}
          aria-label="Record screen video"
          onClick={() => setVideo((v) => !v)}
          className={`focus-ring inline-flex h-7 items-center gap-1.5 rounded-full px-2.5 text-[11px] font-medium ${video ? "bg-ink text-canvas" : "bg-canvas-3 text-ink-2"}`}
          title="Toggle screen video"
        >
          {video ? <Video size={12} /> : <VideoOff size={12} />} {video ? "Screen on" : "Audio only"}
        </button>
      </div>
      {sources.length > 0 && (
        <label className="mt-2 flex items-center gap-2 text-[11px] text-ink-3">
          <span className="shrink-0">{video ? "Screen" : "Window"}</span>
          <select
            aria-label="Screen to record"
            value={target ? keyOf(target) : ""}
            onChange={(e) => setTarget(sources.find((s) => keyOf(s.target) === e.target.value)?.target ?? null)}
            className="focus-ring h-6 min-w-0 flex-1 truncate rounded border border-line-2 bg-canvas px-1.5 text-[11px] text-ink"
          >
            {sources.map((s) => (
              <option key={keyOf(s.target)} value={keyOf(s.target)}>{s.target.kind === "monitor" ? "Display: " : "Window: "}{s.label}</option>
            ))}
          </select>
        </label>
      )}
      <div className="mt-1.5 flex items-center gap-3 text-[11px] text-ink-3" role="radiogroup" aria-label="Which audio to capture">
        <span className="shrink-0">Audio</span>
        <label className={`inline-flex items-center gap-1 ${canAppAudio ? "" : "opacity-50"}`} title={canAppAudio ? "Only what this app plays - no music or notifications from other apps" : isWindows ? "Pick a window to capture just that app" : "Per-app audio needs Windows; all system audio is recorded"}>
          <input type="radio" name="audio" checked={canAppAudio && appAudio} disabled={!canAppAudio} onChange={() => setAppAudio(true)} /> This app only
        </label>
        <label className="inline-flex items-center gap-1">
          <input type="radio" name="audio" checked={!canAppAudio || !appAudio} onChange={() => setAppAudio(false)} /> Everything playing
        </label>
        <span className="ml-auto">+ your mic</span>
      </div>
      <div className="mt-auto flex items-center gap-2">
        <Button variant="ember" size="md" className="flex-1" onClick={() => void record()}>
          <Circle size={12} fill="currentColor" /> Record
        </Button>
        <Button variant="ghost" size="md" onClick={() => void cmd.dismissDetection(det.app, "now")}>
          {manual ? "Cancel" : <>Not now <span className="ml-1 font-mono text-[11px] text-ink-3">{left}</span></>}
        </Button>
        {!manual && (
          <Button variant="ghost" size="md" onClick={() => void cmd.dismissDetection(det.app, "never")} title="Stop asking for this app">
            Never
          </Button>
        )}
      </div>
    </div>
  );
}
