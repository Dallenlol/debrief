import { Fragment, type ReactNode } from "react";
import { cn } from "@/lib/cn";

/** Parse `m:ss` / `h:mm:ss` to ms; null if not a timestamp. */
export function stampToMs(s: string): number | null {
  const parts = s.trim().split(":").map((p) => (/^\d+$/.test(p.trim()) ? Number(p) : NaN));
  if (parts.some(Number.isNaN)) return null;
  if (parts.length === 2) return (parts[0] * 60 + parts[1]) * 1000;
  if (parts.length === 3) return (parts[0] * 3600 + parts[1] * 60 + parts[2]) * 1000;
  return null;
}

/** Inline: **bold**, `code`, and [m:ss] / [m:ss @ Title] citations -> buttons. */
export function renderInline(text: string, onSeek?: (ms: number, label: string) => void): ReactNode[] {
  const out: ReactNode[] = [];
  const re = /(\*\*[^*]+\*\*|`[^`]+`|\[[0-9:]+(?:\s*@[^\]]+)?\])/g;
  let last = 0;
  let m: RegExpExecArray | null;
  let k = 0;
  while ((m = re.exec(text)) !== null) {
    if (m.index > last) out.push(<Fragment key={k++}>{text.slice(last, m.index)}</Fragment>);
    const tok = m[0];
    if (tok.startsWith("**")) out.push(<strong key={k++}>{tok.slice(2, -2)}</strong>);
    else if (tok.startsWith("`")) out.push(<code key={k++} className="rounded bg-canvas-3 px-1 font-mono text-[12px]">{tok.slice(1, -1)}</code>);
    else {
      const inner = tok.slice(1, -1);
      const stamp = inner.split("@")[0].trim();
      const ms = stampToMs(stamp);
      if (ms != null && onSeek) {
        out.push(
          <button
            key={k++}
            type="button"
            onClick={() => onSeek(ms, inner)}
            className="mx-0.5 inline-flex items-center rounded bg-ember-soft px-1.5 font-mono text-[11px] font-medium text-ember hover:bg-ember/20"
            title={inner}
          >
            {stamp}
          </button>,
        );
      } else out.push(<Fragment key={k++}>{tok}</Fragment>);
    }
    last = m.index + tok.length;
  }
  if (last < text.length) out.push(<Fragment key={k++}>{text.slice(last)}</Fragment>);
  return out;
}

/** Tiny markdown renderer: #/##/### headings, bullet lists (incl. [ ] tasks), paragraphs. */
export function Markdown({ text, onSeek, className }: { text: string; onSeek?: (ms: number, label: string) => void; className?: string }) {
  const lines = text.replace(/\r/g, "").split("\n");
  const blocks: ReactNode[] = [];
  let list: ReactNode[] = [];
  let k = 0;
  const flushList = () => {
    if (list.length) {
      blocks.push(<ul key={k++} className="my-1.5 flex flex-col gap-1 pl-1">{list}</ul>);
      list = [];
    }
  };
  for (const raw of lines) {
    const line = raw.trimEnd();
    const h = /^(#{1,3})\s+(.*)$/.exec(line);
    const li = /^\s*[-*+]\s+(.*)$/.exec(line);
    if (h) {
      flushList();
      const level = h[1].length;
      blocks.push(
        <h3 key={k++} className={cn("mt-4 mb-1 font-semibold text-ink first:mt-0", level === 1 ? "text-[16px]" : "text-[11px] tracking-[0.12em] text-ink-3 uppercase")}>
          {h[2].replace(/:$/, "")}
        </h3>,
      );
    } else if (li) {
      let body = li[1];
      let task: boolean | null = null;
      if (/^\[ \]\s*/.test(body)) {
        task = false;
        body = body.replace(/^\[ \]\s*/, "");
      } else if (/^\[x\]\s*/i.test(body)) {
        task = true;
        body = body.replace(/^\[x\]\s*/i, "");
      }
      list.push(
        <li key={k++} className="flex gap-2 text-[14px] leading-6 text-ink/90">
          <span className="mt-[9px] shrink-0">
            {task === null ? <span className="block h-1.5 w-1.5 rounded-full bg-line-2" /> : <span className={cn("block h-3 w-3 rounded-[3px] border", task ? "border-moss bg-moss" : "border-line-2")} />}
          </span>
          <span>{renderInline(body, onSeek)}</span>
        </li>,
      );
    } else if (line.trim() === "") {
      flushList();
    } else {
      flushList();
      blocks.push(<p key={k++} className="my-1 text-[14px] leading-6 text-ink/90">{renderInline(line, onSeek)}</p>);
    }
  }
  flushList();
  return <div className={cn("select-text", className)}>{blocks}</div>;
}
