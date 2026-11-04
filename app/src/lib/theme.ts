import { useEffect, useState } from "react";

export type Theme = "light" | "dark";

const KEY = "debrief.theme";
const EVENT = "debrief-theme";

function stored(): Theme | null {
  try {
    const v = localStorage.getItem(KEY);
    return v === "light" || v === "dark" ? v : null;
  } catch {
    return null;
  }
}

/** The chosen theme; until the user picks one, whatever the OS uses. */
export function currentTheme(): Theme {
  return stored() ?? (window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light");
}

function apply() {
  document.documentElement.classList.toggle("dark", currentTheme() === "dark");
}

export function setTheme(t: Theme) {
  try {
    localStorage.setItem(KEY, t);
  } catch {
    // Storage blocked: the theme still applies for this session.
  }
  document.documentElement.classList.toggle("dark", t === "dark");
  window.dispatchEvent(new CustomEvent(EVENT));
}

/** Apply the theme and keep it in step with the OS (until a choice is made) and with
 * the other Debrief windows (record bar, popup), which share this storage. */
export function installTheme() {
  apply();
  window.matchMedia("(prefers-color-scheme: dark)").addEventListener("change", apply);
  window.addEventListener("storage", (e) => {
    if (e.key === KEY) {
      apply();
      window.dispatchEvent(new CustomEvent(EVENT));
    }
  });
}

export function useTheme(): [Theme, (t: Theme) => void] {
  const [t, setT] = useState<Theme>(currentTheme);
  useEffect(() => {
    const on = () => setT(currentTheme());
    window.addEventListener(EVENT, on);
    return () => window.removeEventListener(EVENT, on);
  }, []);
  return [t, setTheme];
}
