import { Ajv2020 } from "ajv/dist/2020.js";
import formats from "ajv-formats";
import schema from "../../../../api/jsonschema/agent-event.v1alpha1.schema.json" with { type: "json" };
import type {
  Ask,
  Compacted,
  Progress,
  RangelightAgentEventV1Alpha1,
  WorkerDone,
} from "../../../../shared/ts/gen/agent-event.v1alpha1.schema.d.ts";
import type { Clock, IdGen } from "./ids.ts";

type AgentEvent = RangelightAgentEventV1Alpha1;
type Payloads = { ask: Ask; worker_done: WorkerDone; progress: Progress; compacted: Compacted };

/**
 * The events a lane sends, with the types generated from
 * api/jsonschema/agent-event.v1alpha1.schema.json (shared/ts/gen). The schema ties each `type`
 * to its payload with if/then, which the generator can't express; `Payloads` does, and is keyed
 * by the generated `type` union, so a new event type fails to compile until it is mapped. The
 * envelope fields (messageId, sessionId, at) are added by the emitter.
 */
export type LaneEvent = {
  [K in AgentEvent["type"]]: { type: K; payload: Payloads[K] };
}[AgentEvent["type"]];

/** Sends one lane event. */
export type Emit = (event: LaneEvent) => void;

/** Thrown when the agent builds an event the contract rejects: a bug in the agent, never input. */
export class InvalidEventError extends Error {
  override name = "InvalidEventError";
}

const ajv = new Ajv2020({ strict: true, allErrors: true });
formats.default(ajv, ["date-time"]);
const validate = ajv.compile(schema);

/** Options for {@link createEmitter}. */
export interface EmitterOptions {
  sessionId: string;
  clock: Clock;
  ids: IdGen;
  /** Where one serialised event goes; stdout, one JSON object per line, by default. */
  write?: (line: string) => void;
}

/**
 * An emitter that wraps each lane event in its envelope, validates it against the contract and
 * writes it. Throws {@link InvalidEventError} instead of writing an invalid event.
 */
export function createEmitter(options: EmitterOptions): Emit {
  const write =
    options.write ??
    ((line: string) => {
      console.log(line);
    });
  return (event) => {
    const full = {
      messageId: options.ids.next(),
      sessionId: options.sessionId,
      at: options.clock.now().toISOString(),
      ...event,
    };
    if (!validate(full)) {
      throw new InvalidEventError(`${event.type}: ${ajv.errorsText(validate.errors)}`);
    }
    write(JSON.stringify(full));
  };
}
