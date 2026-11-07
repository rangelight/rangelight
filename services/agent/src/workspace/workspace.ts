import { existsSync } from "node:fs";
import { dirname } from "node:path";
import {
  checkpointPrefix,
  checkpointRef,
  lastCheckpoint,
  workBranchRef,
} from "../checkpoint/checkpoint.ts";
import type { Git } from "./git.ts";

/** Where the lane's worktree comes from and goes to. */
export interface WorkspaceOptions {
  sessionId: string;
  /** The mirror's base URL: the only URL git sends the token to. */
  mirrorUrl: string;
  cloneUrl: string;
  pushUrl: string;
  baseRef: string;
  tokenFile: string;
  workdir: string;
  git: Git;
}

/** The worktree is ready; `resumed` when the session already pushed its work branch before. */
export interface Workspace {
  resumed: boolean;
  lastCheckpoint: number;
}

/**
 * A git credential helper that reads the lane's projected token from its file each time git asks.
 * The token never appears in argv, the URL, the environment or a log line: git reads it from the
 * helper's stdout. The path is validated by config to [A-Za-z0-9._/-], so single quotes are safe.
 */
export function credentialHelper(tokenFile: string): string {
  return `!f() { test "$1" = get || exit 0; echo username=session; printf 'password=%s\\n' "$(cat '${tokenFile}')"; }; f`;
}

/** The local name of the session's work branch. */
export function workBranch(sessionId: string): string {
  return workBranchRef(sessionId).replace(/^refs\/heads\//, "");
}

/** The argv of the clone: base branch from the mirror, pushes to the session's own repo. */
export function cloneArgs(o: WorkspaceOptions): string[] {
  const branch = workBranch(o.sessionId);
  return [
    "clone",
    "--branch",
    o.baseRef,
    "-c",
    // Scoped to the mirror: a fetch from any other URL (repo text, a submodule) gets no token.
    `credential.${o.mirrorUrl}.helper=${credentialHelper(o.tokenFile)}`,
    "-c",
    `remote.origin.pushurl=${o.pushUrl}`,
    "-c",
    "push.default=upstream",
    "-c",
    "user.name=Rangelight agent",
    "-c",
    "user.email=agent@rangelight.dev",
    "-c",
    `branch.${branch}.remote=origin`,
    "-c",
    `branch.${branch}.merge=${workBranchRef(o.sessionId)}`,
    "--",
    o.cloneUrl,
    o.workdir,
  ];
}

/**
 * Clones the repo from the mirror and checks out the session's work branch: the one on the mirror
 * if the session pushed before (a reopened lane), otherwise a new one from the base branch, pushed
 * at once so a later start knows the session exists. A worktree already on disk (the container
 * restarted in the same pod) is kept as it is, uncommitted work included.
 */
export async function prepareWorkspace(o: WorkspaceOptions): Promise<Workspace> {
  const { git, workdir } = o;
  const kept = existsSync(`${workdir}/.git`);
  if (!kept) await git(cloneArgs(o), { cwd: dirname(workdir) });
  const work = workBranchRef(o.sessionId);
  const cps = `${checkpointPrefix(o.sessionId)}*`;
  // The mirror answers 404 for a session that never pushed; the clone above just proved the
  // mirror is reachable, so a failure here means a new session.
  const remote = await git(["ls-remote", o.pushUrl, work, cps], { cwd: workdir }).catch(() => "");
  const resumed = remote.split("\n").some((line) => line.endsWith(`\t${work}`));
  const last = lastCheckpoint(o.sessionId, remote);
  if (!kept && resumed) {
    await git(["fetch", o.pushUrl, work], { cwd: workdir });
    await git(["checkout", "-B", workBranch(o.sessionId), "FETCH_HEAD"], { cwd: workdir });
    if (last > 0) await restoreCheckpoint(o, checkpointRef(o.sessionId, last));
  } else if (!kept) {
    await git(["checkout", "-b", workBranch(o.sessionId)], { cwd: workdir });
  }
  if (!resumed) await git(["push", "origin", `HEAD:${work}`], { cwd: workdir });
  return { resumed, lastCheckpoint: last };
}

/**
 * Puts a checkpoint's uncommitted work back into the worktree, as uncommitted changes again,
 * when it was taken on the work branch's current head. A checkpoint on an older head is left
 * alone: the commits since then are newer than its files.
 */
async function restoreCheckpoint(o: WorkspaceOptions, ref: string): Promise<void> {
  const cwd = o.workdir;
  const head = await o.git(["rev-parse", "HEAD"], { cwd });
  await o.git(["fetch", o.pushUrl, ref], { cwd });
  const parent = await o.git(["rev-parse", "FETCH_HEAD^"], { cwd });
  if (parent !== head) return;
  await o.git(["checkout", "FETCH_HEAD", "--", "."], { cwd });
  await o.git(["reset", "-q"], { cwd });
}
