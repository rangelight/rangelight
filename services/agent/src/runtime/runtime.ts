import type { LaneEvent } from "../events/events.ts";

/**
 * What a runtime reports, in order. `lane` events go to Rangelight as they are; the turn and exit
 * events drive the agent itself. `turn_start` is the proof that a prompt or an injected message is
 * in the session (ADR 0045 §2: a message is acked only after injection).
 */
export type RuntimeEvent =
  | { kind: "lane"; event: LaneEvent }
  | { kind: "turn_start" }
  | { kind: "turn_end"; outcome: "ended" | "cancelled" }
  | { kind: "exit"; code: number };

/** Receives a runtime's events, in the order they happened. */
export type OnRuntimeEvent = (event: RuntimeEvent) => void;

/** Whether {@link AgentRuntime.inject} took the message. */
export type InjectResult = { ok: true } | { ok: false; reason: "empty" };

/** The feed line when a session can't be resumed and a new one starts (ADR 0045 §4). */
export const RESUME_FAILED: LaneEvent = {
  type: "progress",
  payload: {
    stage: "resume",
    note: "could not resume the session; started a new one with the digest",
  },
};

/**
 * The agent runtime seam (ADR 0049 §2.2, ADR 0046 §2). Implementations: `fake` (tests) and `cli`
 * (the unmodified Claude Code binary in a PTY). Every implementation passes the shared contract
 * suite in contract.ts.
 */
export interface AgentRuntime {
  /** Starts a new session whose first turn is the prompt. */
  start(prompt: string): Promise<void>;
  /**
   * Resumes an existing session; its next turn is the prompt (the digest on reopen). If the
   * session can't be resumed, reports {@link RESUME_FAILED} and starts a new one with the prompt.
   */
  resume(sessionId: string, prompt: string): Promise<void>;
  /**
   * Queues a message; it becomes a turn of its own, never in the middle of the running turn. A
   * message with no visible text after {@link cleanMessage} is refused.
   */
  inject(message: string): InjectResult;
  /** Cancels the running turn: it ends with `cancelled`. Does nothing between turns. */
  interrupt(): void;
  /** Ends the runtime and its processes; it then reports `exit`. */
  stop(): Promise<void>;
}

/**
 * A message as the agent receives it: newlines stay (Ctrl+J is a newline in Claude Code), tabs
 * become spaces, and every other control character goes, so a message can't press Enter, Escape
 * or send terminal escape sequences.
 */
export function cleanMessage(message: string): string {
  return message.replaceAll("\t", " ").replace(/(?!\n)\p{Cc}/gu, "");
}

// After a cancel, the next message waits this long: Escape followed at once by text reads as
// Alt+<key> in a terminal, and the cancel would be lost.
const AFTER_CANCEL_MS = 300;

/**
 * The turn boundary every runtime shares: messages wait for the running turn to end, then go in
 * one per turn. `deliver` hands one message to the agent; the runtime reports back with
 * {@link Turns.started} and {@link Turns.ended}.
 */
export class Turns {
  #state: "starting" | "running" | "idle" | "delivering" = "starting";
  readonly #queue: string[] = [];
  readonly #deliver: (message: string) => void;
  readonly #onEvent: OnRuntimeEvent;
  #timer: NodeJS.Timeout | undefined;

  constructor(deliver: (message: string) => void, onEvent: OnRuntimeEvent) {
    this.#deliver = deliver;
    this.#onEvent = onEvent;
  }

  /** Whether a turn is running. */
  get running(): boolean {
    return this.#state === "running";
  }

  /** Delivers the cleaned message now if the agent is idle, otherwise after the running turn. */
  inject(message: string): InjectResult {
    const clean = cleanMessage(message);
    if (clean.trim() === "") return { ok: false, reason: "empty" };
    this.#queue.push(clean);
    if (this.#state === "idle" && this.#timer === undefined) this.#next();
    return { ok: true };
  }

  /** The agent began a turn. */
  started(): void {
    this.#state = "running";
    this.#onEvent({ kind: "turn_start" });
  }

  /** The running turn ended; a report with no turn running (a late hook) is ignored. */
  ended(outcome: "ended" | "cancelled"): void {
    if (this.#state !== "running") return;
    this.#state = "idle";
    this.#onEvent({ kind: "turn_end", outcome });
    if (outcome === "ended") {
      this.#next();
      return;
    }
    this.#timer = setTimeout(() => {
      this.#timer = undefined;
      if (this.#state === "idle") this.#next();
    }, AFTER_CANCEL_MS);
  }

  /** Drops any pending delivery; the runtime is going away. */
  close(): void {
    clearTimeout(this.#timer);
    this.#timer = undefined;
    this.#queue.length = 0;
  }

  #next(): void {
    const message = this.#queue.shift();
    if (message === undefined) return;
    this.#state = "delivering";
    this.#deliver(message);
  }
}
