// The History panel: a sample lane's checkpoints, and a fork from the latest.
import { dispatch } from "../engine";
import { notify, SAMPLE_ONLY } from "../toast";
import type { Checkpoint } from "../types";
import { useLane } from "./lane";

const CHECKPOINTS: Checkpoint[] = [
  { id: "k1", at: "09:12", label: "Contract locked", stage: "grill", cost: 0.42 },
  { id: "k2", at: "10:02", label: "Build: 2 lanes done", stage: "build", cost: 1.38 },
  { id: "k3", at: "11:17", label: "QA loop 3 (before the escalation)", stage: "qa", cost: 4.2 },
];

export function useHistory(key: string) {
  const lane = useLane(key);
  return {
    checkpoints: CHECKPOINTS,
    fork: (): void => {
      if (lane) dispatch({ type: "fork", lane: lane.key });
      notify(SAMPLE_ONLY);
    },
  };
}
