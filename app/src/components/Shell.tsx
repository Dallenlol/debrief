import { Circle, Library, MessageCircle, Settings as SettingsIcon, Square } from "lucide-react";
import { useEffect, useState } from "react";
import { NavLink, Outlet } from "react-router-dom";
import { cn } from "@/lib/cn";
import { fmtDuration } from "@/lib/format";
import { cmd, subscribe, type RecordingState } from "@/lib/ipc";
import { Button, Kbd } from "./ui";
import { Toasts } from "./Toasts";
import { UpdateBanner } from "./UpdateBanner";

export function Shell() {
  const [rec, setRec] = useState<RecordingState>({ state: "idle", meeting_id: null, elapsed_ms: 0 });
  const [hotkey, setHotkey] = useState("Ctrl+Shift+R");
  const [downloads, setDownloads] = useState<Record<string, { done: number; total: number }>>({});

  useEffect(() => subscribe("model_progress", (p) => {
    setDownloads((d) => {
      const next = { ...d };
      if (p.status === "downloading") next[p.id] = { done: p.done, total: p.total };
      else delete next[p.id];
      return next;
    });
  }), []);

  useEffect(() => {
    void cmd.recordingStatus().then(setRec);
    void cmd.getSettings().then((s) => setHotkey(s.hotkey.replace("CmdOrCtrl", navigator.platform.includes("Mac") ? "Cmd" : "Ctrl")));
    const un = subscribe("recording_state", setRec);
    const t = setInterval(() => {
      if (rec.state !== "idle") void cmd.recordingStatus().then(setRec);
    }, 1000);
    return () => {
      un();
      clearInterval(t);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [rec.state]);

  const recording = rec.state !== "idle";

  return (
    <div className="grain flex h-full">
      <aside className="relative z-10 flex w-[232px] shrink-0 flex-col border-r border-line bg-canvas-2/60 backdrop-blur">
        <div className="flex items-center gap-2 px-5 pt-5 pb-4">
          <span className="grid h-7 w-7 place-items-center rounded-md bg-ink text-canvas">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round">
              <path d="M4 12v2M8 8v8M12 5v14M16 8v8M20 11v3" />
            </svg>
          </span>
          <span className="font-serif text-[22px] leading-none tracking-tight">Hark</span>
        </div>

        <nav className="flex flex-col gap-0.5 px-3">
          <NavItem to="/" icon={<Library size={16} />} label="Library" />
          <NavItem to="/ask" icon={<MessageCircle size={16} />} label="Ask Hark" />
          <NavItem to="/settings" icon={<SettingsIcon size={16} />} label="Settings" />
        </nav>

        <div className="mt-auto p-3">
          <div className={cn("rounded-lg border p-3 transition-colors", recording ? "border-ember/30 bg-ember-soft" : "border-line bg-canvas")}>
            {recording ? (
              <>
                <div className="flex items-center gap-2 text-[12px] font-semibold text-ember">
                  <span className="rec-dot h-2 w-2 rounded-full bg-ember" />
                  {rec.state === "paused" ? "Paused" : "Recording"}
                  <span className="ml-auto font-mono tabular-nums">{fmtDuration(rec.elapsed_ms)}</span>
                </div>
                <Button variant="ember" size="sm" className="mt-2 w-full" onClick={() => void cmd.stopRecording()}>
                  <Square size={12} fill="currentColor" /> Stop
                </Button>
              </>
            ) : (
              <>
                <div className="text-[12px] text-ink-2">Ready when you are.</div>
                <Button variant="primary" size="sm" className="mt-2 w-full" onClick={() => void cmd.startRecording()}>
                  <Circle size={12} className="text-ember" fill="currentColor" /> Record now
                </Button>
                <div className="mt-2 text-center text-[11px] text-ink-3">
                  or press <Kbd>{hotkey}</Kbd>
                </div>
              </>
            )}
          </div>
        </div>
      </aside>

      <main className="relative z-10 min-w-0 flex-1 overflow-y-auto">
        <Outlet />
      </main>
      <Toasts />
      <UpdateBanner />
      {Object.keys(downloads).length > 0 && <DownloadPill downloads={downloads} />}
    </div>
  );
}

function NavItem({ to, icon, label }: { to: string; icon: React.ReactNode; label: string }) {
  return (
    <NavLink
      to={to}
      end={to === "/"}
      className={({ isActive }) =>
        cn(
          "focus-ring flex h-9 items-center gap-2.5 rounded-md px-3 text-sm font-medium transition-colors",
          isActive ? "bg-canvas text-ink shadow-card" : "text-ink-2 hover:bg-canvas-3 hover:text-ink",
        )
      }
    >
      {icon}
      {label}
    </NavLink>
  );
}

function DownloadPill({ downloads }: { downloads: Record<string, { done: number; total: number }> }) {
  const ids = Object.keys(downloads);
  const done = ids.reduce((n, id) => n + downloads[id].done, 0);
  const total = ids.reduce((n, id) => n + downloads[id].total, 0);
  const pct = total ? Math.round((done / total) * 100) : 0;
  return (
    <NavLink to="/settings" className="fixed bottom-4 left-4 z-40 flex w-[200px] flex-col gap-1 rounded-lg border border-line bg-canvas px-3 py-2 text-[12px] shadow-float hover:bg-canvas-2" title={ids.join(", ")}>
      <span className="flex items-center justify-between text-ink-2">
        <span>Downloading {ids.length === 1 ? "model" : `${ids.length} models`}</span>
        <span className="font-mono text-ink-3">{pct}%</span>
      </span>
      <span className="h-1 w-full overflow-hidden rounded-full bg-line"><span className="block h-full bg-ink transition-[width]" style={{ width: `${pct}%` }} /></span>
    </NavLink>
  );
}
