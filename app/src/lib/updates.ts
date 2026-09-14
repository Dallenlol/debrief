import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { isMock } from "./mock";

export interface AvailableUpdate {
  version: string;
  notes: string | null;
  install: (onProgress?: (done: number, total: number | null) => void) => Promise<void>;
}

let pending: Update | null = null;

/** Ask GitHub for a newer release. `null` when up to date, mocked, or offline. */
export async function checkForUpdate(): Promise<AvailableUpdate | null> {
  if (isMock) return null;
  let update: Update | null;
  try {
    update = await check({ timeout: 15_000 });
  } catch (e) {
    console.warn("update check failed", e);
    return null;
  }
  if (!update) return null;
  pending = update;
  return {
    version: update.version,
    notes: update.body ?? null,
    install: async (onProgress) => {
      let done = 0;
      let total: number | null = null;
      await pending?.downloadAndInstall((ev) => {
        if (ev.event === "Started") total = ev.data.contentLength ?? null;
        else if (ev.event === "Progress") done += ev.data.chunkLength;
        onProgress?.(done, total);
      });
      await relaunch();
    },
  };
}
