// The full stage strip: a sample lane's five stages and where it is.
import { STAGES } from "../world";
import { useLane } from "./lane";

export function useStages(key: string) {
  const lane = useLane(key);
  return STAGES.map((s) => ({ ...s, state: lane?.stages[s.id] ?? "todo" }));
}
