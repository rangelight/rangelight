// What every panel hook shares: the sample lane it shows, and the example a real session previews.
import { useSample } from "../engine";
import { LANES, type SampleLane } from "../world";

/** The sample lane a real session's panels show as an example: never that session's own. */
export const EXAMPLE = LANES.refunds.key;

/** The sample lane `key`, or the example lane when there is no such sample lane. */
export function useLane(key: string): SampleLane | undefined {
  const { lanes } = useSample();
  return lanes.find((l) => l.key === key) ?? lanes.find((l) => l.key === EXAMPLE);
}
