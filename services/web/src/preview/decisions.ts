// The Decisions inbox's data: the real open questions (the api's), then the sample decisions.
import type { Session } from "@gen/proto/rangelight/session/v1alpha1/session_pb.ts";
import { useSessions } from "@/features/sessions/store";
import { dispatch, useSample } from "./engine";
import { notify, SAMPLE_ONLY } from "./toast";
import type { Decision } from "./types";

/** A real session's open question, as a decision: answered in its terminal, not here. */
export const question = (s: Session): Decision => ({
  sample: false,
  id: `q-${s.id}`,
  lane: s.ticketKey,
  sessionId: s.id,
  kind: "question",
  title: s.question,
  why: "The lane asks in its terminal. Answer it there.",
  options: [],
  approver: "you",
  status: "open",
  at: "",
});

/** The real sessions' open questions. */
export function useQuestions(): Decision[] {
  const { state } = useSessions();
  return state.status === "ready" ? state.sessions.filter((s) => s.question).map(question) : [];
}

export function useDecisions() {
  const questions = useQuestions();
  const { decisions } = useSample();
  const open = decisions.filter((d) => d.status === "open");
  return {
    needsYou: [...questions, ...open.filter((d) => d.approver === "you")],
    waiting: open.filter((d) => d.approver !== "you"),
    decided: decisions.filter((d) => d.status === "decided"),
    decide: (id: string, key: 1 | 2 | 3): void => {
      dispatch({ type: "decide", id, key });
      notify(SAMPLE_ONLY);
    },
  };
}
