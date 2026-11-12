// The keys a phone's soft keyboard lacks, as the bytes a desktop xterm sends for them.
import type { Terminal } from "@xterm/xterm";
import { type RefObject, useEffect, useRef, useState } from "react";
import { cn } from "@/lib/utils";

const ESC = "\x1b";

/** Visible label, accessible name, and the key's bytes (arrows by cursor-key mode, DECCKM), most used
 * first: what doesn't fit a phone's width is reached by scrolling the bar. */
const KEYS = [
  ["Esc", "Esc", ESC],
  ["⇧Tab", "Shift+Tab", `${ESC}[Z`],
  ["↑", "Up arrow", "A"],
  ["↓", "Down arrow", "B"],
  ["Tab", "Tab", "\t"],
  ["^C", "Ctrl+C", "\x03"],
  ["/", "Slash", "/"],
  ["⏎", "Enter", "\r"],
  ["←", "Left arrow", "D"],
  ["→", "Right arrow", "C"],
] as const;

/** A key's bytes: an arrow is ESC [ x, or ESC O x once the application asked for cursor-key mode. */
export function keyBytes(bytes: string, applicationCursor: boolean): string {
  if (!/^[A-D]$/.test(bytes)) return bytes;
  return `${ESC}${applicationCursor ? "O" : "["}${bytes}`;
}

/** Which ends of a sideways-scrolling strip hide keys. */
function useHidden(strip: RefObject<HTMLDivElement | null>) {
  const [hidden, setHidden] = useState({ left: false, right: false });
  useEffect(() => {
    const el = strip.current;
    if (!el) return;
    const update = (): void => {
      const left = el.scrollLeft > 1;
      const right = el.scrollLeft + el.clientWidth < el.scrollWidth - 1;
      setHidden((h) => (h.left === left && h.right === right ? h : { left, right }));
    };
    const observer = new ResizeObserver(update);
    observer.observe(el);
    el.addEventListener("scroll", update, { passive: true });
    return () => {
      observer.disconnect();
      el.removeEventListener("scroll", update);
    };
  }, [strip]);
  return hidden;
}

const fade = "pointer-events-none absolute inset-y-0 w-8 from-surface-2 transition-opacity";

/** Shown on touch or narrow screens; a tap never takes focus, so the soft keyboard stays up. A key
 * goes in as user input, as a typed one does: the terminal scrolls to the bottom, the selection clears,
 * and onData sends it. */
export function KeyBar({ terminal }: { terminal: () => Terminal | null }) {
  const strip = useRef<HTMLDivElement>(null);
  const hidden = useHidden(strip);
  return (
    <div className="relative hidden shrink-0 border-t border-line bg-surface-2 touch:block">
      <div
        ref={strip}
        role="group"
        aria-label="Terminal keys"
        className="flex gap-1 overflow-x-auto overscroll-x-contain px-1.5 py-1 [scrollbar-width:none]"
      >
        {KEYS.map(([label, name, bytes]) => (
          <button
            key={name}
            type="button"
            aria-label={name}
            className="grid min-h-11 min-w-10 shrink-0 place-items-center rounded-md border border-line bg-background px-2 font-mono text-[13px] text-foreground active:bg-hover"
            onPointerDown={(e) => {
              e.preventDefault();
            }}
            onClick={() => {
              const term = terminal();
              term?.input(keyBytes(bytes, term.modes.applicationCursorKeysMode), true);
            }}
          >
            {label}
          </button>
        ))}
      </div>
      <div
        data-fade="left"
        aria-hidden
        className={cn(fade, "left-0 bg-linear-to-r", hidden.left ? "opacity-100" : "opacity-0")}
      />
      <div
        data-fade="right"
        aria-hidden
        className={cn(fade, "right-0 bg-linear-to-l", hidden.right ? "opacity-100" : "opacity-0")}
      />
    </div>
  );
}
