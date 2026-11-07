import type { Emit } from "../events/events.ts";
import type { OnRuntimeEvent } from "./runtime.ts";

/** What the lane does with a runtime's events. */
export interface LaneOptions {
  emit: Emit;
  /** Pushes a checkpoint of the worktree to the mirror. */
  checkpoint: () => Promise<unknown>;
  /** Ends the agent once the runtime is gone and the last checkpoint is pushed. */
  exit: (code: number) => void;
}

/**
 * Runs the lane on a runtime's events: lane events go to Rangelight, every turn end and the
 * runtime's exit push a checkpoint, so work survives the pod (ADR 0045 §4). A failed checkpoint
 * is reported, not fatal: the next turn end tries again.
 */
export function laneHandler(o: LaneOptions): OnRuntimeEvent {
  const checkpoint = () =>
    o.checkpoint().catch((error: unknown) => {
      const note = `checkpoint failed: ${error instanceof Error ? error.message : String(error)}`;
      o.emit({ type: "progress", payload: { stage: "checkpoint", note: note.slice(0, 200) } });
    });
  return (event) => {
    switch (event.kind) {
      case "lane":
        o.emit(event.event);
        return;
      case "turn_start":
        return;
      case "turn_end":
        void checkpoint();
        return;
      case "exit":
        void checkpoint().then(() => {
          o.exit(event.code);
        });
    }
  };
}
