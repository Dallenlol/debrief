import { AlertTriangle, Info, X, XCircle } from "lucide-react";
import { useEffect, useState } from "react";
import { cn } from "@/lib/cn";
import { subscribe, type Events } from "@/lib/ipc";

type Toast = Events["notice"] & { id: number };

export function Toasts() {
  const [items, setItems] = useState<Toast[]>([]);
  useEffect(() => {
    let n = 0;
    return subscribe("notice", (p) => {
      const id = ++n;
      setItems((xs) => [...xs, { ...p, id }]);
      setTimeout(() => setItems((xs) => xs.filter((x) => x.id !== id)), p.level === "error" ? 9000 : 5000);
    });
  }, []);
  if (items.length === 0) return null;
  const icons = { info: <Info size={15} />, warning: <AlertTriangle size={15} />, error: <XCircle size={15} /> };
  return (
    <div className="pointer-events-none fixed right-4 bottom-4 z-50 flex w-[360px] flex-col gap-2">
      {items.map((t) => (
        <div
          key={t.id}
          className={cn(
            "rise pointer-events-auto flex items-start gap-2.5 rounded-lg border bg-canvas p-3 text-[13px] shadow-float",
            t.level === "error" ? "border-ember/40 text-ember" : t.level === "warning" ? "border-amber/50 text-ink" : "border-line text-ink",
          )}
        >
          <span className="mt-0.5 shrink-0">{icons[t.level]}</span>
          <span className="flex-1">{t.message}</span>
          <button className="text-ink-3 hover:text-ink" onClick={() => setItems((xs) => xs.filter((x) => x.id !== t.id))}>
            <X size={14} />
          </button>
        </div>
      ))}
    </div>
  );
}
