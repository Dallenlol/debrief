import { Circle, Library, Settings as SettingsIcon, Square } from "lucide-react";
import { useEffect, useState } from "react";
import { NavLink, Outlet } from "react-router-dom";
import { cn } from "@/lib/cn";
import { fmtDuration } from "@/lib/format";
import { cmd, subscribe, type RecordingState } from "@/lib/ipc";
import { Button, Kbd } from "./ui";
import { Toasts } from "./Toasts";

export function Shell() {
  const [rec, setRec] = useState<RecordingState>({ state: "idle", meeting_id: null, elapsed_ms: 0 });
  const [hotkey, setHotkey] = useState("Ctrl+Shift+R");

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
