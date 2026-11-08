// Light and dark, from the mock-up's theme module: the choice is kept per browser, and the first
// visit follows the system.
import { useSyncExternalStore } from "react";

export type Theme = "dark" | "light";

const KEY = "rl.theme";
const listeners = new Set<() => void>();

function getTheme(): Theme {
  try {
    const stored = localStorage.getItem(KEY);
    if (stored === "light" || stored === "dark") return stored;
  } catch {
    // Storage blocked: fall through to the system's choice.
  }
  return matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark";
}

function setTheme(t: Theme): void {
  document.documentElement.classList.toggle("dark", t === "dark");
  try {
    localStorage.setItem(KEY, t);
  } catch {
    // Private mode: the theme still applies for this page.
  }
  listeners.forEach((l) => {
    l();
  });
}

export const applyTheme = (): void => {
  document.documentElement.classList.toggle("dark", getTheme() === "dark");
};

const current = (): Theme =>
  document.documentElement.classList.contains("dark") ? "dark" : "light";

export const toggleTheme = (): void => {
  setTheme(current() === "dark" ? "light" : "dark");
};

export function useTheme(): Theme {
  return useSyncExternalStore((l) => {
    listeners.add(l);
    return () => listeners.delete(l);
  }, current);
}
