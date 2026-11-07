import { execFile, spawn, type ChildProcess } from "node:child_process";
import { setTimeout as sleep } from "node:timers/promises";
import type { ChildEnv } from "../config.ts";
import { hookSettings, listenForHooks, toRuntimeEvent, type HookServer } from "./hooks.ts";
import type { AgentRuntime, InjectResult, OnRuntimeEvent } from "./runtime.ts";
import { RESUME_FAILED, Turns } from "./runtime.ts";

/** What the member sees when the lane has no Claude login (ADR 0046 §3 step 3). */
export const SIGN_IN_QUESTION =
  "Please sign in to Claude: open this lane's terminal, run `claude`, then /login. The lane starts by itself once you're signed in.";

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
  /** How often to check for a login while waiting for one. */
  signInPollMs?: number;
}

// Claude Code treats a burst of input as a paste, Enter included; Enter goes in on its own.
const ENTER_DELAY_MS = 200;

/** Single-quotes one word for sh. */
function shellQuote(word: string): string {
  return `'${word.replaceAll("'", `'\\''`)}'`;
}

/**
 * The unmodified `claude` binary in a PTY in the worktree (ADR 0046 §2). Events come from Claude
 * Code's hooks, posted to a local endpoint; messages are typed at the idle prompt; resume is
 * `claude --resume <id>`. Without a login it asks the member to sign in and waits.
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
    this.#child?.stdin?.write("\x1b");
    this.#turns.ended("cancelled");
  }

  async stop(): Promise<void> {
    this.#stopped = true;
    this.#turns.close();
    const child = this.#child;
    if (child?.pid !== undefined && child.exitCode === null && child.signalCode === null) {
      const exited = new Promise((resolve) => child.once("exit", resolve));
      // script(1) ends claude on SIGTERM, then waits for its own stdin to close.
      child.stdin?.end();
      try {
        process.kill(-child.pid, "SIGTERM");
      } catch {
        // ESRCH: it exited between the check and the kill; its exit event still comes.
      }
      await exited;
    } else {
      this.#exit(143);
    }
    await this.#hooks?.close();
  }

  async #launch(args: string[], onNoTurn?: () => void): Promise<void> {
    await this.#waitForSignIn();
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
    const child = this.#spawn(hooks.url, args);
    this.#child = child;
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

  #spawn(hookUrl: string, args: string[]): ChildProcess {
    const settings = { ...hookSettings(hookUrl), skipDangerousModePermissionPrompt: true };
    // The lane pod is the sandbox (ADR 0015), and nobody watches the prompt to approve tools.
    const argv = [
      this.#o.bin,
      "--settings",
      JSON.stringify(settings),
      "--permission-mode",
      "bypassPermissions",
      ...args,
    ];
    // util-linux script(1) gives claude a PTY; output goes nowhere (the sign-in code must not be
    // stored, ADR 0046 §3 step 4).
    const command = `stty cols 200 rows 50; exec ${argv.map(shellQuote).join(" ")}`;
    return spawn("script", ["-qfec", command, "/dev/null"], {
      cwd: this.#o.cwd,
      env: this.#env,
      stdio: ["pipe", "ignore", "ignore"],
      detached: true,
    });
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

  async #waitForSignIn(): Promise<void> {
    if (await this.#signedIn()) return;
    this.#o.onEvent({
      kind: "lane",
      event: { type: "ask", payload: { question: SIGN_IN_QUESTION } },
    });
    while (!this.#stopped) {
      await sleep(this.#o.signInPollMs ?? 5_000);
      if (await this.#signedIn()) return;
    }
  }

  #type(message: string): void {
    this.#child?.stdin?.write(message);
    setTimeout(() => this.#child?.stdin?.write("\r"), ENTER_DELAY_MS);
  }
}
