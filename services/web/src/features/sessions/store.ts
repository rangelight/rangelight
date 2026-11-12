// Every session, live: one WatchSessions stream for the whole Workbench (the shell's fleet and the
// Sessions page share it), a snapshot and then every change.
import type { Session } from "@gen/proto/rangelight/session/v1alpha1/session_pb.ts";
import {
  createContext,
  createElement,
  type ReactNode,
  useCallback,
  useContext,
  useEffect,
  useState,
} from "react";
import { signedOut } from "@/api/client";
import { applyEvent, followAll, type WatchEnd } from "@/api/live";

export type SessionsState =
  | { status: "loading" }
  | { status: "error"; reason: string }
  | { status: "ready"; sessions: Session[]; paused: Exclude<WatchEnd, "cancelled"> | null };

/** A stream that stopped or failed is tried again this long after. */
const RETRY_MS = 10_000;

/** Whether the stream should be tried again on its own: it stopped or failed, for any reason but a
 * missing login (that one is the sign-in redirect's, whose loop guard a retry would defeat). */
export const retries = (state: SessionsState): boolean =>
  (state.status === "error" && !signedOut(state.reason)) ||
  (state.status === "ready" && state.paused !== null);

interface Value {
  state: SessionsState;
  reload: () => void;
}

const Sessions = createContext<Value | null>(null);

export function SessionsProvider({ children }: { children: ReactNode }) {
  const [state, setState] = useState<SessionsState>({ status: "loading" });
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    const abort = new AbortController();
    void followAll(
      {
        snapshot: (sessions) => {
          setState({ status: "ready", sessions, paused: null });
        },
        change: (e) => {
          setState((s) =>
            s.status === "ready" ? { ...s, sessions: applyEvent(s.sessions, e) } : s,
          );
        },
      },
      abort.signal,
    ).then(({ end, reason }) => {
      if (end === "cancelled") return;
      // Before a snapshot the list never loaded: that's an error, not a pause.
      setState((s) => (s.status === "ready" ? { ...s, paused: end } : { status: "error", reason }));
    });
    return () => {
      abort.abort();
    };
  }, [attempt]);

  const reload = useCallback(() => {
    setState({ status: "loading" });
    setAttempt((n) => n + 1);
  }, []);

  useEffect(() => {
    if (!retries(state)) return;
    const timer = setTimeout(reload, RETRY_MS);
    return () => {
      clearTimeout(timer);
    };
  }, [state, reload]);

  return createElement(Sessions.Provider, { value: { state, reload } }, children);
}

/** The shared stream's state, and a way to reload it now. */
export function useSessions(): Value {
  const value = useContext(Sessions);
  if (!value) throw new Error("useSessions outside SessionsProvider");
  return value;
}
