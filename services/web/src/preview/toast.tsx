// What a preview action says it did: one short notice at a time, read out politely.
import { useEffect, useSyncExternalStore } from "react";

let message = { text: "", at: 0 };
const listeners = new Set<() => void>();

/** Says `text` in the shell's notice. */
export function notify(text: string): void {
  message = { text, at: Date.now() };
  listeners.forEach((l) => {
    l();
  });
}

/** Said after every preview action that would need the api. */
export const SAMPLE_ONLY = "Preview: changed in the sample data only. Nothing was sent.";

const subscribe = (l: () => void): (() => void) => {
  listeners.add(l);
  return () => listeners.delete(l);
};

export function Toaster() {
  const shown = useSyncExternalStore(subscribe, () => message);
  useEffect(() => {
    if (!shown.text) return;
    const timer = setTimeout(() => {
      message = { text: "", at: 0 };
      listeners.forEach((l) => {
        l();
      });
    }, 5000);
    return () => {
      clearTimeout(timer);
    };
  }, [shown]);
  return (
    <div
      role="status"
      aria-live="polite"
      className="pointer-events-none fixed inset-x-3 bottom-20 z-20 flex justify-center sm:bottom-6"
    >
      {shown.text && (
        <p className="pointer-events-auto max-w-md rounded-lg border border-line-2 bg-raised px-3 py-2 text-[12.5px] text-foreground shadow-lg">
          {shown.text}
        </p>
      )}
    </div>
  );
}
