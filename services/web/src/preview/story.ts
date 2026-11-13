// The guided story: the mock-up's chapters, played on the sample world and nothing else. A chapter
// is data: steps that, at a time from the chapter's start, go to a view and dispatch one of the
// engine's actions. It never calls the api, never touches a real session or the terminal.
import { useSyncExternalStore } from "react";
import { ARCHITECT, navigate } from "@/app/route";
import { type Action, dispatch } from "./engine";
import { LANES } from "./world";

export interface Step {
  /** Milliseconds from the chapter's start. */
  at: number;
  /** A view to show. */
  go?: string;
  act?: Action;
}

export interface Chapter {
  id: string;
  title: string;
  caption: string;
  steps: Step[];
}

const DECISIONS = "/decisions";
const REVIEWS = "/reviews";
const { refunds, webhook } = LANES;

export const CHAPTERS: Chapter[] = [
  {
    id: "morning",
    title: "Morning",
    caption: "While you were away: what finished overnight, and the one thing that needs you.",
    steps: [{ at: 0, go: ARCHITECT }],
  },
  {
    id: "upnext",
    title: "Start from Up next",
    caption: `The waiting P0 ticket, ${webhook.key}, gets a lane. The Grill turns it into a contract.`,
    steps: [
      { at: 0, go: ARCHITECT },
      { at: 1500, act: { type: "start", key: webhook.key, title: webhook.title } },
    ],
  },
  {
    id: "build",
    title: "Cross-repo build",
    caption: "A lane asks Security for production traces. Security says no: it reproduces locally.",
    steps: [
      { at: 0, go: DECISIONS },
      { at: 1500, act: { type: "decide", id: "d-access", key: 2 } },
    ],
  },
  {
    id: "loop",
    title: "The loop closes itself",
    caption: "QA and Security bounce the refund fix back and forth, and nobody was paged.",
    steps: [
      { at: 0, go: DECISIONS },
      {
        at: 1200,
        act: {
          type: "reply",
          lane: refunds.key,
          text: "Security gate green on loop 2. QA still counts 13 credits where it expects 12.",
        },
      },
    ],
  },
  {
    id: "autonomy",
    title: "Bounded autonomy",
    caption:
      "The contract says stop after three bounces. The lane stops, explains, and you choose.",
    steps: [
      { at: 0, go: DECISIONS },
      { at: 2000, act: { type: "decide", id: "d-bounce", key: 1 } },
    ],
  },
  {
    id: "budget",
    title: "Budget wall",
    caption: "Finishing needs more than the lane's budget. The manager signs off on the raise.",
    steps: [
      { at: 0, go: DECISIONS },
      { at: 1500, act: { type: "decide", id: "d-budget", key: 1 } },
    ],
  },
  {
    id: "fork",
    title: "Fork",
    caption: "Another approach runs in a second lane from the latest checkpoint; the winner stays.",
    steps: [
      { at: 0, go: ARCHITECT },
      { at: 1500, act: { type: "fork", lane: refunds.key } },
    ],
  },
  {
    id: "proof",
    title: "Proof",
    caption: "Every criterion has evidence. The pull request reads as a story, and you approve it.",
    steps: [
      { at: 0, go: REVIEWS },
      { at: 2000, act: { type: "approve", review: "sample/212" } },
    ],
  },
];

export interface StoryState {
  on: boolean;
  chapter: number;
  /** How many of the chapter's steps have run. */
  done: number;
  playing: boolean;
}

let state: StoryState = { on: false, chapter: 0, done: 0, playing: false };
let timers: ReturnType<typeof setTimeout>[] = [];
const listeners = new Set<() => void>();

function set(next: Partial<StoryState>): void {
  state = { ...state, ...next };
  listeners.forEach((l) => {
    l();
  });
}

function clear(): void {
  timers.forEach(clearTimeout);
  timers = [];
}

/** Plays the chapter's steps from the first one not run yet, keeping their spacing. */
function play(): void {
  clear();
  const steps = CHAPTERS[state.chapter]?.steps ?? [];
  const from = steps[state.done]?.at ?? 0;
  for (const step of steps.slice(state.done))
    timers.push(
      setTimeout(() => {
        if (step.go) navigate(step.go);
        if (step.act) dispatch(step.act);
        set({ done: state.done + 1, playing: state.done + 1 < steps.length });
      }, step.at - from),
    );
  set({ playing: state.done < steps.length });
}

/** Chapter `chapter` from its start: the sample world as it starts, every earlier chapter's
 * actions applied at once, then this chapter played. */
function goto(chapter: number): void {
  clear();
  dispatch({ type: "reset" });
  for (const c of CHAPTERS.slice(0, chapter))
    for (const step of c.steps) if (step.act) dispatch(step.act);
  set({ on: true, chapter, done: 0 });
  play();
}

export const story = {
  start: (): void => {
    goto(0);
  },
  next: (): void => {
    if (state.chapter < CHAPTERS.length - 1) goto(state.chapter + 1);
  },
  back: (): void => {
    goto(Math.max(0, state.chapter - 1));
  },
  pause: (): void => {
    clear();
    set({ playing: false });
  },
  /** Goes on where it paused, or plays a finished chapter again. */
  play: (): void => {
    if (state.done >= (CHAPTERS[state.chapter]?.steps.length ?? 0)) goto(state.chapter);
    else play();
  },
  /** Ends the story and puts the sample world back as it starts, live again. */
  leave: (): void => {
    clear();
    dispatch({ type: "reset" });
    set({ on: false, chapter: 0, done: 0, playing: false });
  },
};

const subscribe = (l: () => void): (() => void) => {
  listeners.add(l);
  return () => listeners.delete(l);
};

export function useStory(): StoryState {
  return useSyncExternalStore(subscribe, () => state);
}
