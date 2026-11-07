// A git mirror on disk for tests: <root>/<repo>.git with one commit on main, and the session's
// own <root>/session/<repo>.git, the layout services/git-mirror serves over HTTP.
import { execFileSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { onTestFinished } from "vitest";
import { childEnv } from "../../config.ts";
import { createGit } from "../git.ts";
import type { WorkspaceOptions } from "../workspace.ts";

export const SESSION = "019a3f4e-7c21-7d10-9a3b-2f5e6c7d8a90";
export const TOKEN = "fake-lane-token";
export const git = createGit(childEnv());

/** Runs git synchronously in a test; resolves to trimmed stdout. */
export function sh(cwd: string, ...args: string[]): string {
  return execFileSync("git", args, { cwd, env: { ...childEnv(), GIT_TERMINAL_PROMPT: "0" } })
    .toString()
    .trim();
}

/** A fresh mirror, token file and workspace options. */
export function makeMirror(): WorkspaceOptions & { root: string } {
  const root = mkdtempSync(join(tmpdir(), "rl-agent-mirror-"));
  onTestFinished(() => {
    rmSync(root, { recursive: true, force: true });
  });
  const seed = join(root, "seed");
  sh(root, "init", "-q", "-b", "main", seed);
  writeFileSync(join(seed, "README.md"), "demo\n");
  sh(seed, "add", ".");
  sh(seed, "-c", "user.name=t", "-c", "user.email=t@example.com", "commit", "-qm", "init");
  sh(root, "clone", "-q", "--bare", seed, join(root, "demo.git"));
  sh(root, "init", "-q", "--bare", join(root, "session", "demo.git"));
  const tokenFile = join(root, "token");
  writeFileSync(tokenFile, TOKEN);
  return {
    root,
    sessionId: SESSION,
    // Only the credential helper's scope; the repos are reached by path.
    mirrorUrl: "http://mirror.test:8080",
    cloneUrl: join(root, "demo.git"),
    pushUrl: join(root, "session", "demo.git"),
    baseRef: "main",
    tokenFile,
    workdir: join(root, "work"),
    git,
  };
}
