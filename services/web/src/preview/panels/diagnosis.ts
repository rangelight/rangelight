// The Diagnosis panel: why a sample lane loops, and the decision it waits on.
import { dispatch, useSample } from "../engine";
import { notify, SAMPLE_ONLY } from "../toast";
import { useLane } from "./lane";

const LOOPS = [
  { attempt: 1, at: "10:53", saw: "expected 12 credits refunded, got 13" },
  { attempt: 2, at: "11:04", saw: "same, after rounding the hold" },
  { attempt: 3, at: "11:17", saw: "same, after rounding the landed part" },
];

export function useDiagnosis(key: string) {
  const lane = useLane(key);
  const { decisions } = useSample();
  return {
    loops: LOOPS,
    cause: "The refund rounds half up; the finance sheet rounds half to even.",
    decision: decisions.find((d) => d.lane === lane?.key && d.kind === "escalation"),
    decide: (id: string, k: 1 | 2 | 3): void => {
      dispatch({ type: "decide", id, key: k });
      notify(SAMPLE_ONLY);
    },
  };
}
