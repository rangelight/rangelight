// The Sessions page's state: one WatchSessions stream, a snapshot and then every change.
import type { Session } from "@gen/proto/rangelight/session/v1alpha1/session_pb.ts";
import { useCallback, useEffect, useState } from "react";
import { applyEvent, followAll, type WatchEnd } from "@/api/live";

export type SessionsState =
  | { status: "loading" }
  | { status: "error"; reason: string }
  | { status: "ready"; sessions: Session[]; paused: Exclude<WatchEnd, "cancelled"> | null };

export function useSessions(): { state: SessionsState; reload: () => void } {
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
  return { state, reload };
}
