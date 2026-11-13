// The retro card: a finished sample lane's timeline and the one change it suggests.
import { LANES } from "./world";

export function useRetro() {
  return {
    lane: LANES.rustls.key,
    title: LANES.rustls.title,
    timeline: [
      { stage: "Build", minutes: 6 },
      { stage: "QA", minutes: 4 },
      { stage: "Review", minutes: 11, note: "waiting on a reviewer" },
      { stage: "Verify", minutes: 3 },
    ],
    suggestion:
      "Dependency bumps with a fully green receipt could merge on their own: review was 46% of this lane.",
  };
}
