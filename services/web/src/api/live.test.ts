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

test("a report that names the session's PR links it, and the first one stays", () => {
  const pr = (seq: bigint, pullRequestUrl: string) =>
    create(EventSchema, {
      sessionId: "s1",
      seq,
      change: { kind: { case: "phaseChanged", value: { phase: Phase.RUNNING, pullRequestUrl } } },
    });
  const [opened] = applyEvent([session], pr(4n, "https://github.com/acme/shop/pull/7"));
  expect(opened?.pullRequestUrl).toBe("https://github.com/acme/shop/pull/7");
  expect(opened && applyEvent([opened], pr(5n, ""))[0]?.pullRequestUrl).toBe(
    "https://github.com/acme/shop/pull/7",
  );
});

test("RAN-63 an ask makes the session wait for you; the next turn's progress ends it", () => {
  const agent = (
    seq: bigint,
    kind:
      | { case: "ask"; value: { question: string } }
      | { case: "progress"; value: { stage: string; note: string } }
      | { case: "compacted"; value: { trigger: string; summary: string } },
  ) =>
    create(EventSchema, {
      sessionId: "s1",
      seq,
      change: { kind: { case: "agentEvent", value: { kind } } },
    });
  const [waiting] = applyEvent(
    [session],
    agent(4n, { case: "ask", value: { question: "Sign in?" } }),
  );
  expect(waiting?.question).toBe("Sign in?");
  const kept =
    waiting &&
    applyEvent(
      [waiting],
      agent(5n, { case: "compacted", value: { trigger: "auto", summary: "s" } }),
    )[0];
  expect(kept?.question).toBe("Sign in?");
  const going =
    kept &&
    applyEvent([kept], agent(6n, { case: "progress", value: { stage: "turn", note: "n" } }))[0];
  expect(going?.question).toBe("");
  const again =
    going && applyEvent([going], agent(7n, { case: "ask", value: { question: "q" } }))[0];
  expect(again && applyEvent([again], warm(8n))[0]?.question).toBe("");
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
