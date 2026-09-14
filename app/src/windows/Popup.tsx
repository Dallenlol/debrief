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

  const keyOf = (t: VideoTarget) => (t.kind === "monitor" ? `m:${t.index}` : `w:${t.title}`);

  useEffect(() => {
    void cmd.getSettings().then((s) => setVideo(s.video_enabled));
    return subscribe("detection", (d) => {
      setDet(d);
      setLeft(30);
      void cmd.listVideoSources(d.app === "unknown" ? null : d.app).then((s) => {
        setSources(s);
        setTarget(s.find((x) => x.is_meeting)?.target ?? s[0]?.target ?? null);
      });
    });
  }, []);

  // Auto-hide countdown.
  useEffect(() => {
    if (!det) return;
    const t = setInterval(() => setLeft((n) => n - 1), 1000);
    return () => clearInterval(t);
  }, [det]);
  useEffect(() => {
    if (det && left <= 0) void cmd.dismissDetection(det.app, "now");
  }, [left, det]);

  const record = async () => {
    if (!det) return;
    await cmd.startRecording({ app: det.app === "unknown" ? undefined : det.app, video, target: video && target ? target : undefined });
  };

  if (!det) return <div className="h-full" />;

  return (
    <div className="flex h-full flex-col rounded-xl border border-line bg-canvas p-4 shadow-float" data-tauri-drag-region>
      <div className="flex items-start justify-between gap-3" data-tauri-drag-region>
        <div className="min-w-0" data-tauri-drag-region>
          <div className="flex items-center gap-2 text-[14px] font-semibold text-ink">
            <span className="h-2 w-2 rounded-full bg-moss" />
            {det.label} detected
          </div>
          {det.event && <div className="mt-0.5 truncate text-[12px] font-medium text-ink-2" title="From your calendar">{det.event}</div>}
          <div className="mt-0.5 truncate text-[12px] text-ink-3" title={det.title}>{det.title}</div>
        </div>
        <button
          onClick={() => setVideo((v) => !v)}
          className={`focus-ring inline-flex h-7 items-center gap-1.5 rounded-full px-2.5 text-[11px] font-medium ${video ? "bg-ink text-canvas" : "bg-canvas-3 text-ink-2"}`}
          title="Toggle screen video"
        >
          {video ? <Video size={12} /> : <VideoOff size={12} />} {video ? "Screen on" : "Audio only"}
        </button>
      </div>
      {video && sources.length > 0 && (
        <label className="mt-2 flex items-center gap-2 text-[11px] text-ink-3">
          <span className="shrink-0">Screen</span>
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
      <div className="mt-auto flex items-center gap-2">
        <Button variant="ember" size="md" className="flex-1" onClick={() => void record()}>
          <Circle size={12} fill="currentColor" /> Record
        </Button>
        <Button variant="ghost" size="md" onClick={() => void cmd.dismissDetection(det.app, "now")}>
          Not now <span className="ml-1 font-mono text-[11px] text-ink-3">{left}</span>
        </Button>
        <Button variant="ghost" size="md" onClick={() => void cmd.dismissDetection(det.app, "never")} title="Stop asking for this app">
          Never
        </Button>
      </div>
    </div>
  );
}
