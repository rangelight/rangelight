import { useEffect, useState } from "react";

/** How long ago, short: "45s", "12m", "2h 05m", "3d 4h". */
export function elapsed(fromMs: number, nowMs: number): string {
  const s = Math.max(0, Math.floor((nowMs - fromMs) / 1000));
  if (s < 60) return `${String(s)}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${String(m)}m`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${String(h)}h ${String(m % 60).padStart(2, "0")}m`;
  return `${String(Math.floor(h / 24))}d ${String(h % 24)}h`;
}

/** How often ages on screen move on. */
const NOW_EVERY_MS = 15_000;

/** The current time, refreshed every NOW_EVERY_MS. */
export function useNow(): number {
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    const timer = setInterval(() => {
      setNow(Date.now());
    }, NOW_EVERY_MS);
    return () => {
      clearInterval(timer);
    };
  }, []);
  return now;
}
