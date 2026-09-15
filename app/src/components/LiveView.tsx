// The Meeting page while its recording is still running: captions as they
// land (the provisional line refines in place) and running notes that the
// local model keeps updating.
import { Radio } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { Markdown } from "@/components/Markdown";
import { fmtDuration } from "@/lib/format";
import { cmd, subscribe, type Caption, type LiveSnapshot } from "@/lib/ipc";

export function useLiveFeed(meetingId: string, active: boolean) {
  const [feed, setFeed] = useState<LiveSnapshot | null>(null);
  useEffect(() => {
    if (!active) {
      setFeed(null);
      return;
    }
    void cmd.liveSnapshot().then((s) => setFeed(s && s.meeting_id === meetingId ? s : null));
    const a = subscribe("caption", (c) =>
      setFeed((f) => {
        if (!f) return f;
        return c.is_final ? { ...f, finals: [...f.finals, c], partial: null } : { ...f, partial: c };
      }),
    );
    const b = subscribe("live_notes", (p) => {
      if (p.meeting_id !== meetingId) return;
      setFeed((f) => (f ? { ...f, notes: p.notes, notes_updated_ms: p.updated_ms } : f));
    });
    return () => {
      a();
      b();
    };
  }, [meetingId, active]);
  return feed;
}

export function LiveTranscript({ finals, partial }: { finals: Caption[]; partial: Caption | null }) {
  const end = useRef<HTMLDivElement>(null);
  const [follow, setFollow] = useState(true);
  useEffect(() => {
    if (follow) end.current?.scrollIntoView({ block: "end" });
  }, [finals.length, partial?.text, follow]);

  if (finals.length === 0 && !partial) {
    return (
      <p className="inline-flex items-center gap-2 text-sm text-ink-3">
        <Radio size={14} className="text-ember" /> Listening... captions appear a few seconds behind the conversation.
      </p>
    );
  }
  return (
    <div
      className="max-h-[70vh] overflow-y-auto pr-1"
      onScroll={(e) => {
        const el = e.currentTarget;
        setFollow(el.scrollHeight - el.scrollTop - el.clientHeight < 40);
      }}
    >
      <ol className="flex flex-col gap-1.5">
        {finals.map((c, i) => (
          <li key={`${c.start_ms}-${i}`} className="flex items-baseline gap-3">
            <span className="w-12 shrink-0 font-mono text-[11px] text-ink-3">{fmtDuration(c.start_ms)}</span>
            <span className="text-[14px] leading-relaxed text-ink">{c.text}</span>
          </li>
        ))}
        {partial && (
          <li className="flex items-baseline gap-3" aria-live="polite">
            <span className="w-12 shrink-0 font-mono text-[11px] text-ink-3">{fmtDuration(partial.start_ms)}</span>
            <span className="text-[14px] leading-relaxed text-ink-2 italic">{partial.text}</span>
          </li>
        )}
      </ol>
      <div ref={end} />
    </div>
  );
}

export function LiveNotes({ notes, updatedMs }: { notes: string; updatedMs: number | null }) {
  if (!notes) {
    return (
      <p className="text-sm text-ink-3">
        Notes start once there is a minute or two of conversation, then keep updating as the meeting goes.
      </p>
    );
  }
  return (
    <div>
      <div className="mb-3 inline-flex items-center gap-2 rounded-full bg-ember-soft px-2.5 py-0.5 text-[11px] font-semibold text-ember">
        <Radio size={12} /> Live notes{updatedMs != null ? ` - as of ${fmtDuration(updatedMs)}` : ""}
      </div>
      <Markdown text={notes} />
    </div>
  );
}
