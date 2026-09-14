import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { Transcript } from "./Transcript";

const segs = [
  { id: 1, start_ms: 0, end_ms: 1000, speaker: "Me", text: "hello there", clean_text: null },
  { id: 2, start_ms: 1000, end_ms: 2500, speaker: "Sarah", text: "hi um how are you", clean_text: "Hi, how are you?" },
  { id: 3, start_ms: 2500, end_ms: 4000, speaker: "Sarah", text: "good", clean_text: null },
];

describe("Transcript", () => {
  it("highlights the segment covering currentMs", () => {
    render(<Transcript segments={segs} currentMs={1200} onSeek={() => {}} autoScroll={false} />);
    const active = document.querySelector("[data-active]") as HTMLElement;
    expect(active).toHaveTextContent("hi um how are you");
  });

  it("calls onSeek with the segment start when clicked", () => {
    const onSeek = vi.fn();
    render(<Transcript segments={segs} currentMs={0} onSeek={onSeek} autoScroll={false} />);
    fireEvent.click(screen.getByText("good"));
    expect(onSeek).toHaveBeenCalledWith(2500);
  });

  it("shows clean text when asked and groups consecutive speakers", () => {
    render(<Transcript segments={segs} currentMs={0} onSeek={() => {}} useClean autoScroll={false} />);
    expect(screen.getByText("Hi, how are you?")).toBeInTheDocument();
    expect(screen.getAllByText("Sarah")).toHaveLength(1);
  });
});
