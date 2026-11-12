import { readFileSync } from "node:fs";
import { setTimeout as sleep } from "node:timers/promises";
import {
  connect,
  tokenAuthenticator,
  type ConnectionOptions,
  type NatsConnection,
} from "@nats-io/transport-node";
import { Ajv2020 } from "ajv/dist/2020.js";
import schema from "../../../../api/jsonschema/terminal.v1alpha1.schema.json" with { type: "json" };
import type {
  RangelightTerminalMessageV1Alpha1 as Message,
  TerminalOutput,
} from "../../../../shared/ts/gen/terminal.v1alpha1.schema.d.ts";
import { ptyIn, ptyOut } from "../subjects.ts";

/** The PTY a terminal drives: the cli runtime's. */
export interface Pty {
  write(bytes: Uint8Array): void;
  resize(cols: number, rows: number): Promise<void>;
}

/** Options for {@link startTerminal}. */
export interface TerminalOptions {
  natsUrl: string;
  /** The lane's projected token for NATS, read again at every connect (the kubelet rotates it). */
  natsTokenFile: string;
  org: string;
  sessionId: string;
  pty: Pty;
  /** Where the terminal reports its own state: never a message's contents (ADR 0046 §3 step 4). */
  log: (line: string) => void;
}

/** The lane terminal: {@link Terminal.output} sends what the PTY wrote. */
export interface Terminal {
  output(chunk: Uint8Array): void;
  close(): Promise<void>;
}

/** How long a refused or failed first connect waits before the next try. */
const RETRY_MS = 2_000;

/** The most bytes one `output` message carries: the schema's base64 limit, decoded (48 KiB). */
export const MAX_CHUNK = (schema.$defs.bytes.maxLength / 4) * 3;

const validate = new Ajv2020({ strict: true }).compile<Message>(schema);

function parse(data: Uint8Array): Message | undefined {
  try {
    const message: unknown = JSON.parse(new TextDecoder().decode(data));
    return validate(message) ? message : undefined;
  } catch {
    return undefined;
  }
}

const reason = (error: unknown): string => (error instanceof Error ? error.message : String(error));

/**
 * The first connect, tried again every {@link RETRY_MS} until it succeeds or `closed()` holds (then
 * `undefined`). Once connected, the client itself reconnects.
 */
async function connectUntil(
  options: ConnectionOptions,
  closed: () => boolean,
  log: (line: string) => void,
): Promise<NatsConnection | undefined> {
  while (!closed()) {
    try {
      return await connect(options);
    } catch (error) {
      log(`terminal: no NATS, trying again: ${reason(error)}`);
      await sleep(RETRY_MS);
    }
  }
  return undefined;
}

/**
 * Bridges the PTY to the lane terminal's core NATS subjects (ADR 0040 §1): `input` and `resize`
 * from `pty.<session>.in` go to the PTY, what the PTY writes goes out on `pty.<session>.out`. It
 * signs in as the lane, with its projected token: the auth callout lets it use exactly these two
 * subjects (ADR 0045 amendment). It connects in the background and never gives up, a refused
 * sign-in included (the callout restarting, a token not yet rotated): the lane runs without it,
 * and output while it is away is dropped (the screen redraws on the browser's next resize).
 */
export function startTerminal(o: TerminalOptions): Terminal {
  const out = ptyOut(o.org, o.sessionId);
  let nc: NatsConnection | undefined;
  let closed = false;
  const token = (): string => {
    try {
      return readFileSync(o.natsTokenFile, "utf8").trim();
    } catch (error) {
      o.log(`terminal: can't read the NATS token: ${reason(error)}`);
      return "";
    }
  };
  const options = {
    servers: o.natsUrl,
    name: `rl-agent-${o.sessionId}`,
    authenticator: tokenAuthenticator(token),
    // No waitOnFirstConnect: a failed first connect rejects, so connectUntil logs why and tries again.
    maxReconnectAttempts: -1,
    // By default two refused sign-ins in a row stop the client for good.
    ignoreAuthErrorAbort: true,
  };
  void connectUntil(options, () => closed, o.log).then((c) => {
    if (c === undefined) return;
    if (closed) {
      void c.close();
      return;
    }
    nc = c;
    c.subscribe(ptyIn(o.org, o.sessionId), {
      callback: (error, msg) => {
        if (error) {
          o.log(`terminal: subscription ended: ${error.message}`);
          return;
        }
        const message = parse(msg.data);
        if (message === undefined || message.type === "output") {
          o.log("terminal: dropped a message that isn't terminal input");
        } else if (message.type === "input") {
          o.pty.write(Buffer.from(message.data, "base64"));
        } else {
          void o.pty.resize(message.cols, message.rows);
        }
      },
    });
  });
  return {
    output(chunk) {
      for (let at = 0; nc && at < chunk.length; at += MAX_CHUNK) {
        const data = Buffer.from(chunk.subarray(at, at + MAX_CHUNK)).toString("base64");
        nc.publish(out, JSON.stringify({ type: "output", data } satisfies TerminalOutput));
      }
    },
    async close() {
      closed = true;
      await nc?.close();
    },
  };
}
