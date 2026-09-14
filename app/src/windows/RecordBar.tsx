import { Bookmark, Pause, Play, Square } from "lucide-react";
import { useEffect, useState } from "react";
import { Meter } from "@/components/ui";
import { cn } from "@/lib/cn";
import { fmtDuration } from "@/lib/format";
import { cmd, subscribe, type Events, type RecordingState } from "@/lib/ipc";

/** Slim always-on-top bar shown while recording. */
export function RecordBar() {
  const [rec, setRec] = useState<RecordingState>({ state: "idle", meeting_id: null, elapsed_ms: 0 });
  const [levels, setLevels] = useState<Events["levels"]>({ mic_db: -100, sys_db: -100 });
  const [captions, setCaptions] = useState<Events["caption"][]>([]);
  const [marked, setMarked] = useState(false);

  useEffect(() => {
    void cmd.recordingStatus().then(setRec);
    const a = subscribe("recording_state", (s) => {
      setRec(s);
      if (s.state === "idle") setCaptions([]);
    });
    const b = subscribe("levels", setLevels);
    const c = subscribe("caption", (cap) => setCaptions((xs) => [...xs.slice(-1), cap]));
    const t = setInterval(() => void cmd.recordingStatus().then(setRec), 1000);
    return () => {
      a();
      b();
      c();
      clearInterval(t);
    };
  }, []);

  const paused = rec.state === "paused";

  const mark = async () => {
    await cmd.markHighlight();
    setMarked(true);
    setTimeout(() => setMarked(false), 900);
  };

  return (
    <div className="flex h-full items-center gap-3 rounded-xl border border-line bg-canvas/95 px-3 shadow-float backdrop-blur" data-tauri-drag-region>
      <div className="flex w-[84px] shrink-0 items-center gap-2" data-tauri-drag-region>
        <span className={cn("h-2.5 w-2.5 rounded-full", paused ? "bg-amber" : "rec-dot bg-ember")} />
        <span className="font-mono text-[13px] font-medium tabular-nums">{fmtDuration(rec.elapsed_ms)}</span>
      </div>
      <div className="flex w-[70px] shrink-0 flex-col gap-1">
        <Meter db={levels.mic_db} />
        <Meter db={levels.sys_db} />
      </div>
      <div className="min-w-0 flex-1 text-[12px] leading-4 text-ink-2" data-tauri-drag-region>
        {captions.length === 0 ? (
          <span className="text-ink-3">{paused ? "Paused" : "Listening..."}</span>
        ) : (
          captions.map((c, i) => (
            <div key={`${c.start_ms}-${i}`} className={cn("truncate", i === captions.length - 1 ? "text-ink" : "text-ink-3")}>
              {c.text}
            </div>
          ))
        )}
      </div>
      <div className="flex shrink-0 items-center gap-1">
        <IconBtn title="Mark highlight" onClick={() => void mark()} className={marked ? "text-amber" : ""}>
          <Bookmark size={15} fill={marked ? "currentColor" : "none"} />
        </IconBtn>
        <IconBtn title={paused ? "Resume" : "Pause"} onClick={() => void (paused ? cmd.resumeRecording() : cmd.pauseRecording())}>
          {paused ? <Play size={15} /> : <Pause size={15} />}
        </IconBtn>
        <button
          onClick={() => void cmd.stopRecording()}
          className="focus-ring ml-1 inline-flex h-8 items-center gap-1.5 rounded-md bg-ember px-3 text-[12px] font-semibold text-white hover:bg-ember-2"
        >
          <Square size={11} fill="currentColor" /> Stop
        </button>
      </div>
    </div>
  );
}

function IconBtn({ children, className, ...props }: React.ButtonHTMLAttributes<HTMLButtonElement>) {
  return (
    <button className={cn("focus-ring grid h-8 w-8 place-items-center rounded-md text-ink-2 hover:bg-canvas-3 hover:text-ink", className)} {...props}>
      {children}
    </button>
  );
}
