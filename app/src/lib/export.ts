// Plain-text renderings of a transcript for Copy / Save. Pure functions, unit-tested.
import type { Segment } from "./ipc";

/** Text of a segment: the cleaned version when asked for and present. */
export function segmentText(s: Segment, useClean: boolean): string {
  return (useClean && s.clean_text ? s.clean_text : s.text).trim();
}

function stamp(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}` : `${m}:${String(s).padStart(2, "0")}`;
}

/** `[m:ss] Speaker: text` per line. */
export function transcriptTxt(segments: Segment[], useClean: boolean): string {
  return segments
    .map((s) => {
      const text = segmentText(s, useClean);
      return `[${stamp(s.start_ms)}] ${s.speaker ? `${s.speaker}: ` : ""}${text}`;
    })
    .join("\n");
}

/** Markdown with a title line, the date and the transcript; the summary goes first when given. */
export function transcriptMd(title: string, date: string, segments: Segment[], useClean: boolean, summary?: string | null): string {
  const parts = [`# ${title}`, "", date, ""];
  if (summary && summary.trim()) parts.push(summary.trim(), "", "---", "");
  parts.push("## Transcript", "");
  for (const s of segments) {
    parts.push(`**${stamp(s.start_ms)}**${s.speaker ? ` **${s.speaker}:**` : ""} ${segmentText(s, useClean)}`, "");
  }
  return parts.join("\n").trimEnd() + "\n";
}

function srtStamp(ms: number): string {
  const total = Math.max(0, Math.floor(ms));
  const h = Math.floor(total / 3_600_000);
  const m = Math.floor((total % 3_600_000) / 60_000);
  const s = Math.floor((total % 60_000) / 1000);
  const f = total % 1000;
  return `${String(h).padStart(2, "0")}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")},${String(f).padStart(3, "0")}`;
}

/** SubRip captions; speaker names are kept as a prefix so players show who spoke. */
export function transcriptSrt(segments: Segment[], useClean: boolean): string {
  return segments
    .map((s, i) => {
      const end = Math.max(s.end_ms, s.start_ms + 500);
      return `${i + 1}\n${srtStamp(s.start_ms)} --> ${srtStamp(end)}\n${s.speaker ? `${s.speaker}: ` : ""}${segmentText(s, useClean)}\n`;
    })
    .join("\n");
}

/** A file name that is safe on every OS. */
export function safeFileName(title: string): string {
  const cleaned = title.replace(/[<>:"/\\|?*]/g, " ").replace(/\s+/g, " ").trim();
  return (cleaned || "meeting").slice(0, 80);
}
