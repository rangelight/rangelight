import type { AgentRuntime, InjectResult, OnRuntimeEvent } from "./runtime.ts";
import { RESUME_FAILED, Turns } from "./runtime.ts";

/** Options for {@link FakeRuntime}. */
export interface FakeOptions {
  onEvent: OnRuntimeEvent;
  sessionId: string;
  /** The sessions that exist, shared by the fakes of one lane, as a Claude home is. */
  sessions: Set<string>;
}

/**
 * An in-memory {@link AgentRuntime} for tests. Each turn reports one progress event whose note is
 * the prompt, then ends; a prompt containing "slow" runs for 300 ms so tests can act mid-turn.
 */
export class FakeRuntime implements AgentRuntime {
  readonly #o: FakeOptions;
  readonly #turns: Turns;
  #timer: NodeJS.Timeout | undefined;

  constructor(options: FakeOptions) {
    this.#o = options;
    this.#turns = new Turns((message) => {
      this.#turn(message);
    }, options.onEvent);
  }

  start(prompt: string): Promise<void> {
    this.#o.sessions.add(this.#o.sessionId);
    this.#turn(prompt);
    return Promise.resolve();
  }

  resume(sessionId: string, prompt: string): Promise<void> {
    if (!this.#o.sessions.has(sessionId)) this.#o.onEvent({ kind: "lane", event: RESUME_FAILED });
    return this.start(prompt);
  }

  inject(message: string): InjectResult {
    return this.#turns.inject(message);
  }

  interrupt(): void {
    if (!this.#turns.running) return;
    clearTimeout(this.#timer);
    this.#turns.ended("cancelled");
  }

  stop(): Promise<void> {
    clearTimeout(this.#timer);
    this.#turns.close();
    this.#o.onEvent({ kind: "exit", code: 143 });
    return Promise.resolve();
  }

  #turn(prompt: string): void {
    setImmediate(() => {
      this.#turns.started();
      const note = prompt.slice(0, 200);
      this.#o.onEvent({
        kind: "lane",
        event: { type: "progress", payload: { stage: "turn", note } },
      });
      this.#timer = setTimeout(
        () => {
          this.#turns.ended("ended");
        },
        prompt.includes("slow") ? 300 : 10,
      );
    });
  }
}
