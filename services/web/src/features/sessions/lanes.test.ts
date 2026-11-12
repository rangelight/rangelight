import { create } from "@bufbuild/protobuf";
import { DesiredState, Phase } from "@gen/proto/rangelight/runner/v1alpha1/runner_pb.ts";
import { SessionSchema } from "@gen/proto/rangelight/session/v1alpha1/session_pb.ts";
import { expect, test } from "vitest";
import { liveLane, notStarted } from "./lanes";

const s = (phase: Phase, state = DesiredState.HOT) => create(SessionSchema, { phase, state });

test("a live lane has started and isn't gone; Up next is what hasn't started and isn't torn down", () => {
  const live = [Phase.STARTING, Phase.RUNNING, Phase.HIBERNATING, Phase.HIBERNATED, Phase.WAKING];
  for (const phase of Object.values(Phase).filter((p) => typeof p === "number")) {
    expect(liveLane(s(phase)), Phase[phase]).toBe(live.includes(phase));
  }
  expect(notStarted(s(Phase.UNSPECIFIED))).toBe(true);
  expect(notStarted(s(Phase.QUEUED, DesiredState.WARM))).toBe(true);
  expect(notStarted(s(Phase.QUEUED, DesiredState.COLD))).toBe(false);
  expect(notStarted(s(Phase.RUNNING))).toBe(false);
  expect(notStarted(s(Phase.FAILED))).toBe(false);
});
