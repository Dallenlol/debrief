import { convertFileSrc } from "@tauri-apps/api/core";
import { Pause, Play, RotateCcw, RotateCw } from "lucide-react";
import { forwardRef, useEffect, useImperativeHandle, useRef, useState } from "react";
import { cn } from "@/lib/cn";
import { fmtDuration } from "@/lib/format";
import { Button } from "./ui";

export interface PlayerHandle {
  seek: (ms: number) => void;
}

interface PlayerProps {
  audio: string;
  video: string | null;
  highlights?: number[];
  onTime: (ms: number) => void;
  className?: string;
}

export const Player = forwardRef<PlayerHandle, PlayerProps>(function Player({ audio, video, highlights = [], onTime, className }, ref) {
  const mediaRef = useRef<HTMLVideoElement | HTMLAudioElement | null>(null);
  const [playing, setPlaying] = useState(false);
  const [time, setTime] = useState(0);
  const [duration, setDuration] = useState(0);
  const src = (video ?? audio) ? convertFileSrc(video ?? audio) : "";

  useImperativeHandle(ref, () => ({
    seek(ms) {
      const m = mediaRef.current;
      if (!m) return;
      m.currentTime = ms / 1000;
      if (m.paused) void m.play();
    },
  }));

  useEffect(() => {
    const m = mediaRef.current;
    if (!m) return;
    const tick = () => {
      setTime(m.currentTime * 1000);
      onTime(m.currentTime * 1000);
    };
    const meta = () => setDuration(isFinite(m.duration) ? m.duration * 1000 : 0);
    const onPlay = () => setPlaying(true);
    const onPause = () => setPlaying(false);
    m.addEventListener("timeupdate", tick);
    m.addEventListener("loadedmetadata", meta);
    m.addEventListener("durationchange", meta);
    m.addEventListener("play", onPlay);
    m.addEventListener("pause", onPause);
    m.addEventListener("ended", onPause);
    return () => {
      m.removeEventListener("timeupdate", tick);
      m.removeEventListener("loadedmetadata", meta);
      m.removeEventListener("durationchange", meta);
      m.removeEventListener("play", onPlay);
      m.removeEventListener("pause", onPause);
      m.removeEventListener("ended", onPause);
    };
  }, [src, onTime]);

  const toggle = () => {
    const m = mediaRef.current;
    if (!m) return;
    if (m.paused) void m.play();
    else m.pause();
  };
  const skip = (s: number) => {
    const m = mediaRef.current;
    if (m) m.currentTime = Math.max(0, m.currentTime + s);
  };
  const scrub = (e: React.MouseEvent<HTMLDivElement>) => {
    const m = mediaRef.current;
    if (!m || !duration) return;
    const rect = e.currentTarget.getBoundingClientRect();
    const frac = (e.clientX - rect.left) / rect.width;
    m.currentTime = (frac * duration) / 1000;
  };

  const pct = duration ? (time / duration) * 100 : 0;

  return (
    <div className={cn("flex flex-col gap-3", className)}>
      {video ? (
        <video ref={mediaRef as React.RefObject<HTMLVideoElement>} src={src} className="aspect-video w-full rounded-lg bg-black shadow-card" onClick={toggle} />
      ) : (
        <>
          <audio ref={mediaRef as React.RefObject<HTMLAudioElement>} src={src} />
          <div className="flex h-24 items-end justify-center gap-[3px] rounded-lg bg-canvas-2 px-4 pb-4" aria-hidden>
            {Array.from({ length: 64 }).map((_, i) => {
              const seed = Math.abs(Math.sin(i * 12.9898) * 43758.5453) % 1;
              const active = duration ? (i / 64) * duration <= time : false;
              return (
                <span
                  key={i}
                  className={cn("w-1 rounded-full transition-colors", active ? "bg-ember" : "bg-line-2")}
                  style={{ height: `${20 + seed * 60}%` }}
                />
              );
            })}
          </div>
        </>
      )}

      <div
        role="slider"
        aria-label="Playback position"
        aria-valuemin={0}
        aria-valuemax={Math.round(duration)}
        aria-valuenow={Math.round(time)}
        tabIndex={0}
        onKeyDown={(e) => {
          if (e.key === "ArrowLeft") skip(-5);
          else if (e.key === "ArrowRight") skip(5);
          else if (e.key === " ") {
            e.preventDefault();
            toggle();
          }
        }}
        className="focus-ring group relative h-2 cursor-pointer rounded-full bg-line"
        onClick={scrub}
      >
        <div className="absolute inset-y-0 left-0 rounded-full bg-ink" style={{ width: `${pct}%` }} />
        {highlights.map((h) => (
          <span
            key={h}
            title={`Highlight at ${fmtDuration(h)}`}
            className="absolute top-1/2 h-3 w-1 -translate-y-1/2 rounded-full bg-amber"
            style={{ left: `${duration ? (h / duration) * 100 : 0}%` }}
          />
        ))}
      </div>

      <div className="flex items-center gap-2">
        <Button variant="ghost" size="iconSm" onClick={() => skip(-10)} title="Back 10 s" aria-label="Back 10 seconds">
          <RotateCcw size={16} />
        </Button>
        <Button variant="primary" size="icon" onClick={toggle} title={playing ? "Pause" : "Play"} aria-label={playing ? "Pause" : "Play"}>
          {playing ? <Pause size={16} /> : <Play size={16} className="translate-x-px" />}
        </Button>
        <Button variant="ghost" size="iconSm" onClick={() => skip(10)} title="Forward 10 s" aria-label="Forward 10 seconds">
          <RotateCw size={16} />
        </Button>
        <span className="ml-2 font-mono text-[12px] tabular-nums text-ink-2">
          {fmtDuration(time)} <span className="text-ink-3">/ {fmtDuration(duration)}</span>
        </span>
      </div>
    </div>
  );
});
