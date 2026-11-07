import { execFile } from "node:child_process";
import type { ChildEnv } from "../config.ts";

/** Runs git with these arguments; resolves to its trimmed stdout. Rejects with {@link GitError}. */
export type Git = (args: string[], options: { cwd: string; env?: ChildEnv }) => Promise<string>;

/** A git command failed. The message has the command and git's stderr, never a credential. */
export class GitError extends Error {
  override name = "GitError";
}

// A clone of a large repo over the mirror is the slowest call; nothing should take longer.
const TIMEOUT_MS = 10 * 60 * 1000;

/** A {@link Git} that runs the real git binary with only the given base environment. */
export function createGit(base: ChildEnv): Git {
  return (args, { cwd, env = {} }) =>
    new Promise((resolve, reject) => {
      execFile(
        "git",
        args,
        {
          cwd,
          env: { ...base, GIT_TERMINAL_PROMPT: "0", ...env },
          timeout: TIMEOUT_MS,
          maxBuffer: 64 * 1024 * 1024,
        },
        (error, stdout, stderr) => {
          if (error)
            reject(new GitError(`git ${args[0] ?? ""}: ${stderr.trim() || error.message}`));
          else resolve(stdout.trim());
        },
      );
    });
}
