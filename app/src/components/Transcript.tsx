import { useEffect, useMemo, useRef } from "react";
import { cn } from "@/lib/cn";
import { fmtStamp } from "@/lib/format";
import type { Segment } from "@/lib/ipc";

export interface TranscriptProps {
  segments: Pick<Segment, "id" | "start_ms" | "end_ms" | "speaker" | "text" | "clean_text">[];
  currentMs: number;
  onSeek: (ms: number) => void;
  useClean?: boolean;
  autoScroll?: boolean;
  className?: string;
}

const SPEAKER_COLORS = ["text-ember", "text-moss", "text-amber", "text-ink-2"];

export function Transcript({ segments, currentMs, onSeek, useClean = false, autoScroll = true, className }: TranscriptProps) {
  const activeIdx = useMemo(() => {
    let idx = -1;
    for (let i = 0; i < segments.length; i++) {
      if (segments[i].start_ms <= currentMs) idx = i;
      else break;
    }
    if (idx >= 0 && currentMs > segments[idx].end_ms + 1500) return -1;
    return idx;
  }, [segments, currentMs]);

  const speakerIndex = useMemo(() => {
    const map = new Map<string, number>();
    for (const s of segments) {
      if (s.speaker && !map.has(s.speaker)) map.set(s.speaker, map.size);
    }
    return map;
  }, [segments]);

  const activeRef = useRef<HTMLButtonElement | null>(null);
  useEffect(() => {
    if (autoScroll && activeRef.current) {
      activeRef.current.scrollIntoView({ block: "center", behavior: "smooth" });
    }
  }, [activeIdx, autoScroll]);

  if (segments.length === 0) {
    return <p className={cn("text-sm text-ink-3", className)}>No transcript yet.</p>;
  }

  let lastSpeaker: string | null | undefined = undefined;
  return (
    <div className={cn("flex flex-col gap-1", className)} data-testid="transcript">
      {segments.map((s, i) => {
        const showSpeaker = s.speaker !== lastSpeaker;
        lastSpeaker = s.speaker;
        const active = i === activeIdx;
        const color = s.speaker ? SPEAKER_COLORS[(speakerIndex.get(s.speaker) ?? 0) % SPEAKER_COLORS.length] : "text-ink-3";
        const text = useClean && s.clean_text ? s.clean_text : s.text;
        return (
          <div key={s.id} className={cn(showSpeaker && i > 0 && "mt-3")}>
            {showSpeaker && s.speaker && <div className={cn("mb-0.5 text-[12px] font-semibold", color)}>{s.speaker}</div>}
            <button
              type="button"
              ref={active ? activeRef : null}
              onClick={() => onSeek(s.start_ms)}
              data-active={active || undefined}
              className={cn(
                "focus-ring group flex w-full cursor-pointer gap-3 rounded-md px-2 py-1 text-left transition-colors",
                active ? "bg-ember-soft" : "hover:bg-canvas-2",
              )}
            >
              <span className={cn("mt-0.5 w-12 shrink-0 font-mono text-[11px] tabular-nums", active ? "text-ember" : "text-ink-3")}>
                {fmtStamp(s.start_ms)}
              </span>
              <span className={cn("text-[14.5px] leading-6", active ? "text-ink" : "text-ink/90")}>{text}</span>
            </button>
          </div>
        );
      })}
    </div>
  );
}
