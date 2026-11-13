// Claude access: each real lane's state as its agent reports it (its open question: a sign-in or
// trust ask), then the sample team's plan and key pool.
import { useSessions } from "@/features/sessions/store";

export function useAccess() {
  const { state } = useSessions();
  const lanes =
    state.status === "ready"
      ? state.sessions.map((s) => ({
          id: s.id,
          key: s.ticketKey,
          title: s.title || s.ticketKey,
          question: s.question,
        }))
      : [];
  return {
    lanes,
    sample: [
      { label: "Plan", value: "Team plan · 6 seats, 4 in use" },
      { label: "Key pool", value: "Team API key · $368 of $400 this month" },
      { label: "Overflow", value: "Cloud provider · needs a manager's sign-off" },
    ],
  };
}
