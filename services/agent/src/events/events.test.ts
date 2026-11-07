import { readFileSync } from "node:fs";
import { expect, test } from "vitest";
import { z } from "zod";
import { createEmitter, InvalidEventError, type LaneEvent } from "./events.ts";
import { uuidv7, type Clock, type IdGen } from "./ids.ts";

const SESSION = "019a3f4e-7c21-7d10-9a3b-2f5e6c7d8a90";
const clock: Clock = { now: () => new Date("2026-11-07T10:00:00.000Z") };
const ids: IdGen = { next: () => "019a3f50-0000-7000-8000-000000000001" };

function emitted(event: LaneEvent): unknown {
  const lines: string[] = [];
  createEmitter({ sessionId: SESSION, clock, ids, write: (l) => lines.push(l) })(event);
  return JSON.parse(lines[0] ?? "null");
}

test("AC5 an emitted event has its envelope", () => {
  const event = emitted({ type: "progress", payload: { stage: "tests", note: "AC1 test added" } });
  expect(event).toEqual({
    messageId: "019a3f50-0000-7000-8000-000000000001",
    sessionId: SESSION,
    at: "2026-11-07T10:00:00.000Z",
    type: "progress",
    payload: { stage: "tests", note: "AC1 test added" },
  });
});

test("AC5 an event the contract rejects is never written", () => {
  const lines: string[] = [];
  const emit = createEmitter({ sessionId: SESSION, clock, ids, write: (l) => lines.push(l) });
  expect(() => {
    emit({ type: "progress", payload: { stage: "Not A Slug", note: "x" } });
  }).toThrow(InvalidEventError);
  expect(lines).toEqual([]);
});

// The examples' envelopes, so the emitter rebuilds each example exactly.
const example = z.object({
  messageId: z.string(),
  sessionId: z.string(),
  at: z.string(),
  type: z.enum(["ask", "worker_done", "progress", "compacted"]),
  payload: z.record(z.string(), z.unknown()),
});

function read(name: string): z.infer<typeof example> {
  const url = new URL(`../../../../api/examples/${name}`, import.meta.url);
  return example.parse(JSON.parse(readFileSync(url, "utf8")));
}

function emitLike(e: z.infer<typeof example>, write: (l: string) => void): void {
  const emit = createEmitter({
    sessionId: e.sessionId,
    clock: { now: () => new Date(e.at) },
    ids: { next: () => e.messageId },
    write,
  });
  // The emitter's validator is what this test checks, so the payload goes in unchecked.
  const unchecked: unknown = { type: e.type, payload: e.payload };
  // eslint-disable-next-line @typescript-eslint/consistent-type-assertions -- see above
  emit(unchecked as LaneEvent);
}

test("AC5 the emitter validates with the api/ schema file: its examples pass, its invalid ones throw", () => {
  for (const name of ["ask", "progress", "compacted", "worker-done"]) {
    const e = read(`agent-event.${name}.example.json`);
    const lines: string[] = [];
    emitLike(e, (l) => lines.push(l));
    expect(JSON.parse(lines[0] ?? "null")).toEqual({ ...e, at: new Date(e.at).toISOString() });
  }
  const invalid = read("invalid/agent-event.progress-with-payload-of-ask.json");
  expect(() => {
    emitLike(invalid, () => undefined);
  }).toThrow(InvalidEventError);
});

test("uuidv7 has the version and variant bits and the timestamp first", () => {
  const id = uuidv7(0x019a3f4e7c21, new Uint8Array(10).fill(0xff));
  expect(id).toBe("019a3f4e-7c21-7fff-bfff-ffffffffffff");
  expect(uuidv7(0, new Uint8Array(10))).toBe("00000000-0000-7000-8000-000000000000");
});
