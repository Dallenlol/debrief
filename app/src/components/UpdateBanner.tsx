import { Download, X } from "lucide-react";
import { useEffect, useState } from "react";
import { cmd } from "@/lib/ipc";
import { checkForUpdate, type AvailableUpdate } from "@/lib/updates";
import { Button } from "./ui";

/** Checks for a newer release shortly after launch and offers a one-click install. */
export function UpdateBanner() {
  const [update, setUpdate] = useState<AvailableUpdate | null>(null);
  const [state, setState] = useState<"idle" | "installing" | "failed">("idle");
  const [progress, setProgress] = useState<string>("");
  const [dismissed, setDismissed] = useState(false);

  useEffect(() => {
    let cancelled = false;
    const t = setTimeout(() => {
      void cmd.getSettings().then((s) => {
        if (!s.auto_update_check || cancelled) return;
        void checkForUpdate().then((u) => !cancelled && setUpdate(u));
      });
    }, 20_000);
    return () => {
      cancelled = true;
      clearTimeout(t);
    };
  }, []);

  if (!update || dismissed) return null;

  const install = async () => {
    setState("installing");
    try {
      await update.install((done, total) => setProgress(total ? `${Math.round((done / total) * 100)}%` : `${Math.round(done / 1e6)} MB`));
    } catch (e) {
      console.error(e);
      setState("failed");
    }
  };

  return (
    <div role="status" className="rise pointer-events-auto fixed right-4 bottom-4 z-50 flex w-[360px] items-start gap-2.5 rounded-lg border border-line bg-canvas p-3 text-[13px] shadow-float">
      <Download size={15} className="mt-0.5 shrink-0 text-ink-2" />
      <div className="min-w-0 flex-1">
        <div className="font-medium text-ink">Hark {update.version} is available</div>
        <div className="mt-0.5 text-ink-3">
          {state === "installing" ? `Downloading ${progress}... Hark restarts when it is done.` : state === "failed" ? "Install failed. Download it from the releases page instead." : "Installs in the background, then restarts."}
        </div>
        {state === "idle" && (
          <div className="mt-2 flex gap-2">
            <Button variant="primary" size="sm" onClick={() => void install()}>Install now</Button>
            <Button variant="ghost" size="sm" onClick={() => setDismissed(true)}>Later</Button>
          </div>
        )}
      </div>
      <button aria-label="Dismiss" className="text-ink-3 hover:text-ink" onClick={() => setDismissed(true)}>
        <X size={14} />
      </button>
    </div>
  );
}
