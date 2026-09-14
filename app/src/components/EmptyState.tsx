import type { ReactNode } from "react";

export function EmptyState({ title, body, action }: { title: string; body?: string; action?: ReactNode }) {
  return (
    <div className="rise mx-auto flex max-w-md flex-col items-center py-24 text-center">
      <div className="mb-5 flex items-end gap-1" aria-hidden>
        {[10, 22, 34, 18, 28, 12].map((h, i) => (
          <span key={i} className="w-1.5 rounded-full bg-line-2" style={{ height: h }} />
        ))}
      </div>
      <h2 className="font-serif text-[28px] leading-tight text-ink">{title}</h2>
      {body && <p className="mt-2 text-[14px] text-ink-2">{body}</p>}
      {action && <div className="mt-6">{action}</div>}
    </div>
  );
}
