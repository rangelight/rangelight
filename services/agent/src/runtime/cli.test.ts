import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, join } from "node:path";
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
  // One line per launch; a launch with no arguments is an empty line.
  return readFileSync(join(dir, "launches"), "utf8")
    .replace(/\n$/, "")
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

/** A runtime whose PTY output collects in `output.text`. */
function watched(dir: string, events: RuntimeEvent[]) {
  const output = { text: "" };
  const runtime = new CliRuntime({
    bin: BIN,
    cwd: dir,
    sessionId: SESSION,
    claudeConfigDir: dir,
    env: childEnv(),
    onEvent: (e) => events.push(e),
    onOutput: (chunk) => (output.text += chunk.toString()),
    signInPollMs: 20,
  });
  return { runtime, output };
}

const typed = (runtime: CliRuntime, text: string) => {
  runtime.write(Buffer.from(text));
};

test("cli: AC8 without a login, claude runs with no prompt in the lane terminal for /login, then the session takes its place", async () => {
  const dir = home(false);
  const events: RuntimeEvent[] = [];
  const { runtime, output } = watched(dir, events);
  const started = runtime.start("one");
  await until(() => output.text.includes("Sign in: type /login"));
  expect(events).toEqual([
    { kind: "lane", event: { type: "ask", payload: { question: SIGN_IN_QUESTION } } },
  ]);
  expect(SIGN_IN_QUESTION).toContain("type /login");
  expect(SIGN_IN_QUESTION).toContain("answer Yes when Claude asks whether you trust the folder");
  // The browser's size carries over to the session's claude.
  await runtime.resize(100, 30);
  typed(runtime, "/login\r");
  await started;
  await until(() => turnEnded(events));
  typed(runtime, "size\r");
  await until(() => output.text.includes("30 100"));
  await runtime.stop();
  expect(launches(dir).map((l) => l.slice(-3))).toEqual([[""], ["--session-id", SESSION, "one"]]);
  expect(events.filter((e) => e.kind === "lane" && e.event.type === "ask")).toHaveLength(1);
});

test("cli: AC8 stop while waiting for sign-in ends the runtime without starting the session", async () => {
  const dir = home(false);
  const events: RuntimeEvent[] = [];
  const { runtime, output } = watched(dir, events);
  const started = runtime.start("one");
  await until(() => output.text.includes("Sign in"));
  await runtime.stop();
  await started;
  writeFileSync(join(dir, "signed-in"), "");
  await new Promise((resolve) => setTimeout(resolve, 100));
  expect(events.at(-1)).toEqual({ kind: "exit", code: 143 });
  expect(launches(dir)).toEqual([[""]]);
});

test("cli: AC7 a resize to the same size still makes claude redraw", async () => {
  const events: RuntimeEvent[] = [];
  const { runtime, output } = watched(home(true), events);
  await runtime.start("one");
  await until(() => turnEnded(events));
  typed(runtime, "size\r");
  await until(() => output.text.includes("50 200 0"));
  await runtime.resize(200, 50); // what a browser sends on connecting, unchanged
  typed(runtime, "size\r");
  await until(() => /50 200 [1-9]/.test(output.text));
  await runtime.stop();
});

test("cli: AC9 a turn the member types is a turn like any other, and their Escape cancels it", async () => {
  const events: RuntimeEvent[] = [];
  const { runtime } = watched(home(true), events);
  await runtime.start("one");
  await until(() => turnEnded(events));
  typed(runtime, "two\r");
  await until(() => events.filter((e) => e.kind === "turn_end").length === 2);
  // A message injected during the member's turn waits for it, even when the member cancels it.
  typed(runtime, "slow three\r");
  const ran = (note: string) =>
    events.some(
      (e) => e.kind === "lane" && e.event.type === "progress" && e.event.payload.note === note,
    );
  await until(() => ran("ran slow"));
  expect(runtime.inject("four")).toEqual({ ok: true });
  typed(runtime, "\x1b");
  await until(() => events.filter((e) => e.kind === "turn_end").length === 4);
  await runtime.stop();
  const turns = events.filter((e) => e.kind !== "exit");
  expect(
    turns.map((e) =>
      e.kind === "lane" && e.event.type === "progress" ? e.event.payload.note : e.kind,
    ),
  ).toEqual([
    "turn_start",
    "ran one",
    "turn_end",
    "turn_start",
    "ran two",
    "turn_end",
    "turn_start",
    "ran slow",
    "turn_end",
    "turn_start",
    "ran four",
    "turn_end",
  ]);
  expect(turns[8]).toEqual({ kind: "turn_end", outcome: "cancelled" });
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

test("cli: review 3 F1 keys and resizes during the sign-in swap, and after stop, never end the agent", async () => {
  const dir = home(false);
  const events: RuntimeEvent[] = [];
  const { runtime, output } = watched(dir, events);
  const started = runtime.start("one");
  await until(() => output.text.includes("Sign in"));
  typed(runtime, "/login\r");
  // The member keeps typing and resizing while the sign-in claude ends and the session's starts.
  const member = { typing: true };
  const keys = (async () => {
    while (member.typing) {
      typed(runtime, "x");
      void runtime.resize(90, 30);
      await new Promise((resolve) => setTimeout(resolve, 10));
    }
  })();
  await started;
  await until(() => turnEnded(events));
  member.typing = false;
  await keys;
  await runtime.stop();
  typed(runtime, "after stop");
  runtime.interrupt();
  await new Promise((resolve) => setTimeout(resolve, 100));
  expect(events.filter((e) => e.kind === "exit")).toHaveLength(1);
}, 15_000);

test("cli: review 3 F2 the last of two resizes wins, even inside a forced redraw", async () => {
  const events: RuntimeEvent[] = [];
  const { runtime, output } = watched(home(true), events);
  await runtime.start("one");
  await until(() => turnEnded(events));
  // The same size (a redraw, which waits 100 ms in between), then a new size inside that wait.
  await Promise.all([runtime.resize(200, 50), runtime.resize(120, 40)]);
  typed(runtime, "size\r");
  await until(() => /(^|\n)40 120 \d/.test(output.text));
  await runtime.stop();
});

test("cli: review 3 F4 a claude that ignores SIGTERM is killed, so stop() ends and nothing is left", async () => {
  const dir = home(true);
  const events: RuntimeEvent[] = [];
  const { runtime } = watched(dir, events);
  await runtime.start("stubborn");
  await until(() => turnEnded(events));
  const left = () => {
    try {
      execFileSync("pgrep", ["-f", `rl-stubborn-${basename(dir)}`]);
      return true;
    } catch {
      return false; // pgrep exits 1: no such process
    }
  };
  expect(left()).toBe(true);
  await runtime.stop();
  expect(events.at(-1)?.kind).toBe("exit");
  await until(() => !left(), 2_000);
}, 15_000);
