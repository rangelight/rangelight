import { create } from "@bufbuild/protobuf";
import { Code, ConnectError } from "@connectrpc/connect";
import { DesiredState, Phase } from "@gen/proto/rangelight/runner/v1alpha1/runner_pb.ts";
import { EventSchema, SessionSchema } from "@gen/proto/rangelight/session/v1alpha1/session_pb.ts";
import { expect, test, vi } from "vitest";

vi.mock("./client", () => ({ sessions: {} }));
const { applyEvent, describe, endOf } = await import("./live");

const session = create(SessionSchema, { id: "s1", state: DesiredState.HOT, generation: 3n });
const warm = (seq: bigint, sessionId = "s1") =>
  create(EventSchema, {
    sessionId,
    seq,
    change: { kind: { case: "stateChanged", value: { state: DesiredState.WARM } } },
  });

test("a newer event moves the session; repeats and other sessions' events don't", () => {
  expect(applyEvent([session], warm(4n))[0]).toMatchObject({
    state: DesiredState.WARM,
    generation: 4n,
  });
  expect(applyEvent([session], warm(3n))[0]).toBe(session);
  expect(applyEvent([session], warm(9n, "s2"))[0]).toBe(session);
});

test("a runner's report moves the session's phase", () => {
  const running = create(EventSchema, {
    sessionId: "s1",
    seq: 4n,
    change: { kind: { case: "phaseChanged", value: { phase: Phase.RUNNING } } },
  });
  expect(applyEvent([session], running)[0]).toMatchObject({
    phase: Phase.RUNNING,
    state: DesiredState.HOT,
    generation: 4n,
  });
});

test("a creation adds the session on top, once", () => {
  const created = create(EventSchema, {
    sessionId: "s2",
    seq: 1n,
    change: {
      kind: {
        case: "created",
        value: {
          ticketKey: "RAN-2",
          ticketUrl: "https://linear.app/x",
          repo: "shop",
          state: DesiredState.HOT,
        },
      },
    },
  });
  const list = applyEvent([session], created);
  expect(list.map((s) => s.id)).toEqual(["s2", "s1"]);
  expect(list[0]).toMatchObject({
    ticketKey: "RAN-2",
    ticketUrl: "https://linear.app/x",
    repo: "shop",
    generation: 1n,
  });
  expect(applyEvent(list, created)).toHaveLength(2);
});

test("ABORTED means events were missed; other errors stop the watch", () => {
  const aborted = new ConnectError("events_missed: reread", Code.Aborted);
  expect(endOf(aborted, false)).toBe("missed");
  expect(endOf(new ConnectError("x", Code.Unavailable), false)).toBe("stopped");
  expect(endOf(aborted, true)).toBe("cancelled");
  expect(describe(new ConnectError("store_unavailable", Code.Unavailable))).toBe(
    "Unavailable: store_unavailable",
  );
});
