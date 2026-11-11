import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { connect, type Msg, type NatsConnection } from "@nats-io/transport-node";
import { Ajv2020 } from "ajv/dist/2020.js";
import schema from "../../../../api/jsonschema/terminal.v1alpha1.schema.json" with { type: "json" };
import type { RangelightTerminalMessageV1Alpha1 as Message } from "../../../../shared/ts/gen/terminal.v1alpha1.schema.d.ts";
import { afterEach, beforeEach, expect, test } from "vitest";
import { testNatsUrl } from "../config.ts";
import { SESSION, until } from "../runtime/contract.ts";
import { ptyIn, ptyOut } from "../subjects.ts";
import { MAX_CHUNK, startTerminal, type Pty, type Terminal } from "./terminal.ts";

const ORG = "acme";
const TOKEN = "sk-ant-oat01-fake-login-code";
const validate = new Ajv2020({ strict: true }).compile<Message>(schema);
// The shared test server takes any token; sign-in.test.ts checks one that checks it.
const tokenFile = join(mkdtempSync(join(tmpdir(), "rl-agent-nats-")), "token");
writeFileSync(tokenFile, "lane-token\n");

/** A PTY that records what it got. */
function fakePty() {
  const sizes: [number, number][] = [];
  const got = { bytes: "", sizes };
  const pty: Pty = {
    write: (b) => (got.bytes += Buffer.from(b).toString()),
    resize: (cols, rows) => {
      got.sizes.push([cols, rows]);
      return Promise.resolve();
    },
  };
  return { got, pty };
}

let client: NatsConnection;
let terminal: Terminal;
let seen: Msg[];
const logs: string[] = [];
const { got, pty } = fakePty();
const send = (m: unknown) => {
  client.publish(ptyIn(ORG, SESSION), typeof m === "string" ? m : JSON.stringify(m));
};

beforeEach(async () => {
  Object.assign(got, { bytes: "", sizes: [] });
  logs.length = 0;
  seen = [];
  client = await connect({ servers: testNatsUrl() });
  client.subscribe("rl.>", {
    callback: (_, m) => {
      seen.push(m);
    },
  });
  await client.flush();
  terminal = startTerminal({
    natsUrl: testNatsUrl(),
    natsTokenFile: tokenFile,
    org: ORG,
    sessionId: SESSION,
    pty,
    log: (l) => logs.push(l),
  });
  // Ready once a resize goes through. NATS keeps one publisher's order, so once the last probe
  // (2x2) is in, every earlier one is too and none lands in the test.
  await until(() => {
    send({ type: "resize", cols: 1, rows: 1 });
    return got.sizes.length > 0;
  });
  send({ type: "resize", cols: 2, rows: 2 });
  await until(() => got.sizes.at(-1)?.[0] === 2);
  Object.assign(got, { bytes: "", sizes: [] });
  seen = [];
});

afterEach(async () => {
  await terminal.close();
  await client.close();
});

test("terminal: input and resize from pty.<session>.in reach the PTY", async () => {
  send({ type: "input", data: Buffer.from("ls\r").toString("base64") });
  send({ type: "resize", cols: 120, rows: 40 });
  await until(() => got.sizes.length === 1);
  expect(got).toEqual({ bytes: "ls\r", sizes: [[120, 40]] });
});

test("terminal: output goes out on its own session's out subject only, split to the schema's limit", async () => {
  const big = Buffer.alloc(MAX_CHUNK * 2 + 10, "x");
  terminal.output(Buffer.from("hello"));
  terminal.output(big);
  await until(() => seen.length === 4);
  expect(seen.map((m) => m.subject)).toEqual(Array(4).fill(ptyOut(ORG, SESSION)));
  const data = seen.map((m) => {
    const message: unknown = m.json();
    if (!validate(message) || message.type !== "output") throw new Error(m.string());
    return Buffer.from(message.data, "base64");
  });
  expect(Buffer.concat(data).toString()).toBe(`hello${big.toString()}`);
});

test("terminal: AC5 a message that isn't input is dropped, and no log line holds its contents", async () => {
  send({ type: "input", data: Buffer.from(TOKEN).toString("base64") });
  for (const bad of [
    `{"type":"input","data":"${TOKEN}"}`,
    { type: "output", data: Buffer.from(TOKEN).toString("base64") },
    { type: "resize", cols: 0, rows: 1, note: TOKEN },
    TOKEN,
  ]) {
    send(bad);
  }
  await until(() => logs.length === 4);
  expect(got).toEqual({ bytes: TOKEN, sizes: [] });
  for (const l of logs) expect(l).toBe("terminal: dropped a message that isn't terminal input");
});
