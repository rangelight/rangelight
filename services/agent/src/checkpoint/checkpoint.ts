import type { Git } from "../workspace/git.ts";

/** Where a session's checkpoint refs live on the mirror. */
export function checkpointPrefix(sessionId: string): string {
  return `refs/rangelight/sessions/${sessionId}/cp/`;
}

/** The append-only checkpoint ref n of a session on the mirror. */
export function checkpointRef(sessionId: string, n: number): string {
  return `${checkpointPrefix(sessionId)}${String(n)}`;
}

/** The session's work branch on the mirror; the agent and the model push their commits here. */
export function workBranchRef(sessionId: string): string {
  return `refs/heads/rangelight/${sessionId}/work`;
}

/** The highest checkpoint number among `git ls-remote` lines, or 0. */
export function lastCheckpoint(sessionId: string, lsRemote: string): number {
  const prefix = checkpointPrefix(sessionId);
  let last = 0;
  for (const line of lsRemote.split("\n")) {
    const ref = line.split("\t")[1] ?? "";
    const n = ref.startsWith(prefix) ? Number(ref.slice(prefix.length)) : 0;
    if (Number.isSafeInteger(n) && n > last) last = n;
  }
  return last;
}

/** Options for {@link Checkpoints}. */
export interface CheckpointOptions {
  sessionId: string;
  workdir: string;
  /** The number of the last checkpoint already on the mirror. */
  last: number;
  git: Git;
}

/**
 * Saves the whole worktree, uncommitted and untracked files included (minus .gitignore), as a
 * checkpoint commit on the mirror, and force-pushes the session's work branch (never whatever
 * other branch the model has checked out). HEAD, the index and the files are left as they are,
 * so the model never sees it happen.
 */
export class Checkpoints {
  readonly #options: CheckpointOptions;
  readonly #git: Git;
  #n: number;
  #saved = "";
  #queue: Promise<unknown> = Promise.resolve();

  constructor(options: CheckpointOptions) {
    this.#options = options;
    this.#git = options.git;
    this.#n = options.last;
  }

  /**
   * Pushes a checkpoint if anything changed since the last one; resolves to its ref, or null.
   * Calls run one after another, so two turn ends never race for the same number.
   */
  save(): Promise<string | null> {
    const next = this.#queue.then(() => this.#save());
    this.#queue = next.catch(() => undefined);
    return next;
  }

  async #save(): Promise<string | null> {
    const { workdir, sessionId } = this.#options;
    const index = { GIT_INDEX_FILE: `${workdir}/.git/rangelight-checkpoint-index` };
    const head = await this.#git(["rev-parse", "HEAD"], { cwd: workdir });
    await this.#git(["read-tree", "HEAD"], { cwd: workdir, env: index });
    await this.#git(["add", "-A"], { cwd: workdir, env: index });
    const tree = await this.#git(["write-tree"], { cwd: workdir, env: index });
    if (`${head} ${tree}` === this.#saved) return null;
    const message = `checkpoint ${String(this.#n + 1)}`;
    const commit = await this.#git(["commit-tree", tree, "-p", head, "-m", message], {
      cwd: workdir,
    });
    const ref = checkpointRef(sessionId, this.#n + 1);
    await this.#git(
      [
        "push",
        "origin",
        `${commit}:${ref}`,
        `+${workBranchRef(sessionId)}:${workBranchRef(sessionId)}`,
      ],
      {
        cwd: workdir,
      },
    );
    this.#n += 1;
    this.#saved = `${head} ${tree}`;
    return ref;
  }
}
