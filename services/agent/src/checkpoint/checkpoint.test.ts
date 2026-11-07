import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { expect, test } from "vitest";
import { makeMirror, SESSION, sh } from "../workspace/testdata/mirror.ts";
import { prepareWorkspace } from "../workspace/workspace.ts";
import { checkpointRef, Checkpoints, lastCheckpoint, workBranchRef } from "./checkpoint.ts";

test("AC3 checkpoint and work branch refs", () => {
  expect(checkpointRef("s1", 3)).toBe("refs/rangelight/sessions/s1/cp/3");
  expect(workBranchRef("s1")).toBe("refs/heads/rangelight/s1/work");
});

test("AC3 the last checkpoint number from ls-remote output", () => {
  const lines = [
    "a\trefs/rangelight/sessions/s1/cp/2",
    "b\trefs/rangelight/sessions/s1/cp/10",
    "c\trefs/heads/rangelight/s1/work",
  ];
  expect(lastCheckpoint("s1", lines.join("\n"))).toBe(10);
  expect(lastCheckpoint("s1", "")).toBe(0);
});

test("AC3 a checkpoint saves the whole worktree without touching HEAD, the index or the files", async () => {
  const m = makeMirror();
  await prepareWorkspace(m);
  writeFileSync(join(m.workdir, "README.md"), "changed\n");
  writeFileSync(join(m.workdir, "new.txt"), "untracked\n");
  const status = sh(m.workdir, "status", "--porcelain");
  const head = sh(m.workdir, "rev-parse", "HEAD");
  const cps = new Checkpoints({ sessionId: SESSION, workdir: m.workdir, last: 4, git: m.git });

  expect(await cps.save()).toBe(checkpointRef(SESSION, 5));
  expect(sh(m.workdir, "status", "--porcelain")).toBe(status);
  expect(sh(m.workdir, "rev-parse", "HEAD")).toBe(head);
  expect(sh(m.pushUrl, "show", `${checkpointRef(SESSION, 5)}:new.txt`)).toBe("untracked");
  expect(sh(m.pushUrl, "rev-parse", `${checkpointRef(SESSION, 5)}^`)).toBe(head);
  expect(sh(m.pushUrl, "rev-parse", workBranchRef(SESSION))).toBe(head);

  expect(await cps.save()).toBeNull();
  writeFileSync(join(m.workdir, "new.txt"), "more\n");
  const [a, b] = await Promise.all([cps.save(), cps.save()]);
  expect([a, b]).toEqual([checkpointRef(SESSION, 6), null]);
});

test("a checkpoint pushes the session's work branch, not whatever branch the model is on", async () => {
  const m = makeMirror();
  await prepareWorkspace(m);
  const work = sh(m.workdir, "rev-parse", "HEAD");
  sh(m.workdir, "checkout", "-q", "-b", "elsewhere");
  writeFileSync(join(m.workdir, "x.txt"), "x\n");
  sh(m.workdir, "add", ".");
  sh(m.workdir, "commit", "-qm", "elsewhere");
  const cps = new Checkpoints({ sessionId: SESSION, workdir: m.workdir, last: 0, git: m.git });
  await cps.save();
  expect(sh(m.pushUrl, "rev-parse", workBranchRef(SESSION))).toBe(work);
});
