// Share of speaking time per speaker, from the transcript segments. Pure, unit-tested.
import type { Segment } from "./ipc";

export interface TalkShare {
  name: string;
  ms: number;
  pct: number;
}

/** Speaking time per labelled speaker, most talkative first. Unlabelled segments are ignored. */
export function talkTime(segments: Segment[]): TalkShare[] {
  const by = new Map<string, number>();
  for (const s of segments) {
    if (!s.speaker) continue;
    by.set(s.speaker, (by.get(s.speaker) ?? 0) + Math.max(0, s.end_ms - s.start_ms));
  }
  const total = Array.from(by.values()).reduce((a, b) => a + b, 0);
  if (total === 0) return [];
  return Array.from(by, ([name, ms]) => ({ name, ms, pct: Math.round((ms / total) * 100) })).sort((a, b) => b.ms - a.ms);
}
