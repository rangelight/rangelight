import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, expect, test } from "vitest";
import { childEnv } from "../config.ts";
import { createEmitter } from "../events/events.ts";
import { CliRuntime, SIGN_IN_QUESTION } from "./cli.ts";
import { runtimeContract, SESSION, until } from "./contract.ts";
import { RESUME_FAILED, type OnRuntimeEvent, type RuntimeEvent } from "./runtime.ts";

const BIN = join(import.meta.dirname, "testdata", "fake-claude.sh");
const dirs: string[] = [];

afterEach(() => {
  for (const d of dirs.splice(0)) rmSync(d, { recursive: true, force: true });
});

function home(signedIn: boolean): string {
  const dir = mkdtempSync(join(tmpdir(), "rl-agent-home-"));
  dirs.push(dir);
  if (signedIn) writeFileSync(join(dir, "signed-in"), "");
  return dir;
}

function cli(claudeConfigDir: string, onEvent: OnRuntimeEvent, cwd = claudeConfigDir): CliRuntime {
  return new CliRuntime({
    bin: BIN,
    cwd,
    sessionId: SESSION,
    claudeConfigDir,
    env: childEnv(),
    onEvent,
    signInPollMs: 20,
  });
}

function launches(dir: string): string[][] {
  return readFileSync(join(dir, "launches"), "utf8")
    .trim()
    .split("\n")
    .map((l) => l.split("\x1f"));
}

/** Runs one runtime until `done` holds, then stops it. */
async function run(
  dir: string,
  go: (r: CliRuntime) => Promise<void>,
  done: (events: RuntimeEvent[]) => boolean,
): Promise<RuntimeEvent[]> {
  const events: RuntimeEvent[] = [];
  const runtime = cli(dir, (e) => events.push(e));
  await go(runtime);
  await until(() => done(events));
  await runtime.stop();
  return events;
}

const turnEnded = (events: RuntimeEvent[]) => events.some((e) => e.kind === "turn_end");

runtimeContract("cli", () => {
  const dir = home(true);
  return (onEvent) => Promise.resolve(cli(dir, onEvent));
});

test("cli: AC6 without a login it asks the member to sign in and waits, then starts", async () => {
  const dir = home(false);
  const events: RuntimeEvent[] = [];
  const runtime = cli(dir, (e) => events.push(e));
  const started = runtime.start("one");
  await until(() => events.length > 0);
  expect(events).toEqual([
    { kind: "lane", event: { type: "ask", payload: { question: SIGN_IN_QUESTION } } },
  ]);
  expect(SIGN_IN_QUESTION).toContain("sign in to Claude");
  await new Promise((resolve) => setTimeout(resolve, 100));
  expect(() => launches(dir)).toThrow();
  writeFileSync(join(dir, "signed-in"), "");
  await started;
  await until(() => turnEnded(events));
  await runtime.stop();
  expect(events.filter((e) => e.kind === "lane" && e.event.type === "ask")).toHaveLength(1);
});

test("cli: AC6 stop while waiting for sign-in ends the runtime without launching claude", async () => {
  const dir = home(false);
  const events: RuntimeEvent[] = [];
  const runtime = cli(dir, (e) => events.push(e));
  const started = runtime.start("one");
  await until(() => events.length > 0);
  await runtime.stop();
  await started;
  writeFileSync(join(dir, "signed-in"), "");
  await new Promise((resolve) => setTimeout(resolve, 100));
  expect(events.at(-1)).toEqual({ kind: "exit", code: 143 });
  expect(() => launches(dir)).toThrow();
});

test("cli: AC6 start and resume launch claude with the session id, the hooks and the prompt", async () => {
  const dir = home(true);
  await run(dir, (r) => r.start("one"), turnEnded);
  await run(dir, (r) => r.resume(SESSION, "two"), turnEnded);
  const [start, resume] = launches(dir);
  expect(start?.slice(-3)).toEqual(["--session-id", SESSION, "one"]);
  expect(resume?.slice(-3)).toEqual(["--resume", SESSION, "two"]);
  expect(start?.slice(2, 4)).toEqual(["--permission-mode", "bypassPermissions"]);
  // A path no other process in the pod knows, so it can't post forged hooks without reading argv.
  const url = /"url":"(http:\/\/127\.0\.0\.1:\d+\/hooks\/[0-9a-f-]{36})"/.exec(start?.[1] ?? "");
  expect(url).not.toBeNull();
  expect(start?.[1]).toContain(`"Stop":[{"hooks":[{"type":"http","url":"${url?.[1] ?? ""}"}]}]`);
});

test("cli: AC6 a resume claude can't do relaunches once as a new session with the prompt", async () => {
  const dir = home(true);
  const events = await run(dir, (r) => r.resume(SESSION, "go on"), turnEnded);
  expect(launches(dir).map((l) => l.slice(-3))).toEqual([
    ["--resume", SESSION, "go on"],
    ["--session-id", SESSION, "go on"],
  ]);
  expect(events[0]).toEqual({ kind: "lane", event: RESUME_FAILED });
  // The failed resume's exit is the runtime's business: only the final stop is reported.
  expect(events.filter((e) => e.kind === "exit")).toHaveLength(1);
});

test("cli: AC5 AC6 hook events become lane events the emitter accepts", async () => {
  const events = await run(home(true), (r) => r.start("one"), turnEnded);
  const lane = events.flatMap((e) => (e.kind === "lane" ? [e.event] : []));
  expect(lane).toEqual([{ type: "progress", payload: { stage: "one", note: "ran one" } }]);
  const lines: string[] = [];
  const emit = createEmitter({
    sessionId: SESSION,
    clock: { now: () => new Date("2026-11-07T10:00:00Z") },
    ids: { next: () => "019a3f50-0000-7000-8000-000000000001" },
    write: (l) => lines.push(l),
  });
  for (const e of lane) emit(e);
  expect(lines).toHaveLength(lane.length);
});

test("cli: AC6 the runtime reports claude's exit code", async () => {
  const events = await run(
    home(true),
    (r) => r.start("exit"),
    (e) => e.length > 0,
  );
  expect(events).toEqual([{ kind: "exit", code: 3 }]);
});

test("cli: a launch that can't spawn is reported and ends the runtime, it doesn't throw", async () => {
  const dir = home(true);
  const events: RuntimeEvent[] = [];
  const runtime = cli(dir, (e) => events.push(e), join(dir, "no-such-worktree"));
  await runtime.start("one");
  await until(() => events.some((e) => e.kind === "exit"));
  await runtime.stop();
  const [report, exit] = events;
  expect(
    report?.kind === "lane" && report.event.type === "progress" && report.event.payload,
  ).toEqual({
    stage: "runtime",
    note: "claude failed to start: spawn script ENOENT",
  });
  expect(exit).toEqual({ kind: "exit", code: 127 });
  expect(events).toHaveLength(2);
});
