import { render } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { Player } from "./Player";

vi.mock("@tauri-apps/api/core", () => ({ convertFileSrc: (p: string) => `asset://${p}` }));

describe("Player", () => {
  it("takes sound from mix.wav and mutes the screen video", () => {
    const { container } = render(<Player audio="C:/r/mix.wav" video="C:/r/screen.mp4" onTime={() => {}} />);
    expect(container.querySelector("audio")?.getAttribute("src")).toBe("asset://C:/r/mix.wav");
    const v = container.querySelector("video")!;
    expect(v.getAttribute("src")).toBe("asset://C:/r/screen.mp4");
    expect(v.muted).toBe(true);
  });
});
