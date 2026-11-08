// The Session page's state: its live event feed (WatchSession) and the session (GetSession), read
// only once the watch is open so the header can't miss a change.
import type { Event, Session } from "@gen/proto/rangelight/session/v1alpha1/session_pb.ts";
import { Code, ConnectError } from "@connectrpc/connect";
import { useCallback, useEffect, useState } from "react";
import { sessions as api } from "@/api/client";
import { applyEvent, describe, follow, type WatchEnd } from "@/api/live";

export type SessionState =
  | { status: "loading" }
  | { status: "error"; reason: string }
  | { status: "missing" }
  | { status: "ready"; session: Session };

export type Watch = "watching" | Exclude<WatchEnd, "cancelled">;

/** Appends the event unless the feed has it already (receivers dedupe on Event.id). */
export function addEvent(feed: readonly Event[], e: Event): readonly Event[] {
  return feed.some((x) => x.id === e.id) ? feed : [...feed, e];
}

/** The session as read, with the events the watch delivered meanwhile applied. */
export function caughtUp(session: Session, feed: readonly Event[]): Session {
  return feed.reduce((s, e) => applyEvent([s], e)[0] ?? s, session);
}

export function useSession(id: string): {
  state: SessionState;
  feed: readonly Event[];
  watch: Watch;
  reconnect: () => void;
} {
  const [state, setState] = useState<SessionState>({ status: "loading" });
  const [feed, setFeed] = useState<readonly Event[]>([]);
  const [watch, setWatch] = useState<Watch>("watching");
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    const abort = new AbortController();
    const { signal } = abort;
    const seen: Event[] = [];
    const read = (): void => {
      api.getSession({ sessionId: id }, { signal }).then(
        ({ session }) => {
          setState(
            session ? { status: "ready", session: caughtUp(session, seen) } : { status: "missing" },
          );
        },
        (error: unknown) => {
          if (signal.aborted) return;
          const missing = ConnectError.from(error).code === Code.NotFound;
          setState(missing ? { status: "missing" } : { status: "error", reason: describe(error) });
        },
      );
    };
    const event = (e: Event): void => {
      seen.push(e);
      setFeed((f) => addEvent(f, e));
      setState((s) => (s.status === "ready" ? { ...s, session: caughtUp(s.session, [e]) } : s));
    };
    setWatch("watching");
    void follow(id, { open: read, event }, signal).then(({ end, reason }) => {
      if (end === "cancelled") return;
      setWatch(end);
      // A watch that fails before it opens leaves nothing to show: say why.
      setState((s) => (s.status === "loading" ? { status: "error", reason } : s));
    });
    return () => {
      abort.abort();
    };
  }, [id, attempt]);

  const reconnect = useCallback(() => {
    setAttempt((n) => n + 1);
  }, []);
  return { state, feed, watch, reconnect };
}
