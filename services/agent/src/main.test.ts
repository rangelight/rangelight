import { spawn } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, onTestFinished, test } from "vitest";
import { childEnv } from "./config.ts";
import { SESSION, until } from "./runtime/contract.ts";

// Starts a real node process; under a loaded test run that alone can take seconds.
test(
  "SIGTERM while cloning ends the agent at once with 143, not by the signal",
  { timeout: 20_000 },
  async () => {
    const dir = mkdtempSync(join(tmpdir(), "rl-agent-main-"));
    // A mirror that accepts the connection and never answers: the clone hangs.
    let connected = false;
    const mirror = createServer(() => {
      connected = true;
    });
    await new Promise<void>((resolve) => mirror.listen(0, "127.0.0.1", resolve));
    onTestFinished(() => {
      mirror.close();
      rmSync(dir, { recursive: true, force: true });
    });
    const address = mirror.address();
    const port = address !== null && typeof address === "object" ? address.port : 0;
    writeFileSync(
      join(dir, "ticket.json"),
      JSON.stringify({ key: "RAN-1", title: "t", description: "", criteria: ["c"] }),
    );
    writeFileSync(join(dir, "profile.json"), JSON.stringify({ spec: { services: [] } }));
    const agent = spawn(process.execPath, [join(import.meta.dirname, "main.ts")], {
      env: {
        ...childEnv(),
        RANGELIGHT_AGENT_SESSION_ID: SESSION,
        RANGELIGHT_AGENT_MIRROR_URL: `http://127.0.0.1:${String(port)}`,
        RANGELIGHT_AGENT_REPO: "demo",
        RANGELIGHT_AGENT_WORKDIR: join(dir, "repo"),
        RANGELIGHT_AGENT_TOKEN_FILE: join(dir, "token"),
        RANGELIGHT_AGENT_TICKET_FILE: join(dir, "ticket.json"),
        RANGELIGHT_AGENT_PROFILE_FILE: join(dir, "profile.json"),
        CLAUDE_CONFIG_DIR: join(dir, "claude"),
      },
      stdio: "ignore",
    });
    const exited = new Promise<[number | null, string | null]>((resolve) => {
      agent.on("exit", (code, signal) => {
        resolve([code, signal]);
      });
    });
    await until(() => connected);
    agent.kill("SIGTERM");
    expect(await exited).toEqual([143, null]);
  },
);
