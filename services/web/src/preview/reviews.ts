// Reviews' data: the real lanes' pull requests (the api's links), then the sample reviews.
import { safeHttps } from "@/components/rl/ticket-link";
import { useSessions } from "@/features/sessions/store";
import { dispatch, useSample } from "./engine";
import { notify, SAMPLE_ONLY } from "./toast";
import type { Review } from "./types";

export function useReviews() {
  const { state } = useSessions();
  const { reviews } = useSample();
  const real: Review[] =
    state.status === "ready"
      ? state.sessions
          .filter((s) => safeHttps(s.pullRequestUrl))
          .map((s) => ({
            sample: false,
            id: `pr/${s.id}`,
            number: s.pullRequestUrl.split("/").pop() ?? "",
            lane: s.ticketKey,
            title: s.title || s.ticketKey,
            // Who opened it isn't served yet: nothing is shown rather than a guess.
            author: "",
            repos: s.repo,
            proven: "",
            minutes: 0,
            chapters: [],
            url: s.pullRequestUrl,
          }))
      : [];
  return {
    reviews: [...real, ...reviews],
    approve: (id: string): void => {
      dispatch({ type: "approve", review: id });
      notify(SAMPLE_ONLY);
    },
  };
}
