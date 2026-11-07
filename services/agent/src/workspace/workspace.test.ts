import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { expect, test } from "vitest";
import { childEnv } from "../config.ts";
import type { Git } from "./git.ts";
import { makeMirror, SESSION, sh, TOKEN } from "./testdata/mirror.ts";
import { cloneArgs, credentialHelper, prepareWorkspace, workBranch } from "./workspace.ts";

function recording(
  inner: Git,
  calls: { args: string[]; env: unknown }[],
  failLsRemote = false,
): Git {
  return (args, options) => {
    calls.push({ args, env: options.env });
    if (failLsRemote && args[0] === "ls-remote") return Promise.reject(new Error("404"));
    return inner(args, options);
  };
}

test("AC4 the clone's argv: base branch from the mirror URL, pushes to the session's repo", () => {
  const m = makeMirror();
  expect(
    cloneArgs({
      ...m,
      mirrorUrl: "http://mirror",
      cloneUrl: "http://mirror/demo.git",
      pushUrl: "http://mirror/session/demo.git",
    }),
  ).toEqual([
    "clone",
    "--branch",
    "main",
    "-c",
    `credential.http://mirror.helper=${credentialHelper(m.tokenFile)}`,
    "-c",
    "remote.origin.pushurl=http://mirror/session/demo.git",
    "-c",
    "push.default=upstream",
    "-c",
    "user.name=Rangelight agent",
    "-c",
    "user.email=agent@rangelight.dev",
    "-c",
    `branch.rangelight/${SESSION}/work.remote=origin`,
    "-c",
    `branch.rangelight/${SESSION}/work.merge=refs/heads/rangelight/${SESSION}/work`,
    "--",
    "http://mirror/demo.git",
    m.workdir,
  ]);
});

test("AC4 the token is never in argv or the environment; git reads it from the file at use time", async () => {
  const m = makeMirror();
  const calls: { args: string[]; env: unknown }[] = [];
  await prepareWorkspace({ ...m, git: recording(m.git, calls) });
  expect(calls.length).toBeGreaterThan(0);
  expect(JSON.stringify(calls)).not.toContain(TOKEN);
  expect(fill(m.workdir, "http", "mirror.test:8080")).toContain(`password=${TOKEN}\n`);
  writeFileSync(m.tokenFile, "rotated");
  expect(fill(m.workdir, "http", "mirror.test:8080")).toContain("password=rotated\n");
});

/** What git would send as credentials to protocol://host from the worktree, or "" for none. */
function fill(cwd: string, protocol: string, host: string): string {
  try {
    return execFileSync("git", ["credential", "fill"], {
      cwd,
      env: { ...childEnv(), GIT_TERMINAL_PROMPT: "0" },
      input: `protocol=${protocol}\nhost=${host}\n\n`,
      stdio: ["pipe", "pipe", "ignore"],
    }).toString();
  } catch {
    return "";
  }
}

test("AC4 only the mirror gets the token: any other host the worktree fetches from gets none", async () => {
  const m = makeMirror();
  await prepareWorkspace(m);
  expect(fill(m.workdir, "http", "mirror.test:8080")).toContain(`password=${TOKEN}`);
  for (const [protocol, host] of [
    ["https", "evil.example"],
    ["http", "mirror.test:9999"],
  ] as const) {
    expect(fill(m.workdir, protocol, host)).not.toContain(TOKEN);
  }
});

test("AC4 a new session gets a work branch from the base branch, pushed at once", async () => {
  const m = makeMirror();
  const ws = await prepareWorkspace(m);
  expect(ws).toEqual({ resumed: false, lastCheckpoint: 0 });
  expect(sh(m.workdir, "branch", "--show-current")).toBe(workBranch(SESSION));
  expect(sh(m.pushUrl, "rev-parse", `rangelight/${SESSION}/work`)).toBe(
    sh(m.cloneUrl, "rev-parse", "main"),
  );
});

test("AC4 a mirror that never saw the session (404) means a new session", async () => {
  const m = makeMirror();
  const ws = await prepareWorkspace({ ...m, git: recording(m.git, [], true) });
  expect(ws.resumed).toBe(false);
});

