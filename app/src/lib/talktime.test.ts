import { describe, expect, it } from "vitest";
import { talkTime } from "./talktime";
import type { Segment } from "./ipc";

const seg = (start: number, end: number, speaker: string | null): Segment => ({ id: start, meeting_id: "m", start_ms: start, end_ms: end, speaker, text: "", clean_text: null });

describe("talkTime", () => {
  it("sums per speaker, sorts by time and ignores unlabelled speech", () => {
    const r = talkTime([seg(0, 1000, "A"), seg(1000, 4000, "B"), seg(4000, 5000, null), seg(5000, 6000, "A")]);
    expect(r).toEqual([
      { name: "B", ms: 3000, pct: 60 },
      { name: "A", ms: 2000, pct: 40 },
    ]);
  });

  it("is empty when nobody is labelled", () => {
    expect(talkTime([seg(0, 1000, null)])).toEqual([]);
    expect(talkTime([])).toEqual([]);
  });
});
