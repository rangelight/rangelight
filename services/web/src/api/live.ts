// Following sessions as their events commit (WatchSessions, WatchSession), and applying events.
import { create } from "@bufbuild/protobuf";
import { Code, ConnectError } from "@connectrpc/connect";
import { DesiredState } from "@gen/proto/rangelight/runner/v1alpha1/runner_pb.ts";
import {
  type Change,
  type Event,
  type Session,
  SessionSchema,
} from "@gen/proto/rangelight/session/v1alpha1/session_pb.ts";
import { sessions } from "./client";

/** Why a watch stopped: events went missing (ABORTED, events_missed), it ended or failed, or we
 * cancelled it. */
export type WatchEnd = "missed" | "stopped" | "cancelled";

export function endOf(error: unknown, cancelled: boolean): WatchEnd {
  if (cancelled) return "cancelled";
  return ConnectError.from(error).code === Code.Aborted ? "missed" : "stopped";
}

/** How a watch ended, and the api's reason when it failed. */
export interface Ended {
  end: WatchEnd;
  reason: string;
}

async function until(run: () => Promise<void>, signal: AbortSignal): Promise<Ended> {
  try {
    await run();
    return { end: signal.aborted ? "cancelled" : "stopped", reason: "the stream closed" };
  } catch (error) {
    return { end: endOf(error, signal.aborted), reason: describe(error) };
  }
}

/** Every session, live: the snapshot first, then each committed event of any session. */
export function followAll(
  on: { snapshot: (sessions: Session[]) => void; change: (e: Event) => void },
  signal: AbortSignal,
): Promise<Ended> {
  return until(async () => {
    for await (const { kind } of sessions.watchSessions({}, { signal })) {
      if (kind.case === "snapshot") on.snapshot(kind.value.sessions);
      else if (kind.case === "change") on.change(kind.value);
    }
  }, signal);
}

/** One session's events as they commit. `on.open` runs when the api has subscribed (its response
 * headers arrived): a read of the session after that can't miss an event. */
export function follow(
  sessionId: string,
  on: { open: () => void; event: (e: Event) => void },
  signal: AbortSignal,
): Promise<Ended> {
  return until(async () => {
    for await (const { event } of sessions.watchSession(
      { sessionId },
      { signal, onHeader: on.open },
    )) {
      if (event) on.event(event);
    }
  }, signal);
}

/** The list with the event applied. A creation the list doesn't hold adds the session on top
 * (newest first); an event at or below a session's generation is already reflected, or a repeat. */
export function applyEvent(list: readonly Session[], e: Event): Session[] {
  const kind = e.change?.kind;
  if (kind?.case === "created" && !list.some((s) => s.id === e.sessionId)) {
    const { ticketId, ticketKey, title, ticketUrl, repo, namespace, state } = kind.value;
    const added = create(SessionSchema, {
      id: e.sessionId,
      ticketId,
      ticketKey,
      title,
      ticketUrl,
      repo,
      namespace,
      state,
      generation: e.seq,
      ...(e.at && { createdAt: e.at }),
    });
    return [added, ...list];
  }
  return list.map((s) => {
    if (s.id !== e.sessionId || e.seq <= s.generation) return s;
    const state = kind?.case === "stateChanged" ? kind.value.state : s.state;
    const reported = kind?.case === "phaseChanged" ? kind.value : undefined;
    const phase = reported?.phase ?? s.phase;
    // Only the report that first names the session's PR carries it; the first one stays.
    const pullRequestUrl = s.pullRequestUrl || (reported?.pullRequestUrl ?? "");
    return {
      ...s,
      state,
      phase,
      pullRequestUrl,
      question: question(s.question, kind),
      generation: e.seq,
    };
  });
}

/** The session's open question after a change: an ask opens it, a progress or a worker_done
 * (the lane went on) or leaving hot closes it, anything else leaves it (the api's rule). */
function question(open: string, kind: Change["kind"] | undefined): string {
  // Only a hot session has a lane to wait on.
  if (kind?.case === "stateChanged") return kind.value.state === DesiredState.HOT ? open : "";
  if (kind?.case !== "agentEvent") return open;
  switch (kind.value.kind.case) {
    case "ask":
      return kind.value.kind.value.question;
    case "progress":
    case "workerDone":
      return "";
    default:
      return open;
  }
}

/** A short, safe description of a failed call: its code and the api's machine reason. */
export function describe(error: unknown): string {
  const e = ConnectError.from(error);
  return `${Code[e.code]}: ${e.rawMessage}`;
}
