import { Check, Merge, Pencil, X } from "lucide-react";
import { useEffect, useId, useState } from "react";
import { cn } from "@/lib/cn";
import { fmtDuration } from "@/lib/format";
import type { MeetingSpeaker, SpeakerStat } from "@/lib/ipc";
import { SPEAKER_COLORS } from "./Transcript";
import { SectionTitle } from "./ui";

interface SpeakersPanelProps {
  stats: SpeakerStat[];
  speakers: MeetingSpeaker[];
  /** Attendees and known speakers, offered while typing. */
  nameOptions?: string[];
  onRename: (label: string, name: string) => void | Promise<void>;
  onAcceptSuggestion?: (label: string) => void | Promise<void>;
  onDismissSuggestion?: (label: string) => void;
  className?: string;
}

/**
 * Everyone Hark heard in this meeting. Naming two rows the same person merges
 * them - which is how you fix one voice that was split into two speakers.
 */
export function SpeakersPanel({ stats, speakers, nameOptions = [], onRename, onAcceptSuggestion, onDismissSuggestion, className }: SpeakersPanelProps) {
  const [editing, setEditing] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const listId = useId();

  // A row that disappears (merged away) must not keep the editor open.
  useEffect(() => {
    if (editing && !stats.some((s) => s.label === editing)) setEditing(null);
  }, [stats, editing]);

  if (stats.length === 0) return null;

  const suggestionFor = (label: string) => {
    const row = speakers.find((s) => s.label === label);
    return row && row.suggested_name && row.suggested_score != null && !row.speaker_id
      ? { name: row.suggested_name, score: row.suggested_score }
      : null;
  };

  const commit = async (label: string) => {
    const name = draft.trim();
    setEditing(null);
    if (name && name !== label) await onRename(label, name);
  };

  const options = Array.from(new Set([...stats.map((s) => s.label), ...nameOptions])).filter((n) => !/^Speaker \d+$/.test(n));

  return (
    <div className={className}>
      <SectionTitle hint={stats.length > 1 ? "Same name = same person" : undefined}>Speakers</SectionTitle>
      <ul className="flex flex-col divide-y divide-line rounded-lg border border-line">
        {stats.map((s, i) => {
          const color = SPEAKER_COLORS[i % SPEAKER_COLORS.length];
          const suggestion = suggestionFor(s.label);
          const others = stats.filter((o) => o.label !== s.label);
          return (
            <li key={s.label} className="px-3 py-2.5">
              <div className="flex items-center gap-2">
                <span className={cn("h-2 w-2 shrink-0 rounded-full bg-current", color)} aria-hidden />
                {editing === s.label ? (
                  <form
                    className="flex min-w-0 flex-1 items-center gap-1"
                    onSubmit={(e) => {
                      e.preventDefault();
                      void commit(s.label);
                    }}
                  >
                    <input
                      autoFocus
                      aria-label={`Name for ${s.label}`}
                      value={draft}
                      list={options.length ? listId : undefined}
                      onChange={(e) => setDraft(e.target.value)}
                      onKeyDown={(e) => e.key === "Escape" && setEditing(null)}
                      className="focus-ring h-7 min-w-0 flex-1 rounded border border-line-2 bg-canvas px-2 text-[13px] font-medium"
                    />
                    <button type="submit" aria-label="Save name" title="Save" className="focus-ring rounded text-moss">
                      <Check size={14} />
                    </button>
                    <button type="button" aria-label="Cancel" title="Cancel" onClick={() => setEditing(null)} className="focus-ring rounded text-ink-3">
                      <X size={14} />
                    </button>
                  </form>
                ) : (
                  <button
                    type="button"
                    onClick={() => {
                      setDraft(s.label);
                      setEditing(s.label);
                    }}
                    title="Rename this speaker"
                    className="focus-ring group/name inline-flex min-w-0 items-center gap-1.5 rounded text-[13px] font-medium hover:underline"
                  >
                    <span className="truncate">{s.label}</span>
                    <Pencil size={11} className="shrink-0 text-ink-3 opacity-0 transition-opacity group-hover/name:opacity-100" />
                  </button>
                )}
                <span className="ml-auto shrink-0 text-[11px] text-ink-3" title={`${fmtDuration(s.ms)} of speech`}>
                  {s.segments} line{s.segments === 1 ? "" : "s"}
                </span>
              </div>

              {suggestion && (
                <div className="mt-1.5 flex items-center gap-1.5 pl-4 text-[12px]">
                  <span className="text-ink-3">
                    Is this <span className="font-medium text-ink-2">{suggestion.name}</span>? ({Math.round(suggestion.score * 100)}% match)
                  </span>
                  <button
                    onClick={() => void onAcceptSuggestion?.(s.label)}
                    className="focus-ring rounded px-1 text-moss hover:underline"
                    aria-label={`Yes, this is ${suggestion.name}`}
                  >
                    Yes
                  </button>
                  <button onClick={() => onDismissSuggestion?.(s.label)} className="focus-ring rounded px-1 text-ink-3 hover:underline" aria-label="Dismiss suggestion">
                    No
                  </button>
                </div>
              )}

              {others.length > 0 && (
                <label className="mt-1.5 flex items-center gap-1.5 pl-4 text-[11px] text-ink-3">
                  <Merge size={11} className="shrink-0" />
                  <span className="sr-only sm:not-sr-only">Same person as</span>
                  <select
                    aria-label={`Merge ${s.label} into another speaker`}
                    value=""
                    onChange={(e) => e.target.value && void onRename(s.label, e.target.value)}
                    className="focus-ring h-6 min-w-0 max-w-[60%] truncate rounded border border-line-2 bg-canvas px-1 text-[11px] text-ink-2"
                  >
                    <option value="">nobody else</option>
                    {others.map((o) => (
                      <option key={o.label} value={o.label}>
                        {o.label}
                      </option>
                    ))}
                  </select>
                </label>
              )}
            </li>
          );
        })}
      </ul>
      {options.length > 0 && (
        <datalist id={listId}>
          {options.map((n) => (
            <option key={n} value={n} />
          ))}
        </datalist>
      )}
      {stats.length > 1 && (
        <p className="mt-2 text-[12px] text-ink-3">
          Split one person into two? Give both rows the same name (or pick them in "Same person as") and Hark merges them everywhere, including the summary.
        </p>
      )}
    </div>
  );
}
