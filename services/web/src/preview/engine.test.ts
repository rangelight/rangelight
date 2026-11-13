import { expect, test } from "vitest";
import { advance, reduce, TICKS_PER_STAGE } from "./engine";
import { initial } from "./world";

const laneOf = (w: ReturnType<typeof initial>, key: string) => w.lanes.find((l) => l.key === key);

test("deciding closes the decision, tells its lane, and lets a lane held on it go on", () => {
  const w = reduce(initial(), { type: "decide", id: "d-bounce", key: 1 });
  expect(w.decisions.find((d) => d.id === "d-bounce")).toMatchObject({
    status: "decided",
    chosen: 1,
    decidedBy: "you",
  });
  const lane = laneOf(w, "ACME-41");
  expect(lane?.status).toBe("running");
  const told = lane?.feed.at(-1);
  expect(told?.kind === "outcome" && told.text).toMatch(/^You chose/);
  // A second decision on a closed one changes nothing.
  expect(reduce(w, { type: "decide", id: "d-bounce", key: 2 })).toBe(w);
});

test("time moves running lanes through their stages, not those held on you or paused", () => {
  let w = initial();
  for (let i = 0; i < TICKS_PER_STAGE; i++) w = advance(w);
  // ACME-41 waits on your decision; ACME-42 moves from review to verify; ACME-44 is done.
  expect(laneOf(w, "ACME-41")?.stage).toBe("qa");
  expect(laneOf(w, "ACME-42")).toMatchObject({
    stage: "verify",
    stages: { review: "done", verify: "active" },
  });
  expect(laneOf(w, "ACME-44")).toMatchObject({ status: "done" });
  const paused = reduce(initial(), { type: "pause", lane: "ACME-42" });
  expect(laneOf(advance(paused), "ACME-42")?.ticks).toBe(0);
});

test("say, approve, fork and undo each change the sample world once", () => {
  let w = reduce(initial(), { type: "say", lane: "ACME-42", text: "hi" });
  expect(w.lanes.find((l) => l.key === "ACME-42")?.feed.at(-1)).toMatchObject({
    kind: "human",
    text: "hi",
  });
  // A real session's id reaches no feed: sample text never speaks for a real lane.
  expect(reduce(w, { type: "say", lane: "session-1", text: "hi" }).lanes).toEqual(w.lanes);
  w = reduce(w, { type: "approve", review: "sample/212" });
  expect(w.reviews.find((r) => r.id === "sample/212")?.approved).toBe(true);
  expect(reduce(w, { type: "approve", review: "sample/212" })).toBe(w);
  w = reduce(w, { type: "fork", lane: "ACME-42" });
  expect(laneOf(w, "ACME-42·b")?.status).toBe("running");
  expect(reduce(w, { type: "undo" }).undone).toBe(true);
});

test("RAN-85 a waiting ticket starts a lane at the Grill, once; reset is the world as it starts", () => {
  const w = reduce(initial(), { type: "start", key: "ACME-45", title: "Webhook retries" });
  expect(laneOf(w, "ACME-45")).toMatchObject({
    stage: "grill",
    status: "running",
    stages: { grill: "active", build: "todo" },
  });
  expect(reduce(w, { type: "start", key: "ACME-45", title: "again" })).toBe(w);
  expect(reduce(w, { type: "reset" })).toEqual(initial());
});
