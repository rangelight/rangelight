// The sample world, live: actions change it (reduce), time moves its lanes on (advance), and every
// view that shows the same thing re-renders. Nothing here reaches the api.
import { useEffect, useSyncExternalStore } from "react";
import type { FeedEntry, Review } from "./types";
import { initial, STAGES, type SampleLane, type SampleWorld } from "./world";

export type Action =
  | { type: "decide"; id: string; key: 1 | 2 | 3 }
  | { type: "approve"; review: string }
  | { type: "say"; lane: string; text: string }
  | { type: "reply"; lane: string; text: string }
  | { type: "pause"; lane: string }
  | { type: "resume"; lane: string }
  | { type: "fork"; lane: string }
  | { type: "rerun"; lane: string }
  | { type: "undo" }
  /** A waiting ticket gets a lane, at the Grill (the guided story's "Start from Up next"). */
  | { type: "start"; key: string; title: string }
  /** The sample world as it starts (leaving the guided story). */
  | { type: "reset" };

/** A sample lane's feed. A real session has none here: sample text never speaks for a real lane. */
export const feedOf = (w: SampleWorld, lane: string): FeedEntry[] =>
  w.lanes.find((l) => l.key === lane)?.feed ?? [];

/** The world with `entry` on a sample lane's feed; anything else (a real session's id) is ignored. */
const withFeed = (w: SampleWorld, lane: string, entry: FeedEntry): SampleWorld => ({
  ...w,
  lanes: w.lanes.map((l) => (l.key === lane ? { ...l, feed: [...l.feed, entry] } : l)),
});

const setLane = (w: SampleWorld, key: string, fn: (l: SampleLane) => SampleLane): SampleWorld => ({
  ...w,
  lanes: w.lanes.map((l) => (l.key === key ? fn(l) : l)),
});

function decide(w: SampleWorld, id: string, key: 1 | 2 | 3): SampleWorld {
  const d = w.decisions.find((x) => x.id === id);
  if (d?.status !== "open") return w;
  const option = d.options.find((o) => o.key === key)?.label ?? "";
  const decided = {
    ...w,
    decisions: w.decisions.map((x) =>
      x.id === id ? { ...x, status: "decided" as const, chosen: key, decidedBy: "you" } : x,
    ),
  };
  const told = withFeed(decided, d.lane, {
    kind: "outcome",
    id: `n${String(w.next)}`,
    text: `You chose: ${option}`,
    meta: "signed into the receipt",
    tone: "ok",
  });
  // A lane held on this decision goes on.
  return setLane({ ...told, next: w.next + 1 }, d.lane, (l) =>
    l.status === "paused"
      ? {
          ...l,
          status: "running",
          working: "Working on your answer",
          stages: { ...l.stages, [l.stage]: "active" },
        }
      : l,
  );
}

const approve = (w: SampleWorld, id: string): SampleWorld => {
  const r = w.reviews.find((x) => x.id === id);
  if (!r || r.approved) return w;
  const reviews: Review[] = w.reviews.map((x) => (x.id === id ? { ...x, approved: true } : x));
  return withFeed({ ...w, reviews, next: w.next + 1 }, r.lane, {
    kind: "outcome",
    id: `n${String(w.next)}`,
    text: `You approved pull request ${r.number}`,
    meta: "the lane merges once the gates pass",
    tone: "ok",
  });
};

/** A second run of a sample lane from its latest checkpoint (once). */
function fork(w: SampleWorld, lane: string): SampleWorld {
  const from = w.lanes.find((l) => l.key === lane);
  if (!from || w.lanes.some((l) => l.key === `${lane}·b`)) return w;
  const id = `n${String(w.next)}`;
  const second: SampleLane = {
    ...from,
    key: `${lane}·b`,
    title: `${from.title} (fork)`,
    status: "running",
    working: "Trying the other approach",
    ticks: 0,
    budget: { spent: 0, cap: from.budget.cap },
    feed: [{ kind: "outcome", id, text: `Forked from ${lane}'s latest checkpoint`, tone: "brand" }],
  };
  return { ...w, next: w.next + 1, lanes: [...w.lanes, second] };
}

/** A ticket's new lane, at the Grill. */
const started = (key: string, title: string, id: string): SampleLane => ({
  key,
  title,
  repos: [],
  stage: "grill",
  stages: { grill: "active", build: "todo", qa: "todo", review: "todo", verify: "todo" },
  status: "running",
  working: "Drafting the contract",
  reran: false,
  budget: { spent: 0, cap: 8 },
  ticks: 0,
  feed: [{ kind: "outcome", id, text: "Workspace ready · the Grill started", tone: "ok" }],
});

