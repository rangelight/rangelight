import { FitAddon } from "@xterm/addon-fit";
import { Terminal as XTerm, type ITheme } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { RefreshCw } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { openTerminal } from "@/api/client";
import { Button } from "@/components/ui/button";
import { useTheme } from "@/theme/theme";
import { inputFrames, outputBytes, resizeFrame } from "./terminal-protocol";

type Status = "connecting" | "open" | "closed";

/** The terminal's font: loaded before xterm measures its cells, or it measures a fallback. */
const FONT = '12.5px "Geist Mono Variable"';

/** xterm's colours, read from the design tokens of the current theme (from the mock-up). */
function palette(): ITheme {
  const css = getComputedStyle(document.documentElement);
  const v = (n: string) => css.getPropertyValue(n).trim();
  return {
    background: v("--code"),
    foreground: v("--foreground"),
    cursor: v("--primary"),
    selectionBackground: v("--primary-soft"),
    black: v("--faint"),
    red: v("--bad"),
    green: v("--ok"),
    yellow: v("--warn"),
    blue: v("--primary"),
    magenta: v("--violet"),
    cyan: v("--ss"),
    white: v("--foreground"),
    brightBlack: v("--dim"),
  };
}

/** An xterm in `el`, sized to it, in the tokens' colours. */
function open(el: HTMLElement): { term: XTerm; fit: FitAddon } {
  const term = new XTerm({
    fontFamily: '"Geist Mono Variable", ui-monospace, monospace',
    fontSize: 12.5,
    lineHeight: 1.35,
    cursorBlink: !matchMedia("(prefers-reduced-motion: reduce)").matches,
    theme: palette(),
    scrollback: 5000,
  });
  const fit = new FitAddon();
  term.loadAddon(fit);
  term.open(el);
  return { term, fit };
}

/** Wires a terminal in `el` to the session's terminal socket; returns the teardown. */
function connect(
  el: HTMLElement,
  sessionId: string,
  onStatus: (s: Status) => void,
): { term: XTerm; close: () => void } {
  const { term, fit } = open(el);
  const socket = openTerminal(sessionId);
  const send = (frame: string): void => {
    if (socket.readyState === WebSocket.OPEN) socket.send(frame);
  };
  // The PTY hears about a new size only when the cell grid actually changes.
  const resized = term.onResize(({ cols, rows }) => {
    send(resizeFrame(cols, rows));
  });
  socket.onopen = () => {
    onStatus("open");
    fit.fit();
    send(resizeFrame(term.cols, term.rows));
  };
  socket.onclose = () => {
    onStatus("closed");
  };
  socket.onmessage = (m: MessageEvent<unknown>) => {
    const bytes = outputBytes(m.data);
    if (bytes) term.write(bytes);
  };
  const typed = term.onData((d) => {
    inputFrames(new TextEncoder().encode(d)).forEach(send);
  });
  // Mouse reports and the like: one byte per character, not UTF-8.
  const binary = term.onBinary((d) => {
    inputFrames(Uint8Array.from(d, (c) => c.charCodeAt(0) & 0xff)).forEach(send);
  });
  const observer = new ResizeObserver(() => {
    fit.fit();
  });
  observer.observe(el);
  fit.fit();
  return {
    term,
    close: () => {
      observer.disconnect();
      typed.dispose();
      binary.dispose();
      resized.dispose();
      socket.onopen = socket.onclose = socket.onmessage = null;
      socket.close();
      term.dispose();
    },
  };
}

/** The lane's terminal, through the api's WebSocket (ADR 0040 §1): never a direct path to the pod. */
export function Terminal({ sessionId }: { sessionId: string }) {
  const el = useRef<HTMLDivElement>(null);
  const term = useRef<XTerm | null>(null);
  const [status, setStatus] = useState<Status>("connecting");
  const [attempt, setAttempt] = useState(0);
  const theme = useTheme();

  useEffect(() => {
    const node = el.current;
    if (!node) return;
    setStatus("connecting");
    let close: (() => void) | undefined;
    let unmounted = false;
    const start = (): void => {
      if (unmounted) return;
      const c = connect(node, sessionId, setStatus);
      term.current = c.term;
      close = c.close;
    };
    document.fonts.load(FONT).then(start, start);
    return () => {
      unmounted = true;
      close?.();
    };
  }, [sessionId, attempt]);

  useEffect(() => {
    if (term.current) term.current.options.theme = palette();
  }, [theme]);

  return (
    <section aria-labelledby="terminal-title" className="flex min-h-0 flex-col">
      <div className="flex shrink-0 items-center gap-3 border-b border-line px-4 py-1.5">
        <h2 id="terminal-title" className="text-[12px] font-semibold text-foreground">
          Terminal
        </h2>
        <span className="text-[12px] text-dim" role="status">
          {{ connecting: "Connecting…", open: "Connected", closed: "Disconnected" }[status]}
        </span>
        {status === "closed" && (
          <Button
            className="ml-auto"
            onClick={() => {
              setAttempt((n) => n + 1);
            }}
          >
            <RefreshCw aria-hidden /> Reconnect
          </Button>
        )}
      </div>
      <div
        ref={el}
        data-testid="terminal"
        className="min-h-0 flex-1 overflow-hidden bg-code px-3 py-2"
      />
    </section>
  );
}
