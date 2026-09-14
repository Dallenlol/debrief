import { Check, Pencil, X } from "lucide-react";
import { useId, useState } from "react";
import { cn } from "@/lib/cn";

export interface SpeakerChipProps {
  label: string;
  colorClass: string;
  suggestion?: { name: string; score: number } | null;
  onRename: (name: string) => void | Promise<void>;
  onAcceptSuggestion?: () => void | Promise<void>;
  onDismissSuggestion?: () => void;
  /** Names to offer while typing: attendees seen in the meeting and known speakers. */
  nameOptions?: string[];
}

/** Speaker label in the transcript: click to rename; shows a confirmable "Is this X?" chip. */
export function SpeakerChip({ label, colorClass, suggestion, onRename, onAcceptSuggestion, onDismissSuggestion, nameOptions = [] }: SpeakerChipProps) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(label);
  const listId = useId();

  const commit = async () => {
    const name = draft.trim();
    setEditing(false);
    if (name && name !== label) await onRename(name);
  };

  if (editing) {
    return (
      <form
        className="mb-0.5 inline-flex items-center gap-1"
        onSubmit={(e) => {
          e.preventDefault();
          void commit();
        }}
      >
        <input
          autoFocus
          aria-label="Speaker name"
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => e.key === "Escape" && setEditing(false)}
          list={nameOptions.length ? listId : undefined}
          className="focus-ring h-6 w-40 rounded border border-line-2 bg-canvas px-2 text-[12px] font-semibold"
        />
        <button type="submit" className="focus-ring rounded text-moss" title="Save" aria-label="Save name">
          <Check size={14} />
        </button>
        <button type="button" onClick={() => setEditing(false)} className="focus-ring rounded text-ink-3" title="Cancel" aria-label="Cancel">
          <X size={14} />
        </button>
        {nameOptions.length > 0 && (
          <datalist id={listId}>
            {nameOptions.map((n) => <option key={n} value={n} />)}
          </datalist>
        )}
      </form>
    );
  }

  return (
    <div className="mb-0.5 flex flex-wrap items-center gap-2">
      <button
        type="button"
        onClick={() => {
          setDraft(label);
          setEditing(true);
        }}
        className={cn("group/chip inline-flex items-center gap-1 rounded text-[12px] font-semibold hover:underline", colorClass)}
        title="Rename speaker"
      >
        {label}
        <Pencil size={11} className="opacity-0 transition-opacity group-hover/chip:opacity-60" />
      </button>
      {suggestion && (
        <span className="inline-flex items-center gap-1 rounded-full bg-amber-soft px-2 py-0.5 text-[11px] text-ink">
          Is this <b>{suggestion.name}</b>?
          <button type="button" onClick={() => void onAcceptSuggestion?.()} className="ml-1 font-semibold text-moss hover:underline">
            Yes
          </button>
          <button type="button" onClick={onDismissSuggestion} className="font-semibold text-ink-3 hover:underline">
            No
          </button>
        </span>
      )}
    </div>
  );
}