/** The sample world after `a`. */
export function reduce(w: SampleWorld, a: Action): SampleWorld {
  const id = `n${String(w.next)}`;
  const next = { ...w, next: w.next + 1 };
  switch (a.type) {
    case "decide":
      return decide(w, a.id, a.key);
    case "approve":
      return approve(w, a.review);
    case "say":
      return withFeed(next, a.lane, { kind: "human", id, who: "you", text: a.text });
    case "reply":
      return withFeed(next, a.lane, { kind: "note", id, text: a.text });
    case "pause":
      return setLane(next, a.lane, (l) =>
        l.status === "running"
          ? {
              ...l,
              status: "paused",
              working: "Paused by you",
              feed: [...l.feed, { kind: "outcome", id, text: "Paused by you", tone: "warn" }],
            }
          : l,
      );
    case "resume":
      return setLane(next, a.lane, (l) =>
        l.status === "paused"
          ? {
              ...l,
              status: "running",
              working: "Resumed",
              feed: [...l.feed, { kind: "outcome", id, text: "Resumed by you", tone: "ok" }],
            }
          : l,
      );
    case "fork":
      return fork(w, a.lane);
    case "rerun":
      return setLane(next, a.lane, (l) =>
        l.reran
          ? l
          : {
              ...l,
              reran: true,
              feed: [
                ...l.feed,
                { kind: "outcome", id, text: "Re-ran the failing tests: all pass", tone: "ok" },
              ],
            },
      );
    case "undo":
      return { ...w, undone: !w.undone };
    case "start":
      if (w.lanes.some((l) => l.key === a.key)) return w;
      return { ...next, lanes: [...next.lanes, started(a.key, a.title, id)] };
    case "reset":
      return initial();
  }
}

/** A lane waits while a decision it raised waits on you. */
const heldByYou = (w: SampleWorld, l: SampleLane): boolean =>
  w.decisions.some((d) => d.lane === l.key && d.status === "open" && d.approver === "you");

/** How many ticks a lane spends in a stage. */
export const TICKS_PER_STAGE = 3;

/** The sample world one tick later: each running lane spends a little and, every few ticks, moves
 * to its next stage (and is done after Verify). */
export function advance(w: SampleWorld): SampleWorld {
  let next = w.next;
  const lanes = w.lanes.map((l): SampleLane => {
    if (l.status !== "running" || heldByYou(w, l)) return l;
    const spent = { ...l.budget, spent: Math.round((l.budget.spent + 0.05) * 100) / 100 };
    if (l.ticks + 1 < TICKS_PER_STAGE) return { ...l, ticks: l.ticks + 1, budget: spent };
    const at = STAGES.findIndex((s) => s.id === l.stage);
    const to = STAGES[at + 1];
    const id = `n${String(next++)}`;
    if (!to)
      return {
        ...l,
        ticks: 0,
        budget: spent,
        status: "done",
        working: "Verified in production",
        stages: { ...l.stages, [l.stage]: "done" },
        feed: [...l.feed, { kind: "outcome", id, text: "Verified in production", tone: "ok" }],
      };
    return {
      ...l,
      ticks: 0,
      budget: spent,
      stage: to.id,
      working: `${to.label} under way`,
      stages: { ...l.stages, [l.stage]: "done", [to.id]: "active" },
      feed: [
        ...l.feed,
        {
          kind: "outcome",
          id,
          text: `${STAGES[at]?.label ?? ""} done · ${to.label} started`,
          tone: "ok",
        },
      ],
    };
  });
  return { ...w, lanes, next };
}

// The store: one sample world per tab.
let world = initial();
const listeners = new Set<() => void>();

const subscribe = (l: () => void): (() => void) => {
  listeners.add(l);
  return () => listeners.delete(l);
};

/** Applies an action to the sample world, and only it. */
export function dispatch(a: Action): void {
  world = reduce(world, a);
  listeners.forEach((l) => {
    l();
  });
}

/** How often the sample world moves on. */
const TICK_MS = 4000;
let ticker: ReturnType<typeof setInterval> | undefined;
let viewers = 0;

/** The sample world, re-rendering on change; while any view reads it, its lanes move on. */
export function useSample(): SampleWorld {
  useEffect(() => {
    viewers += 1;
    ticker ??= setInterval(() => {
      world = advance(world);
      listeners.forEach((l) => {
        l();
      });
    }, TICK_MS);
    return () => {
      viewers -= 1;
      if (viewers === 0 && ticker) {
        clearInterval(ticker);
        ticker = undefined;
      }
    };
  }, []);
  return useSyncExternalStore(subscribe, () => world);
}
