// Role home: what your seat needs today. The counts the api has are real; the rest is sample.
import { liveLane } from "@/features/sessions/lanes";
import { useSessions } from "@/features/sessions/store";
import { useQuestions } from "./decisions";
import { useSample } from "./engine";

export function useHome() {
  const { state } = useSessions();
  const questions = useQuestions();
  const w = useSample();
  return {
    real: {
      needsYou: questions.length,
      live: state.status === "ready" ? state.sessions.filter(liveLane).length : 0,
    },
    sample: {
      waiting: w.decisions.filter((d) => d.status === "open" && d.approver !== "you").length,
      reviews: w.reviews.filter((r) => !r.approved).length,
      timeBack: "2h 12m",
    },
  };
}
