import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { Markdown, stampToMs } from "./Markdown";

describe("Markdown", () => {
  it("parses stamps", () => {
    expect(stampToMs("1:05")).toBe(65000);
    expect(stampToMs("1:02:05")).toBe(3725000);
    expect(stampToMs("abc")).toBeNull();
  });

  it("renders headings, tasks and clickable citations", () => {
    const onSeek = vi.fn();
    render(<Markdown text={"## Action items\n- [ ] Ship it - Sam\n- [x] Done thing\n\nAgreed at [1:05] and [0:30 @ Budget]."} onSeek={onSeek} />);
    expect(screen.getByText("Action items")).toBeInTheDocument();
    expect(screen.getByText("Ship it - Sam")).toBeInTheDocument();
    fireEvent.click(screen.getByText("1:05"));
    expect(onSeek).toHaveBeenCalledWith(65000, "1:05");
    fireEvent.click(screen.getByText("0:30"));
    expect(onSeek).toHaveBeenCalledWith(30000, "0:30 @ Budget");
  });
});
