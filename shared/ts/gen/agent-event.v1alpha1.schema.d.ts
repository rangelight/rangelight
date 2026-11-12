// Generated from api/jsonschema/ by scripts/check.sh gen. Do not edit.

/**
 * One typed message from a lane's agent to Rangelight. The rld sidecar validates every outgoing message against this schema and its size caps before it publishes it; the model can say only what the schema allows (ADR 0045 §2 and §5). Free text to other lanes doesn't exist.
 */
export type RangelightAgentEventV1Alpha1 = {
  /**
   * Client event id, the idempotency key: receivers dedupe on it (ADR 0021, ADR 0045 §2).
   */
  messageId: string;
  /**
   * The lane's session. rld sets it from its own credential; the agent can't choose it.
   */
  sessionId: string;
  /**
   * When the agent emitted the event.
   */
  at: string;
  type: "ask" | "worker_done" | "progress" | "compacted";
  payload: {
    [k: string]: unknown;
  };
};
/**
 * This interface was referenced by `undefined`'s JSON-Schema
 * via the `definition` "uuidv7".
 */
export type Uuidv7 = string;

/**
 * The lane needs a member: it moves to waiting_for_you, and the dispatcher and the lane's owner see it (ADR 0045 §2).
 *
 * This interface was referenced by `undefined`'s JSON-Schema
 * via the `definition` "ask".
 */
export interface Ask {
  question: string;
  /**
   * Optional choices to offer the member.
   *
   * @minItems 2
   * @maxItems 8
   */
  options?: [string, string, ...string[]];
}
/**
 * The agent says it is done. This starts Rangelight's own checks on the pushed head; the summary is a claim, never proof (ADR 0045 §3).
 *
 * This interface was referenced by `undefined`'s JSON-Schema
 * via the `definition` "workerDone".
 */
export interface WorkerDone {
  summary: string;
  /**
   * The commit the agent pushed and wants checked.
   */
  headSha: string;
}
/**
 * Feed only (ADR 0045 §2), except that it ends a wait for the member: the lane went on. The agent sends one with stage 'turn' at each turn start (RAN-63).
 *
 * This interface was referenced by `undefined`'s JSON-Schema
 * via the `definition` "progress".
 */
export interface Progress {
  /**
   * A short slug, e.g. 'tests' or 'migrations'.
   */
  stage: string;
  note: string;
}
/**
 * The agent session's context was compacted. The PreCompact hook persists the summary, so the receipt records that compaction happened (ADR 0020).
 *
 * This interface was referenced by `undefined`'s JSON-Schema
 * via the `definition` "compacted".
 */
export interface Compacted {
  trigger: "auto" | "manual";
  summary: string;
}
