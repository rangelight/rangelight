// The Coordinator's data: a lane's header and, for a sample lane only, its feed, composer and
// actions. A real session shows only what the api serves (its open question); it is talked to in
// its terminal.
import { useSessions } from "@/features/sessions/store";
import { question } from "./decisions";
import { dispatch, feedOf, useSample } from "./engine";
import { notify, SAMPLE_ONLY } from "./toast";
import type { Decision, FeedEntry } from "./types";

export interface CoordinatorLane {
  /** A real session's id, or a sample lane's key. */
  id: string;
  key: string;
  title: string;
  sample: boolean;
  /** The real session's open question, as a card (a sample lane asks through its decisions). */
  ask?: Decision;
  working: string;
  status: "running" | "paused" | "done" | "";
}

/** What the agent answers in the sample world. */
const REPLY = "Got it. I'll fold that into the current loop and note it in the receipt.";

export function useCoordinator(selected: string | null) {
  const { state } = useSessions();
  const w = useSample();
  const real: CoordinatorLane[] =
    state.status === "ready"
      ? state.sessions.map((s) => ({
          id: s.id,
          key: s.ticketKey,
          title: s.title || s.ticketKey,
          sample: false,
          ...(s.question && { ask: question(s) }),
          working: "",
          status: "",
        }))
      : [];
  const sample: CoordinatorLane[] = w.lanes.map((l) => ({
    id: l.key,
    key: l.key,
    title: l.title,
    sample: true,
    working: l.working,
    status: l.status,
  }));
  const lane = [...real, ...sample].find((l) => l.id === selected) ?? real[0] ?? sample[0];
  const feed: FeedEntry[] = lane ? feedOf(w, lane.id) : [];
  const said = (): void => {
    notify(SAMPLE_ONLY);
  };
  return {
    lanes: { real, sample },
    lane,
    feed,
    decision: (id: string): Decision | undefined => w.decisions.find((d) => d.id === id),
    say: (text: string): void => {
      // A real lane is talked to in its terminal: the sample agent never answers for it.
      if (!lane?.sample) return;
      dispatch({ type: "say", lane: lane.id, text });
      setTimeout(() => {
        dispatch({ type: "reply", lane: lane.id, text: REPLY });
      }, 900);
      said();
    },
    decide: (id: string, key: 1 | 2 | 3): void => {
      dispatch({ type: "decide", id, key });
      said();
    },
    pause: (): void => {
      if (lane?.sample)
        dispatch({ type: lane.status === "paused" ? "resume" : "pause", lane: lane.id });
      said();
    },
    fork: (): void => {
      if (lane?.sample) dispatch({ type: "fork", lane: lane.id });
      said();
    },
  };
}
