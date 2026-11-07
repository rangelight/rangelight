import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, onTestFinished, test } from "vitest";
import { until } from "../runtime/contract.ts";
import { childEnv } from "../config.ts";
import type { LaneEvent } from "../events/events.ts";
import { devCommands, startDevCommands } from "./dev.ts";

function lane(services: object[]): { profile: string; workdir: string } {
  const dir = mkdtempSync(join(tmpdir(), "rl-agent-dev-"));
  onTestFinished(() => {
    rmSync(dir, { recursive: true, force: true });
  });
  const workdir = join(dir, "repo");
  mkdirSync(join(workdir, "src", "frontend"), { recursive: true });
  // The worktree's own profile says something else; it must never be used (ADR 0041).
  writeFileSync(
    join(workdir, "rangelight.yaml"),
    "spec: {services: [{name: frontend, underChange: {command: 'echo worktree >> ran'}}]}\n",
  );
  const profile = join(dir, "profile.json");
  writeFileSync(
    profile,
    JSON.stringify({ apiVersion: "rangelight.dev/v1alpha1", spec: { services } }),
  );
  return { profile, workdir };
}

const frontend = {
  name: "frontend",
  serviceName: "frontend",
  port: 8080,
  underChange: {
    repoPath: "src/frontend",
    command: 'echo "$GREETING" >> ran',
    env: { GREETING: "profile" },
    park: { values: {} },
  },
};

test("AC7 the profile's dev command runs once, in its repo path, from the mounted profile only", async () => {
  const { profile, workdir } = lane([frontend, { name: "cart", serviceName: "cart", port: 1 }]);
  const commands = devCommands(profile, ["frontend"], workdir);
  expect(commands).toEqual([
    {
      service: "frontend",
      cwd: join(workdir, "src/frontend"),
      command: frontend.underChange.command,
      env: { GREETING: "profile" },
    },
  ]);
  const events: LaneEvent[] = [];
  startDevCommands(commands, childEnv(), (e) => events.push(e));
  await until(() => events.length === 1);
  expect(readFileSync(join(workdir, "src/frontend/ran"), "utf8")).toBe("profile\n");
  expect(events).toEqual([
    { type: "progress", payload: { stage: "dev", note: "frontend dev command exited (code 0)" } },
  ]);
});

test("AC7 a service under change without a dev command, or a path out of the repo, is refused", () => {
  const { profile, workdir } = lane([frontend, { name: "cart", serviceName: "cart", port: 1 }]);
  expect(() => devCommands(profile, ["cart"], workdir)).toThrow(/cart has no underChange/);
  const escape = lane([
    { ...frontend, underChange: { ...frontend.underChange, repoPath: "src/../../etc" } },
  ]);
  expect(() => devCommands(escape.profile, ["frontend"], escape.workdir)).toThrow(
    /inside the repo/,
  );
});
