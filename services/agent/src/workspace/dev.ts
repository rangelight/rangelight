import { spawn } from "node:child_process";
import { readFileSync } from "node:fs";
import { z } from "zod";
import type { ChildEnv } from "../config.ts";
import type { Emit } from "../events/events.ts";

// Only the fields the agent uses; the runner already validated the whole profile (RAN-19).
const profileSchema = z.object({
  spec: z.object({
    services: z.array(
      z.object({
        name: z.string(),
        underChange: z
          .object({
            repoPath: z
              .string()
              .regex(/^(?!\/)(?!.*(^|\/)\.\.(\/|$)).*$/, "a path inside the repo"),
            command: z.string().min(1),
            env: z.record(z.string(), z.string()).optional(),
          })
          .optional(),
      }),
    ),
  }),
});

/** One dev command to run in the worktree. */
export interface DevCommand {
  service: string;
  cwd: string;
  command: string;
  env: Record<string, string>;
}

/**
 * The dev commands of the services under change, from the profile the runner mounts (read from
 * the base branch, ADR 0041), never from the worktree's rangelight.yaml. Throws if the profile is
 * invalid or a service under change has no dev command.
 */
export function devCommands(
  profileFile: string,
  underChange: string[],
  workdir: string,
): DevCommand[] {
  const profile = profileSchema.parse(JSON.parse(readFileSync(profileFile, "utf8")));
  return underChange.map((name) => {
    const dev = profile.spec.services.find((s) => s.name === name)?.underChange;
    if (!dev) throw new Error(`profile: service ${name} has no underChange command`);
    return {
      service: name,
      cwd: `${workdir}/${dev.repoPath}`,
      command: dev.command,
      env: dev.env ?? {},
    };
  });
}

/**
 * Starts each dev command once, in the background. The app's own dev server does the hot reload;
 * the agent never restarts it. An exit is reported as a progress event. Output goes to stderr,
 * because stdout carries the events.
 */
export function startDevCommands(commands: DevCommand[], env: ChildEnv, emit: Emit): void {
  for (const c of commands) {
    const child = spawn("sh", ["-c", c.command], {
      cwd: c.cwd,
      env: { ...env, ...c.env },
      stdio: ["ignore", 2, 2],
    });
    const report = (how: string) => {
      const note = `${c.service} dev command ${how}`.slice(0, 200);
      emit({ type: "progress", payload: { stage: "dev", note } });
    };
    child.on("error", (error) => {
      report(`failed to start: ${error.message}`);
    });
    child.on("exit", (code, signal) => {
      report(`exited (${signal ?? `code ${String(code)}`})`);
    });
  }
}
