import { describe, expect, it } from "vitest";
import { dbToLevel, fmtBytes, fmtDuration } from "./format";

describe("fmtDuration", () => {
  it("formats minutes and seconds", () => {
    expect(fmtDuration(65000)).toBe("1:05");
    expect(fmtDuration(0)).toBe("0:00");
    expect(fmtDuration(59999)).toBe("0:59");
  });
  it("formats hours", () => {
    expect(fmtDuration(3725000)).toBe("1:02:05");
  });
});

describe("fmtBytes", () => {
  it("scales units", () => {
    expect(fmtBytes(500)).toBe("500 B");
    expect(fmtBytes(1536)).toBe("1.5 KB");
    expect(fmtBytes(2_497_280_256)).toBe("2.3 GB");
  });
});

describe("dbToLevel", () => {
  it("maps silence to 0 and full scale to 1", () => {
    expect(dbToLevel(-100)).toBe(0);
    expect(dbToLevel(0)).toBe(1);
    expect(dbToLevel(-30)).toBeGreaterThan(0.2);
  });
});
