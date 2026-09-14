import { FolderOpen, FolderPlus, Inbox, Layers, Pencil, Trash2 } from "lucide-react";
import { useState } from "react";
import { cn } from "@/lib/cn";
import { cmd, type Folder, type Tag } from "@/lib/ipc";

export type Selection = { kind: "all" } | { kind: "unfiled" } | { kind: "folder"; id: string } | { kind: "tag"; id: string };

interface FolderTreeProps {
  folders: Folder[];
  tags: Tag[];
  selection: Selection;
  onSelect: (s: Selection) => void;
  onChange: () => void;
  /** Called when a meeting is dropped on a folder ("unfiled" clears). */
  onDropMeeting: (meetingId: string, folderId: string | null) => void;
}

/** Sidebar-style folder + tag navigation for the Library. */
export function FolderTree({ folders, tags, selection, onSelect, onChange, onDropMeeting }: FolderTreeProps) {
  const [editing, setEditing] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [dragOver, setDragOver] = useState<string | null>(null);

  const children = (parent: string | null) => folders.filter((f) => f.parent_id === parent);

  const addFolder = async (parent: string | null) => {
    const name = prompt(parent ? "Subfolder name" : "Folder name");
    if (!name?.trim()) return;
    await cmd.createFolder(name, parent);
    onChange();
  };

  const rename = async (f: Folder) => {
    const name = draft.trim();
    setEditing(null);
    if (name && name !== f.name) {
      await cmd.updateFolder(f.id, name, f.parent_id, f.default_template_id);
      onChange();
    }
  };

  const remove = async (f: Folder) => {
    if (!confirm(`Delete folder "${f.name}"? Meetings inside move to the parent folder.`)) return;
    await cmd.deleteFolder(f.id);
    if (selection.kind === "folder" && selection.id === f.id) onSelect({ kind: "all" });
    onChange();
  };

  const dropProps = (folderId: string | null, key: string) => ({
    onDragOver: (e: React.DragEvent) => {
      if (e.dataTransfer.types.includes("text/hark-meeting")) {
        e.preventDefault();
        setDragOver(key);
      }
    },
    onDragLeave: () => setDragOver((d) => (d === key ? null : d)),
    onDrop: (e: React.DragEvent) => {
      e.preventDefault();
      setDragOver(null);
      const id = e.dataTransfer.getData("text/hark-meeting");
      if (id) onDropMeeting(id, folderId);
    },
  });

  const row = (active: boolean, key: string) =>
    cn(
      "focus-ring group flex h-8 w-full items-center gap-2 rounded-md px-2 text-left text-[13px] transition-colors",
      active ? "bg-canvas text-ink shadow-card" : "text-ink-2 hover:bg-canvas-3 hover:text-ink",
      dragOver === key && "ring-2 ring-ember/50",
    );

  const renderFolder = (f: Folder, depth: number) => {
    const active = selection.kind === "folder" && selection.id === f.id;
    return (
      <div key={f.id}>
        <div className={row(active, f.id)} style={{ paddingLeft: 8 + depth * 14 }} {...dropProps(f.id, f.id)}>
          {editing === f.id ? (
            <form
              className="flex-1"
              onSubmit={(e) => {
                e.preventDefault();
                void rename(f);
              }}
            >
              <input autoFocus value={draft} onChange={(e) => setDraft(e.target.value)} onBlur={() => void rename(f)} className="w-full bg-transparent outline-none" />
            </form>
          ) : (
            <button className="flex min-w-0 flex-1 items-center gap-2" onClick={() => onSelect({ kind: "folder", id: f.id })}>
              <FolderOpen size={14} className="shrink-0 text-ink-3" />
              <span className="truncate">{f.name}</span>
              <span className="ml-auto text-[11px] text-ink-3">{f.meeting_count || ""}</span>
            </button>
          )}
          <span className="hidden shrink-0 items-center gap-0.5 group-hover:flex">
            <IconBtn title="Rename" onClick={() => { setEditing(f.id); setDraft(f.name); }}><Pencil size={11} /></IconBtn>
            <IconBtn title="New subfolder" onClick={() => void addFolder(f.id)}><FolderPlus size={11} /></IconBtn>
            <IconBtn title="Delete" onClick={() => void remove(f)}><Trash2 size={11} /></IconBtn>
          </span>
        </div>
        {children(f.id).map((c) => renderFolder(c, depth + 1))}
      </div>
    );
  };

  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-col gap-0.5">
        <button className={row(selection.kind === "all", "all")} onClick={() => onSelect({ kind: "all" })}>
          <Layers size={14} className="text-ink-3" /> All meetings
        </button>
        <button className={row(selection.kind === "unfiled", "unfiled")} onClick={() => onSelect({ kind: "unfiled" })} {...dropProps(null, "unfiled")}>
          <Inbox size={14} className="text-ink-3" /> Unfiled
        </button>
      </div>
      <div>
        <div className="mb-1 flex items-center justify-between px-2 text-[11px] font-semibold tracking-[0.12em] text-ink-3 uppercase">
          Folders
          <button onClick={() => void addFolder(null)} className="text-ink-3 hover:text-ink" title="New folder"><FolderPlus size={13} /></button>
        </div>
        <div className="flex flex-col gap-0.5">
          {children(null).map((f) => renderFolder(f, 0))}
          {folders.length === 0 && <div className="px-2 text-[12px] text-ink-3">Drag meetings into folders you create.</div>}
        </div>
      </div>
      {tags.length > 0 && (
        <div>
          <div className="mb-1 px-2 text-[11px] font-semibold tracking-[0.12em] text-ink-3 uppercase">Tags</div>
          <div className="flex flex-wrap gap-1 px-1">
            {tags.map((t) => {
              const active = selection.kind === "tag" && selection.id === t.id;
              return (
                <button key={t.id} onClick={() => onSelect(active ? { kind: "all" } : { kind: "tag", id: t.id })} className={cn("rounded-full px-2 py-0.5 text-[12px]", active ? "bg-ink text-canvas" : "bg-canvas-3 text-ink-2 hover:text-ink")}>
                  {t.name} <span className="opacity-60">{t.meeting_count}</span>
                </button>
              );
            })}
          </div>
        </div>
      )}
    </div>
  );
}

function IconBtn({ children, ...props }: React.ButtonHTMLAttributes<HTMLButtonElement>) {
  return (
    <button className="grid h-5 w-5 place-items-center rounded text-ink-3 hover:bg-canvas-3 hover:text-ink" {...props}>
      {children}
    </button>
  );
}
