/** 65000 -> "1:05", 3725000 -> "1:02:05". */
export function fmtDuration(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const mm = h > 0 ? String(m).padStart(2, "0") : String(m);
  return `${h > 0 ? h + ":" : ""}${mm}:${String(s).padStart(2, "0")}`;
}

/** Timestamp for transcripts: always m:ss or h:mm:ss. */
export const fmtStamp = fmtDuration;

export function fmtDate(iso: string): string {
  const d = new Date(iso);
  const now = new Date();
  const sameYear = d.getFullYear() === now.getFullYear();
  return d.toLocaleDateString(undefined, { month: "short", day: "numeric", year: sameYear ? undefined : "numeric" });
}

export function fmtTime(iso: string): string {
  return new Date(iso).toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" });
}

export function fmtBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let v = n / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v < 10 ? v.toFixed(1) : Math.round(v)} ${units[i]}`;
}

/** Normalize dBFS (-100..0) to 0..1 for meters, with a gentle curve. */
export function dbToLevel(db: number): number {
  const clamped = Math.min(0, Math.max(-60, db));
  return Math.pow((clamped + 60) / 60, 1.6);
}

export const APP_LABELS: Record<string, string> = {
  zoom: "Zoom",
  teams: "Teams",
  meet: "Google Meet",
  webex: "Webex",
  discord: "Discord",
  slack: "Slack",
  facetime: "FaceTime",
  gotomeeting: "GoToMeeting",
  unknown: "Call",
};

export function appLabel(app: string | null | undefined): string {
  if (!app) return "Recording";
  return APP_LABELS[app] ?? app;
}
