import { execFile, spawn, type ChildProcess } from "node:child_process";
import { readlink } from "node:fs/promises";
import { setTimeout as sleep } from "node:timers/promises";
import type { ChildEnv } from "../config.ts";
import { hookSettings, listenForHooks, toRuntimeEvent, type HookServer } from "./hooks.ts";
import { claudeOf, end, shellQuote, stty } from "./pty.ts";
import type { AgentRuntime, InjectResult, OnRuntimeEvent } from "./runtime.ts";
import { RESUME_FAILED, Turns } from "./runtime.ts";

/**
 * What the member sees when the lane has no Claude login (ADR 0046 §3 step 3). Claude Code also
 * asks once whether it may trust the worktree; every lane's is /workspace/repo, so one Yes, like
 * one /login, covers the lanes after it. The agent never writes either into the Claude home.
 */
export const SIGN_IN_QUESTION =
  "Please sign in to Claude: open this lane's terminal, type /login, and answer Yes when Claude asks whether you trust the folder. The lane starts by itself once you're signed in.";

/** Options for {@link CliRuntime}. */
export interface CliOptions {
  /** The claude binary. */
  bin: string;
  /** The worktree. */
  cwd: string;
  sessionId: string;
  /** The mounted Claude home. Passed to Claude Code; the agent never reads anything under it. */
  claudeConfigDir: string;
  env: ChildEnv;
  onEvent: OnRuntimeEvent;
  /** Receives what the PTY writes: the lane terminal's output. It is never stored (ADR 0046 §3). */
  onOutput?: (chunk: Buffer) => void;
  /** How often to check for a login while waiting for one. */
  signInPollMs?: number;
}

// Claude Code treats a burst of input as a paste, Enter included; Enter goes in on its own.
const ENTER_DELAY_MS = 200;
// Between the two size changes of a forced redraw: Node reads the size when SIGWINCH arrives and
// redraws only if it changed, so the two must not land in one read.
const REDRAW_STEP_MS = 100;
// What a member's key does to a running turn in Claude Code: Escape or Ctrl+C on its own cancels it
// (no Stop hook follows), as interrupt() does.
// ponytail: an Escape that only closed a dialog or a completion list also marks the turn cancelled,
// so an injected message could then be typed into the still-running turn. Upgrade when injection is
// wired (RAN-35/36): a turn ends only on the Stop hook or a signal that the prompt is idle.
const CANCEL_KEYS = new Set(["\x1b", "\x03"]);

/**
 * The unmodified `claude` binary in a PTY in the worktree (ADR 0046 §2): the lane terminal is its
 * own TUI. Events come from Claude Code's hooks, posted to a local endpoint; messages are typed at
 * the idle prompt; resume is `claude --resume <id>`. Without a login it asks the member to sign in
 * and runs `claude` with no prompt in the same PTY, for /login; once signed in, the session takes
 * its place there.
 */
export class CliRuntime implements AgentRuntime {
  readonly #o: CliOptions;
  readonly #turns: Turns;
  readonly #env: ChildEnv;
  #child: ChildProcess | undefined;
  #hooks: HookServer | undefined;
  #turnSeen = false;
  #stopped = false;
  #exited = false;
  /** The lane terminal's size: every claude starts at it. */
  #size = { cols: 200, rows: 50 };

