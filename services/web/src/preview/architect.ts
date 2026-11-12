// The Architect's data: one stream across every lane. What needs you from the api (open questions)
// is real; what the architect did and saw is the sample world's.
import { liveLane } from "@/features/sessions/lanes";
import { useSessions } from "@/features/sessions/store";
import { useQuestions } from "./decisions";
import { dispatch, useSample } from "./engine";
import { notify, SAMPLE_ONLY } from "./toast";
import type { Decision } from "./types";
import { LANES, type SampleWorld } from "./world";

export type ItemKind = "needs" | "acted" | "heads" | "waiting" | "rollup";

export interface ArchitectItem {
  id: string;
  kind: ItemKind;
  at: string;
  lanes: string[];
  title: string;
  why?: string;
  decision?: Decision;
  sample: boolean;
}

function sampleItems(w: SampleWorld): ArchitectItem[] {
  const { refunds, badge, webhook } = LANES;
  const done = w.lanes.filter((l) => l.status === "done").map((l) => l.key);
  const pool = w.lanes.reduce((n, l) => n + l.budget.spent, 360);
  const items: ArchitectItem[] = [
    {
      id: "rollup",
      kind: "rollup",
      at: "09:40",
      lanes: done,
      sample: true,
      title: done.length
        ? `Since 09:40: ${done.join(", ")} verified in production.`
        : "Since 09:40: nothing finished overnight.",
    },
    {
      id: "overlap",
      kind: "acted",
      at: "10:58",
      lanes: [badge.key, refunds.key],
      sample: true,
      title: w.undone
        ? `Undone. ${badge.key} runs in parallel with ${refunds.key}.`
        : `Sequenced ${badge.key} behind ${refunds.key}.`,
      why: w.undone
        ? "Both still touch the cart totals module. I'll flag the conflict when it appears."
        : "Both touch the cart totals module. Waiting costs about 6 minutes and nothing else.",
    },
    {
      id: "p0",
      kind: "heads",
      at: "11:02",
      lanes: [webhook.key],
      sample: true,
      title: `${webhook.key} (P0) is labelled and waiting for a slot.`,
      why: `It changes the webhook handler next to the code ${refunds.key} is editing; starting after it avoids a conflict.`,
    },
    {
      id: "pool",
      kind: "heads",
      at: "11:05",
      lanes: [],
      sample: true,
      title: `Team pool at $${pool.toFixed(0)} of $400.`,
      why: "I'll hold Up next before touching running lanes.",
    },
  ];
  return [...items, ...decisionItems(w)];
}

/** The sample decisions still open: yours to decide, or waiting on someone else. */
function decisionItems(w: SampleWorld): ArchitectItem[] {
  const items: ArchitectItem[] = [];
  for (const d of w.decisions.filter((x) => x.status === "open"))
    items.push(
      d.approver === "you"
        ? {
            id: d.id,
            kind: "needs",
            at: d.at,
            lanes: [d.lane],
            title: d.title,
            decision: d,
            sample: true,
          }
        : {
            id: d.id,
            kind: "waiting",
            at: d.at,
            lanes: [d.lane],
            title: `Waiting on ${d.approver}: ${d.title}`,
            why: "Nothing for you unless it blocks the merge.",
            sample: true,
          },
    );
  return items;
}

export function useArchitect() {
  const { state } = useSessions();
  const w = useSample();
  const questions = useQuestions();
  const real: ArchitectItem[] = questions.map((d) => ({
    id: d.id,
    kind: "needs",
    at: "",
    lanes: [d.lane],
    title: d.title,
    decision: d,
    sample: false,
  }));
  return {
    items: [...real, ...sampleItems(w)],
    /** Real lanes running now, and the sample world's. */
    watching: {
      real: state.status === "ready" ? state.sessions.filter(liveLane).length : 0,
      sample: w.lanes.filter((l) => l.status !== "done").length,
    },
    undo: (): void => {
      dispatch({ type: "undo" });
      notify(SAMPLE_ONLY);
    },
    decide: (id: string, key: 1 | 2 | 3): void => {
      dispatch({ type: "decide", id, key });
      notify(SAMPLE_ONLY);
    },
  };
}

/** The strip on top of a lane: the newest thing that needs you, real first. */
export function useNeedsYou(): Decision | undefined {
  const questions = useQuestions();
  const { decisions } = useSample();
  return (
    questions.at(-1) ?? decisions.filter((d) => d.status === "open" && d.approver === "you").at(-1)
  );
}