test("AC4 a reopened session checks out its work branch and continues the checkpoint numbers", async () => {
  const m = makeMirror();
  const other = join(m.root, "other");
  sh(m.root, "clone", "-q", m.cloneUrl, other);
  writeFileSync(join(other, "work.txt"), "earlier work\n");
  sh(other, "add", ".");
  sh(other, "-c", "user.name=t", "-c", "user.email=t@example.com", "commit", "-qm", "work");
  const head = sh(other, "rev-parse", "HEAD");
  for (const ref of [
    `refs/heads/rangelight/${SESSION}/work`,
    `refs/rangelight/sessions/${SESSION}/cp/1`,
    `refs/rangelight/sessions/${SESSION}/cp/2`,
  ]) {
    sh(other, "push", "-q", m.pushUrl, `HEAD:${ref}`);
  }
  expect(await prepareWorkspace(m)).toEqual({ resumed: true, lastCheckpoint: 2 });
  expect(sh(m.workdir, "rev-parse", "HEAD")).toBe(head);
});

test("AC4 a reopened session gets back the uncommitted work of its last checkpoint", async () => {
  const m = makeMirror();
  const other = join(m.root, "other");
  sh(m.root, "clone", "-q", m.cloneUrl, other);
  const head = sh(other, "rev-parse", "HEAD");
  sh(other, "push", "-q", m.pushUrl, `HEAD:refs/heads/rangelight/${SESSION}/work`);
  writeFileSync(join(other, "README.md"), "edited\n");
  writeFileSync(join(other, "draft.txt"), "unsaved\n");
  sh(other, "add", "-A");
  sh(other, "-c", "user.name=t", "-c", "user.email=t@example.com", "commit", "-qm", "checkpoint 3");
  sh(other, "push", "-q", m.pushUrl, `HEAD:refs/rangelight/sessions/${SESSION}/cp/3`);

  expect(await prepareWorkspace(m)).toEqual({ resumed: true, lastCheckpoint: 3 });
  expect(sh(m.workdir, "rev-parse", "HEAD")).toBe(head);
  expect(readFileSync(join(m.workdir, "draft.txt"), "utf8")).toBe("unsaved\n");
  expect(sh(m.workdir, "status", "--short", "--porcelain").split("\n")).toEqual([
    "M README.md",
    "?? draft.txt",
  ]);
  expect(sh(m.workdir, "diff", "--cached", "--name-only")).toBe("");
});

test("AC4 a checkpoint not taken on the work branch's head is not restored", async () => {
  const m = makeMirror();
  const other = join(m.root, "other");
  sh(m.root, "clone", "-q", m.cloneUrl, other);
  writeFileSync(join(other, "draft.txt"), "stale\n");
  sh(other, "add", "-A");
  sh(other, "-c", "user.name=t", "-c", "user.email=t@example.com", "commit", "-qm", "checkpoint 1");
  sh(other, "push", "-q", m.pushUrl, `HEAD:refs/rangelight/sessions/${SESSION}/cp/1`);
  writeFileSync(join(other, "work.txt"), "newer\n");
  sh(other, "add", "-A");
  sh(other, "-c", "user.name=t", "-c", "user.email=t@example.com", "commit", "-qm", "work");
  sh(other, "push", "-q", m.pushUrl, `HEAD:refs/heads/rangelight/${SESSION}/work`);

  await prepareWorkspace(m);
  expect(sh(m.workdir, "status", "--porcelain")).toBe("");
});

test("AC4 a worktree already on disk is kept, uncommitted work included", async () => {
  const m = makeMirror();
  await prepareWorkspace(m);
  writeFileSync(join(m.workdir, "draft.txt"), "unsaved\n");
  const calls: { args: string[]; env: unknown }[] = [];
  await prepareWorkspace({ ...m, git: recording(m.git, calls) });
  expect(calls.map((c) => c.args[0])).toEqual(["ls-remote"]);
  expect(
    existsSync(join(m.workdir, "draft.txt")) && readFileSync(join(m.workdir, "draft.txt"), "utf8"),
  ).toBe("unsaved\n");
});
