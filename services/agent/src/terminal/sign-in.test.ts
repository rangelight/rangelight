// The lane signs in to NATS with its token file, and never gives up on a refused sign-in: a
// nats-server of its own that admits one token, then, restarted, another.
import { spawn, type ChildProcess } from "node:child_process";
import { mkdtempSync, writeFileSync } from "node:fs";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { setTimeout as sleep } from "node:timers/promises";
import { connect } from "@nats-io/transport-node";
import { afterEach, expect, test } from "vitest";
import { SESSION, until } from "../runtime/contract.ts";
import { ptyIn } from "../subjects.ts";
import { startTerminal, type Terminal } from "./terminal.ts";

const ORG = "acme";
let server: ChildProcess | undefined;
let terminal: Terminal | undefined;

afterEach(async () => {
  await terminal?.close();
  server?.kill();
});

function freePort(): Promise<number> {
  return new Promise((resolve) => {
    const s = createServer().listen(0, "127.0.0.1", () => {
      const address = s.address();
      s.close(() => {
        resolve(typeof address === "object" && address ? address.port : 0);
      });
    });
  });
}

/** nats-server (scripts/test/services.sh puts it on PATH) on `port`, admitting only `token`. */
async function natsServer(port: number, token: string): Promise<ChildProcess> {
  const child = spawn("nats-server", ["-a", "127.0.0.1", "-p", String(port), "--auth", token], {
    stdio: "ignore",
  });
  await until(() => child.exitCode === null && child.pid !== undefined);
  await sleep(300);
  return child;
}

async function stopped(child: ChildProcess): Promise<void> {
  const exited = new Promise((resolve) => child.once("exit", resolve));
  child.kill();
  await exited;
}

/** Whether a resize from the browser side, signed in with `token`, reaches the PTY now. */
async function reaches(port: number, token: string, sizes: [number, number][]): Promise<boolean> {
  const nc = await connect({ servers: `nats://127.0.0.1:${String(port)}`, token });
  nc.publish(ptyIn(ORG, SESSION), JSON.stringify({ type: "resize", cols: 7, rows: 7 }));
  await nc.flush();
  await sleep(100);
  await nc.close();
  return sizes.length > 0;
}

test("terminal: refused sign-ins never stop it, and the token file is read at every connect", async () => {
  const port = await freePort();
  const tokenFile = join(mkdtempSync(join(tmpdir(), "rl-agent-sign-in-")), "token");
  writeFileSync(tokenFile, "stale\n");
  server = await natsServer(port, "one");
  const sizes: [number, number][] = [];
  terminal = startTerminal({
    natsUrl: `nats://127.0.0.1:${String(port)}`,
    natsTokenFile: tokenFile,
    org: ORG,
    sessionId: SESSION,
    pty: {
      write: () => undefined,
      resize: (cols, rows) => {
        sizes.push([cols, rows]);
        return Promise.resolve();
      },
    },
    log: () => undefined,
  });
  // Refused at least twice (nats.js retries every 2 s), then the token rotates.
  await sleep(5_000);
  expect(await reaches(port, "one", sizes)).toBe(false);
  writeFileSync(tokenFile, "one\n");
  await untilAsync(() => reaches(port, "one", sizes), 15_000);

  // The server restarts wanting another token: refused again and again, until the file has it.
  await stopped(server);
  server = await natsServer(port, "two");
  await sleep(5_000);
  sizes.length = 0;
  writeFileSync(tokenFile, "two\n");
  await untilAsync(() => reaches(port, "two", sizes), 15_000);
}, 60_000);

async function untilAsync(ready: () => Promise<boolean>, ms: number): Promise<void> {
  const deadline = Date.now() + ms;
  while (!(await ready())) {
    if (Date.now() > deadline) throw new Error("timed out");
    await sleep(200);
  }
}

test("terminal: RAN-66 each failed first connect is logged with its reason, then it connects", async () => {
  const port = await freePort();
  const tokenFile = join(mkdtempSync(join(tmpdir(), "rl-agent-sign-in-")), "token");
  writeFileSync(tokenFile, "lane-token\n");
  const logs: string[] = [];
  const sizes: [number, number][] = [];
  terminal = startTerminal({
    natsUrl: `nats://127.0.0.1:${String(port)}`,
    natsTokenFile: tokenFile,
    org: ORG,
    sessionId: SESSION,
    pty: {
      write: () => undefined,
      resize: (cols, rows) => {
        sizes.push([cols, rows]);
        return Promise.resolve();
      },
    },
    log: (l) => logs.push(l),
  });
  // Nothing listens: every try (every 2 s) says why, and only that.
  await until(() => logs.length >= 2, 10_000);
  for (const l of logs) expect(l).toMatch(/^terminal: no NATS, trying again: \S/);
  expect(logs.join("\n")).not.toContain("lane-token");
  server = await natsServer(port, "lane-token");
  await untilAsync(() => reaches(port, "lane-token", sizes), 15_000);
}, 30_000);
