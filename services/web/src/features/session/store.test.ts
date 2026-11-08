import { create } from "@bufbuild/protobuf";
import { DesiredState } from "@gen/proto/rangelight/runner/v1alpha1/runner_pb.ts";
import { EventSchema, SessionSchema } from "@gen/proto/rangelight/session/v1alpha1/session_pb.ts";
import { expect, test, vi } from "vitest";

vi.mock("@/api/client", () => ({ sessions: {} }));
const { addEvent, caughtUp } = await import("./store");

const event = (id: string, seq: bigint, state: DesiredState) =>
  create(EventSchema, {
    id,
    sessionId: "s1",
    seq,
    change: { kind: { case: "stateChanged", value: { state } } },
  });

test("a session read after the watch opened catches up on what the watch already delivered", () => {
  const read = create(SessionSchema, { id: "s1", state: DesiredState.HOT, generation: 1n });
  const delivered = [event("e2", 2n, DesiredState.WARM), event("e3", 3n, DesiredState.COLD)];
  expect(caughtUp(read, delivered)).toMatchObject({ state: DesiredState.COLD, generation: 3n });
  // The read already held seq 2: only seq 3 changes it.
  const newer = create(SessionSchema, { id: "s1", state: DesiredState.WARM, generation: 2n });
  expect(caughtUp(newer, delivered.slice(0, 1))).toBe(newer);
});

test("the feed dedupes on the event id", () => {
  const e = event("e2", 2n, DesiredState.WARM);
  expect(addEvent(addEvent([], e), e)).toHaveLength(1);
});