  constructor(options: CliOptions) {
    this.#o = options;
    this.#turns = new Turns((message) => {
      this.#type(message);
    }, options.onEvent);
    this.#env = {
      ...options.env,
      CLAUDE_CONFIG_DIR: options.claudeConfigDir,
      // The image pins the version; an update would change the binary under a running lane.
      DISABLE_AUTOUPDATER: "1",
      TERM: "xterm-256color",
      SHELL: "/bin/sh",
    };
  }

  start(prompt: string): Promise<void> {
    return this.#launch(["--session-id", this.#o.sessionId, prompt]);
  }

  resume(sessionId: string, prompt: string): Promise<void> {
    // claude exits at once when it has no transcript for the id (a lane closed before its first
    // turn); ADR 0045 §4: then start a new session with the digest.
    return this.#launch(["--resume", sessionId, prompt], () => {
      this.#o.onEvent({ kind: "lane", event: RESUME_FAILED });
      this.#launch(["--session-id", sessionId, prompt]).catch(() => {
        this.#exit(1);
      });
    });
  }

  inject(message: string): InjectResult {
    return this.#turns.inject(message);
  }

  interrupt(): void {
    if (!this.#turns.running) return;
    this.#send("\x1b");
    this.#turns.ended("cancelled");
  }

  /**
   * Types raw bytes into the PTY, as the member does in the lane terminal. A member's turn is a
   * turn like any other (its hooks fire); their Escape or Ctrl+C cancels the running one.
   */
  write(bytes: Uint8Array): void {
    this.#send(bytes);
    if (this.#turns.running && CANCEL_KEYS.has(Buffer.from(bytes).toString("latin1"))) {
      this.#turns.ended("cancelled");
    }
  }

  /**
   * Sets the PTY's size in cells; the kernel sends claude SIGWINCH, and it redraws. A browser
   * sends its size whenever it connects, so the same size forces a full redraw (one column less,
   * then back): a new viewer sees the screen at once, not when claude next prints.
   */
  async resize(cols: number, rows: number): Promise<void> {
    const same = cols === this.#size.cols && rows === this.#size.rows;
    this.#size = { cols, rows };
    const pts = await this.#pts();
    if (pts === undefined) return;
    if (same && cols > 1) {
      await stty(pts, cols - 1, rows);
      await sleep(REDRAW_STEP_MS);
    }
    // The latest size: another resize may have come in meanwhile.
    await stty(pts, this.#size.cols, this.#size.rows);
  }

  async stop(): Promise<void> {
    this.#stopped = true;
    this.#turns.close();
    if (!(await end(this.#child))) this.#exit(143);
    await this.#hooks?.close();
  }

  async #launch(args: string[], onNoTurn?: () => void): Promise<void> {
    await this.#signIn();
    if (this.#stopped) return;
    const hooks = await listenForHooks((hook) => {
      const event = toRuntimeEvent(hook);
      if (event.kind === "turn_start") {
        this.#turnSeen = true;
        this.#turns.started();
      } else if (event.kind === "turn_end") this.#turns.ended(event.outcome);
      else this.#o.onEvent(event);
    });
    this.#hooks = hooks;
    this.#turnSeen = false;
    const settings = { ...hookSettings(hooks.url), skipDangerousModePermissionPrompt: true };
    // The lane pod is the sandbox (ADR 0015), and nobody watches the prompt to approve tools.
    const child = this.#spawn([
      "--settings",
      JSON.stringify(settings),
      "--permission-mode",
      "bypassPermissions",
      ...args,
    ]);
    child.on("error", (error) => {
      void hooks.close();
      const note = `claude failed to start: ${error.message}`.slice(0, 200);
      this.#o.onEvent({
        kind: "lane",
        event: { type: "progress", payload: { stage: "runtime", note } },
      });
      this.#exit(127);
    });
    child.on("exit", (code) => {
      void hooks.close();
      if (onNoTurn && !this.#turnSeen && !this.#stopped) onNoTurn();
      else this.#exit(code ?? 143);
    });
  }

  /** Runs claude with `args` in a new PTY of the lane terminal's size; it becomes the PTY's claude. */
  #spawn(args: string[]): ChildProcess {
    const argv = [this.#o.bin, ...args];
    // util-linux script(1) gives claude a PTY; its output goes only to the lane terminal, never to
    // a file or a log (the sign-in code must not be stored, ADR 0046 §3 step 4).
    const { cols, rows } = this.#size;
    const stty = `stty cols ${String(cols)} rows ${String(rows)}`;
    const command = `${stty}; exec ${argv.map(shellQuote).join(" ")}`;
    const child = spawn("script", ["-qfec", command, "/dev/null"], {
      cwd: this.#o.cwd,
      env: this.#env,
      stdio: ["pipe", "pipe", "ignore"],
      detached: true,
    });
    // Always read it: a full pipe would stop claude.
    child.stdout.on("data", (chunk: Buffer) => this.#o.onOutput?.(chunk));
    // A write racing the end of this claude (a key during the sign-in swap) must not end the agent.
    child.stdin.on("error", () => undefined);
    this.#child = child;
    return child;
  }

  /** The PTY claude runs on: the terminal of script(1)'s child, which became claude. */
  async #pts(): Promise<string | undefined> {
    const pid = this.#child?.pid;
    const claude = pid === undefined ? undefined : await claudeOf(pid);
    if (claude === undefined) return undefined;
    try {
      const pts = await readlink(`/proc/${String(claude)}/fd/0`);
      return /^\/dev\/pts\/\d+$/.test(pts) ? pts : undefined;
    } catch {
      return undefined; // claude is gone, or not started yet
    }
  }

  /** Reports the runtime's end, once. */
  #exit(code: number): void {
    if (this.#exited) return;
    this.#exited = true;
    this.#o.onEvent({ kind: "exit", code });
  }

  /** Asks Claude Code itself whether it holds a login: the agent never looks at the files. */
  #signedIn(): Promise<boolean> {
    return new Promise((resolve) => {
      execFile(this.#o.bin, ["auth", "status"], { env: this.#env, timeout: 30_000 }, (error) => {
        resolve(error === null);
      });
    });
  }

  /**
   * Returns once Claude Code holds a login, or the runtime stops. Until then `claude` runs with no
   * prompt in the lane terminal (again if the member exits it), where the member types /login; it
   * is ended before the session takes its place.
   */
  async #signIn(): Promise<void> {
    if (await this.#signedIn()) return;
    this.#o.onEvent({
      kind: "lane",
      event: { type: "ask", payload: { question: SIGN_IN_QUESTION } },
    });
    let login: ChildProcess | undefined;
    while (!this.#stopped) {
      if (login === undefined || login.exitCode !== null || login.signalCode !== null) {
        login = this.#spawn([]);
        login.on("error", () => undefined); // spawning again is the answer
        login.on("exit", () => {
          if (this.#stopped) this.#exit(143);
        });
      }
      await sleep(this.#o.signInPollMs ?? 5_000);
      if (await this.#signedIn()) break;
    }
    if (!this.#stopped) await end(login);
  }

  #type(message: string): void {
    this.#send(message);
    setTimeout(() => {
      this.#send("\r");
    }, ENTER_DELAY_MS);
  }

  /** Writes to the current claude's PTY, if it is still taking input; between claudes, drops it. */
  #send(bytes: Uint8Array | string): void {
    const stdin = this.#child?.stdin;
    if (stdin?.writable) stdin.write(bytes);
  }
}
