// Fork compare: a sample lane forked from a checkpoint, its two runs side by side.
import { useState } from "react";
import { notify, SAMPLE_ONLY } from "./toast";
import { LANES } from "./world";

const RUNS = [
  {
    id: "a",
    label: "A · round half to even",
    cost: 0.62,
    minutes: 9,
    adds: 6,
    dels: 2,
    criteria: "5/5",
    summary: "Matches the finance sheet; one more journey passes.",
  },
  {
    id: "b",
    label: "B · amend the sheet",
    cost: 0.4,
    minutes: 6,
    adds: 1,
    dels: 1,
    criteria: "4/5",
    summary: "Smaller change; needs Product to amend the contract.",
  },
] as const;

export function useFork() {
  const [kept, setKept] = useState<"a" | "b" | null>(null);
  return {
    lane: LANES.refunds.key,
    runs: RUNS,
    kept,
    keep: (id: "a" | "b"): void => {
      setKept(id);
      notify(SAMPLE_ONLY);
    },
  };
}
