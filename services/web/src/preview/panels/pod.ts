// The Pod panel: a sample lane's pod, its resources and worktrees, and hibernate/wake.
import { dispatch } from "../engine";
import { notify, SAMPLE_ONLY } from "../toast";
import { useLane } from "./lane";

export function usePod(key: string) {
  const lane = useLane(key);
  const k = (lane?.key ?? "").toLowerCase();
  return {
    namespace: `rl-${k}`,
    node: "pool-c-7xk2",
    image: "agent:0.9.2",
    cpu: { used: 2.6, limit: 4 },
    mem: { used: 5.1, limit: 8 },
    worktrees: (lane?.repos ?? []).map((r) => ({ repo: r, branch: `rangelight/${k}`, ahead: 3 })),
    paused: lane?.status === "paused",
    toggle: (): void => {
      if (lane) dispatch({ type: lane.status === "paused" ? "resume" : "pause", lane: lane.key });
      notify(SAMPLE_ONLY);
    },
  };
}
