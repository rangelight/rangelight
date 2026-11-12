// What a session's lane is doing, from its runner's phase and its desired state (Session fields).
import { DesiredState, Phase } from "@gen/proto/rangelight/runner/v1alpha1/runner_pb.ts";
import type { Session } from "@gen/proto/rangelight/session/v1alpha1/session_pb.ts";

/** A live lane: its pod has started and isn't gone (starting, running, hibernating or hibernated
 * with its work kept, or waking): the Fleet wall's tiles. */
export const liveLane = (s: Session): boolean =>
  [Phase.STARTING, Phase.RUNNING, Phase.HIBERNATING, Phase.HIBERNATED, Phase.WAKING].includes(
    s.phase,
  );

/** No lane yet: the runner hasn't reported, or has it queued; and it isn't torn down (Up next). */
export const notStarted = (s: Session): boolean =>
  (s.phase === Phase.UNSPECIFIED || s.phase === Phase.QUEUED) && s.state !== DesiredState.COLD;
