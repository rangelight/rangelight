import { expect, test } from "vitest";
import type { LaneEvent } from "../events/events.ts";
import { SESSION, until } from "./contract.ts";
import { FakeRuntime } from "./fake.ts";
import { laneHandler } from "./lane.ts";

test("every turn end and the exit push a checkpoint; lane events are emitted", async () => {
  const emitted: LaneEvent[] = [];
  let saves = 0;
  let exited: number | undefined;
  const onEvent = laneHandler({
    emit: (e) => emitted.push(e),
    checkpoint: () => {
      saves += 1;
      return Promise.resolve();
    },
    exit: (code) => {
      exited = code;
    },
  });
  const runtime = new FakeRuntime({ onEvent, sessionId: SESSION, sessions: new Set() });
  await runtime.start("one");
  runtime.inject("two");
  await until(() => saves === 2);
  await runtime.stop();
  await until(() => exited === 143);
  expect(saves).toBe(3);
  expect(emitted.map((e) => e.type)).toEqual(["progress", "progress"]);
});

test("a failed checkpoint is reported and the lane goes on", async () => {
  const emitted: LaneEvent[] = [];
  const onEvent = laneHandler({
    emit: (e) => emitted.push(e),
    checkpoint: () => Promise.reject(new Error("remote: rangelight: push refused")),
    exit: () => undefined,
  });
  onEvent({ kind: "turn_end", outcome: "ended" });
  await until(() => emitted.length === 1);
  expect(emitted).toEqual([
    {
      type: "progress",
      payload: { stage: "checkpoint", note: "checkpoint failed: remote: rangelight: push refused" },
    },
  ]);
});
