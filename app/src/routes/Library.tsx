import { CalendarDays, Circle, Search, Video } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { EmptyState } from "@/components/EmptyState";
import { FolderTree, type Selection } from "@/components/FolderTree";
import { Badge, Button, Input } from "@/components/ui";
import { appLabel, fmtDate, fmtDuration, fmtTime } from "@/lib/format";
import { cmd, subscribe, type CalEvent, type Folder, type Meeting, type SearchHit, type Tag } from "@/lib/ipc";

const STATUS_TONE = { recording: "ember", processing: "amber", ready: "moss", failed: "ember" } as const;

export function Library() {
  const [meetings, setMeetings] = useState<Meeting[] | null>(null);
  const [folders, setFolders] = useState<Folder[]>([]);
  const [tags, setTags] = useState<Tag[]>([]);
  const [selection, setSelection] = useState<Selection>({ kind: "all" });
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<SearchHit[] | null>(null);
  const [upcoming, setUpcoming] = useState<CalEvent[]>([]);
  const navigate = useNavigate();

  useEffect(() => {
    const load = () => void cmd.listUpcoming(12).then(setUpcoming).catch(() => setUpcoming([]));
    load();
    const t = setInterval(load, 60_000);
    return () => clearInterval(t);
  }, []);

  const reload = useCallback(() => {
    const filter =
      selection.kind === "all" ? { folder: null, tag_id: null }
      : selection.kind === "unfiled" ? { folder: [null], tag_id: null }
      : selection.kind === "folder" ? { folder: [selection.id], tag_id: null }
      : { folder: null, tag_id: selection.id };
    void cmd.listMeetingsFiltered(filter as { folder: null | (string | null)[]; tag_id: string | null }).then(setMeetings);
    void cmd.listFolders().then(setFolders);
    void cmd.listTags().then(setTags);
  }, [selection]);

  useEffect(() => {
    reload();
    const a = subscribe("recording_state", reload);
    const b = subscribe("processing", reload);
    return () => {
      a();
      b();
    };
  }, [reload]);

  useEffect(() => {
    const q = query.trim();
    if (q.length < 2) {
      setHits(null);
      return;
    }
    const t = setTimeout(() => void cmd.search(q).then(setHits), 120);
    return () => clearTimeout(t);
  }, [query]);

  const heading =
    selection.kind === "all" ? "Library"
    : selection.kind === "unfiled" ? "Unfiled"
    : selection.kind === "folder" ? (folders.find((f) => f.id === selection.id)?.name ?? "Folder")
    : `#${tags.find((t) => t.id === selection.id)?.name ?? "tag"}`;

  return (
    <div className="grid h-full grid-cols-[220px_minmax(0,1fr)]">
      <aside className="overflow-y-auto border-r border-line px-3 py-6">
        <FolderTree
          folders={folders}
          tags={tags}
          selection={selection}
          onSelect={setSelection}
          onChange={reload}
          onDropMeeting={(id, folderId) => void cmd.moveMeeting(id, folderId).then(reload)}
        />
      </aside>

      <div className="mx-auto w-full max-w-4xl px-8 py-8">
        <header className="mb-6 flex items-end justify-between gap-6">
          <div>
            <h1 className="font-serif text-[34px] leading-none tracking-tight">{heading}</h1>
            <p className="mt-1.5 text-[13px] text-ink-3">
              {meetings ? `${meetings.length} recording${meetings.length === 1 ? "" : "s"}, all on this computer.` : "Loading..."}
            </p>
          </div>
          <div className="relative w-72">
            <Search size={14} className="pointer-events-none absolute top-1/2 left-3 -translate-y-1/2 text-ink-3" />
            <Input placeholder="Search transcripts and titles" value={query} onChange={(e) => setQuery(e.target.value)} className="pl-8" />
          </div>
        </header>

        {!hits && selection.kind === "all" && upcoming.length > 0 && (
          <section className="mb-6">
            <div className="mb-2 flex items-center gap-1.5 text-[11px] font-semibold tracking-wide text-ink-3 uppercase"><CalendarDays size={12} /> Today</div>
            <ul className="flex flex-col gap-1">
              {upcoming.slice(0, 4).map((e) => {
                const start = new Date(e.start).getTime();
                const live = start - 10 * 60_000 <= Date.now() && Date.now() <= new Date(e.end).getTime();
                return (
                  <li key={`${e.uid}-${e.start}`} className="flex items-center gap-3 rounded-md border border-line px-3 py-2 text-[13px]">
                    <span className="w-14 shrink-0 font-mono text-[12px] text-ink-3">{fmtTime(e.start)}</span>
                    <span className="min-w-0 flex-1 truncate font-medium">{e.title}</span>
                    {e.attendees.length > 0 && <span className="hidden truncate text-[12px] text-ink-3 md:inline">{e.attendees.slice(0, 3).join(", ")}{e.attendees.length > 3 ? ` +${e.attendees.length - 3}` : ""}</span>}
                    {live ? (
                      <Button variant="ember" size="sm" onClick={() => void cmd.startRecording({ title: e.title })}><Circle size={10} fill="currentColor" /> Record</Button>
                    ) : (
                      <span className="text-[12px] text-ink-3">in {Math.max(1, Math.round((start - Date.now()) / 60_000))} min</span>
                    )}
                  </li>
                );
              })}
            </ul>
          </section>
        )}

        {hits ? (
          <ul className="flex flex-col gap-1">
            {hits.length === 0 && <li className="py-10 text-center text-sm text-ink-3">No matches.</li>}
            {hits.map((h) => (
              <li key={`${h.meeting_id}-${h.segment_id}`}>
                <button
                  className="focus-ring flex w-full items-baseline gap-4 rounded-md px-3 py-2 text-left hover:bg-canvas-2"
                  onClick={() => navigate(`/meeting/${h.meeting_id}${h.segment_id ? `?t=${h.start_ms}` : ""}`)}
                >
                  <span className="w-44 shrink-0 truncate text-[13px] font-medium">{h.meeting_title}</span>
                  <span className="w-12 shrink-0 font-mono text-[11px] text-ink-3">{h.segment_id ? fmtDuration(h.start_ms) : ""}</span>
                  <span className="truncate text-[13px] text-ink-2">{renderSnippet(h.snippet)}</span>
                </button>
              </li>
            ))}
          </ul>
        ) : meetings && meetings.length === 0 ? (
          selection.kind === "all" ? (
            <EmptyState
              title="Nothing recorded yet."
              body="Hark will offer to record when it notices a call. Or start one yourself."
              action={<Button variant="primary" onClick={() => void cmd.startRecording()}>Record now</Button>}
            />
          ) : (
            <EmptyState title="Nothing here." body="Drag meetings from the list onto a folder, or tag them from the meeting page." />
          )
        ) : (
          <ul className="flex flex-col">
            {meetings?.map((m, i) => (
              <li
                key={m.id}
                className="rise"
                style={{ animationDelay: `${Math.min(i, 12) * 25}ms` }}
                draggable
                onDragStart={(e) => {
                  e.dataTransfer.setData("text/hark-meeting", m.id);
                  e.dataTransfer.effectAllowed = "move";
                }}
              >
                <Link
                  to={`/meeting/${m.id}`}
                  className="focus-ring group flex items-center gap-4 rounded-lg border-b border-line px-3 py-3.5 transition-colors hover:bg-canvas-2"
                >
                  <div className="flex w-16 shrink-0 flex-col text-[12px] leading-tight text-ink-3">
                    <span className="font-medium text-ink-2">{fmtDate(m.started_at)}</span>
                    <span>{fmtTime(m.started_at)}</span>
                  </div>
                  <div className="min-w-0 flex-1">
                    <div className="truncate text-[15px] font-medium text-ink group-hover:underline group-hover:decoration-line-2 group-hover:underline-offset-4">
                      {m.title}
                    </div>
                    <div className="mt-0.5 flex items-center gap-2 text-[12px] text-ink-3">
                      <span>{appLabel(m.app)}</span>
                      {m.folder_id && selection.kind !== "folder" && <span>in {folders.find((f) => f.id === m.folder_id)?.name}</span>}
                      {m.has_video && (
                        <span className="inline-flex items-center gap-1">
                          <Video size={12} /> video
                        </span>
                      )}
                    </div>
                  </div>
                  <Badge tone={STATUS_TONE[m.status]}>{m.status}</Badge>
                  <span className="w-14 text-right font-mono text-[12px] tabular-nums text-ink-2">{fmtDuration(m.duration_ms)}</span>
                </Link>
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}

/** FTS5 marks matches with <b>..</b>; render them as React nodes so nothing is injected as HTML. */
function renderSnippet(snippet: string) {
  return snippet.split(/(<b>.*?<\/b>)/g).map((part, i) =>
    part.startsWith("<b>") ? (
      <b key={i} className="font-semibold text-ink">
        {part.slice(3, -4)}
      </b>
    ) : (
      <span key={i}>{part}</span>
    ),
  );
}
