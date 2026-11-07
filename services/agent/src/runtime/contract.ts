// The AgentRuntime contract suite (ADR 0049 §2.2): every implementation, the fake included, runs
// it from its own test file.
import { expect, test } from "vitest";
import type { AgentRuntime, OnRuntimeEvent, RuntimeEvent } from "./runtime.ts";
import { RESUME_FAILED } from "./runtime.ts";

/** Builds a runtime under test that reports to `onEvent`. */
export type MakeRuntime = (onEvent: OnRuntimeEvent) => Promise<AgentRuntime>;

/**
 * Sets up one lane (its own state: sessions, Claude home) and returns how to build runtimes in
 * it. Runtimes built from one lane share its sessions, as two pods of one lane do.
 */
export type MakeLane = () => MakeRuntime;

/** The session id every contract test uses. */
export const SESSION = "019a3f4e-7c21-7d10-9a3b-2f5e6c7d8a90";

/** Resolves once `ready()` holds; rejects after `ms`. */
export async function until(ready: () => boolean, ms = 10_000): Promise<void> {
  const deadline = Date.now() + ms;
  while (!ready()) {
    if (Date.now() > deadline) throw new Error("timed out");
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
}

const kinds = (events: RuntimeEvent[]) => events.map((e) => e.kind);
const ends = (events: RuntimeEvent[]) => events.filter((e) => e.kind === "turn_end");
const notes = (events: RuntimeEvent[]) =>
  events.flatMap((e) =>
    e.kind === "lane" && e.event.type === "progress" ? [e.event.payload.note] : [],
  );

async function withRuntime(
  make: MakeRuntime,
  body: (r: AgentRuntime, events: RuntimeEvent[]) => Promise<void>,
) {
  const events: RuntimeEvent[] = [];
  const runtime = await make((e) => events.push(e));
  try {
    await body(runtime, events);
  } finally {
    await runtime.stop();
  }
}

function turns(name: string, lane: MakeLane): void {
  test(`${name}: AC1 events arrive in order`, () =>
    withRuntime(lane(), async (runtime, events) => {
      await runtime.start("one");
      await until(() => ends(events).length === 1);
      expect(kinds(events)).toEqual(["turn_start", "lane", "turn_end"]);
      expect(events[2]).toEqual({ kind: "turn_end", outcome: "ended" });
    }));

  test(`${name}: AC1 inject during a turn is delivered only after the turn ends`, () =>
    withRuntime(lane(), async (runtime, events) => {
      await runtime.start("slow one");
      await until(() => events.length > 0);
      expect(runtime.inject("two")).toEqual({ ok: true });
      await until(() => ends(events).length === 2);
      expect(kinds(events)).toEqual([
        ...["turn_start", "lane", "turn_end"],
        ...["turn_start", "lane", "turn_end"],
      ]);
      expect(notes(events)[1]).toContain("two");
    }));

  test(`${name}: AC1 a message that would type nothing is refused`, () =>
    withRuntime(lane(), async (runtime, events) => {
      await runtime.start("one");
      await until(() => ends(events).length === 1);
      for (const empty of ["", "  \n", "\x1b", "\r\x03"]) {
        expect(runtime.inject(empty)).toEqual({ ok: false, reason: "empty" });
      }
      expect(runtime.inject("two")).toEqual({ ok: true });
      await until(() => ends(events).length === 2);
      expect(notes(events)[1]).toContain("two");
    }));
}

function interrupts(name: string, lane: MakeLane): void {
  test(`${name}: AC1 interrupt ends the turn with cancelled`, () =>
    withRuntime(lane(), async (runtime, events) => {
      await runtime.start("slow one");
      await until(() => events.length > 0);
      runtime.interrupt();
      expect(ends(events)).toEqual([{ kind: "turn_end", outcome: "cancelled" }]);
      await new Promise((resolve) => setTimeout(resolve, 600));
      expect(ends(events)).toHaveLength(1);
    }));

  test(`${name}: AC1 interrupt with a queued message cancels the turn, then delivers it`, () =>
    withRuntime(lane(), async (runtime, events) => {
      await runtime.start("slow one");
      await until(() => events.length > 0);
      runtime.inject("two");
      runtime.interrupt();
      await until(() => ends(events).length === 2);
      expect(ends(events).map((e) => e.outcome)).toEqual(["cancelled", "ended"]);
      expect(notes(events)[1]).toContain("two");
      await new Promise((resolve) => setTimeout(resolve, 600));
      expect(ends(events)).toHaveLength(2);
    }));
}

function resumes(name: string, lane: MakeLane): void {
  test(`${name}: AC1 resume continues the session with the prompt as the next turn`, async () => {
    const make = lane();
    await withRuntime(make, async (runtime, events) => {
      await runtime.start("one");
      await until(() => ends(events).length === 1);
    });
    await withRuntime(make, async (runtime, events) => {
      await runtime.resume(SESSION, "go on");
      await until(() => ends(events).length === 1);
      expect(kinds(events)).toEqual(["turn_start", "lane", "turn_end"]);
    });
  });

  test(`${name}: AC1 resume of a session that can't be resumed starts a new one (ADR 0045 §4)`, () =>
    withRuntime(lane(), async (runtime, events) => {
      await runtime.resume(SESSION, "go on");
      await until(() => ends(events).length === 1);
      expect(events[0]).toEqual({ kind: "lane", event: RESUME_FAILED });
      expect(kinds(events).slice(1)).toEqual(["turn_start", "lane", "turn_end"]);
    }));
}

/** Registers the contract tests for one implementation. */
export function runtimeContract(name: string, lane: MakeLane): void {
  turns(name, lane);
  interrupts(name, lane);
  resumes(name, lane);
}
