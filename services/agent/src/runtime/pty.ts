// The PTY processes of the cli runtime: util-linux script(1) gives claude a terminal of its own.
import { execFile, type ChildProcess } from "node:child_process";
import { readFile } from "node:fs/promises";

// How long claude gets to exit on SIGTERM before its process group is killed.
const KILL_AFTER_MS = 5_000;

/** Sets a PTY's size. */
export function stty(pts: string, cols: number, rows: number): Promise<unknown> {
  return new Promise((resolve) => {
    execFile("stty", ["-F", pts, "cols", String(cols), "rows", String(rows)], resolve);
  });
}

/** The pid of claude: script(1)'s child, which runs in a session and process group of its own. */
export async function claudeOf(scriptPid: number): Promise<number | undefined> {
  try {
    const children = await readFile(
      `/proc/${String(scriptPid)}/task/${String(scriptPid)}/children`,
      "utf8",
    );
    const pid = Number(children.trim().split(" ")[0]);
    return Number.isInteger(pid) && pid > 0 ? pid : undefined;
  } catch {
    return undefined; // script is gone
  }
}

/**
 * Ends a script(1) child and its claude; false if there was none still running. script(1) passes
 * SIGTERM on and kills a claude that ignores it, but not the rest of claude's process group, which
 * is killed once script is gone; if script itself hangs, both groups are killed after 5 s.
 * ponytail: a process that left claude's group (setsid) survives until the pod goes.
 */
export async function end(child: ChildProcess | undefined): Promise<boolean> {
  if (child?.pid === undefined || child.exitCode !== null || child.signalCode !== null) {
    return false;
  }
  const pid = child.pid;
  const claude = await claudeOf(pid);
  const exited = new Promise((resolve) => child.once("exit", resolve));
  const signal = (group: number | undefined, name: NodeJS.Signals) => {
    try {
      if (group !== undefined) process.kill(-group, name);
    } catch {
      // ESRCH: it exited between the check and the kill; script's exit event still comes.
    }
  };
  // script(1) ends claude on SIGTERM, then waits for its own stdin to close.
  child.stdin?.end();
  signal(pid, "SIGTERM");
  const late = setTimeout(() => {
    signal(pid, "SIGKILL");
    signal(claude, "SIGKILL");
  }, KILL_AFTER_MS);
  await exited;
  clearTimeout(late);
  // script(1) ends only its own child: whatever else is left in claude's group outlived the PTY.
  signal(claude, "SIGKILL");
  return true;
}

/** Single-quotes one word for sh. */
export function shellQuote(word: string): string {
  return `'${word.replaceAll("'", `'\\''`)}'`;
}
