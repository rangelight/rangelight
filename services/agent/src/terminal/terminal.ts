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
import { createEmitter, type Emit } from "../events/events.ts";
import { systemClock, uuidv7Ids } from "../events/ids.ts";
import { agentEvents, ptyIn, ptyOut } from "../subjects.ts";

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

/** What {@link connectLane} needs of the agent's config. */
export type LaneConfig = Omit<TerminalOptions, "pty" | "log">;

/**
 * The lane's NATS connection: {@link Terminal.output} sends what the PTY wrote, and
 * {@link Terminal.event} one serialised lane event.
 */
export interface Terminal {
  output(chunk: Uint8Array): void;
  event(line: string): void;
  close(): Promise<void>;
}

/** The most events held while the first connect is under way; later ones are dropped. */
export const PENDING_EVENTS = 64;

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

/** The lane's connection: as the lane, with its token read again at every connect, retried for
 * ever. */
function connectionOptions(o: TerminalOptions): ConnectionOptions {
  const token = (): string => {
    try {
      return readFileSync(o.natsTokenFile, "utf8").trim();
    } catch (error) {
      o.log(`terminal: can't read the NATS token: ${reason(error)}`);
      return "";
    }
  };
  return {
    servers: o.natsUrl,
    name: `rl-agent-${o.sessionId}`,
    authenticator: tokenAuthenticator(token),
    // No waitOnFirstConnect: a failed first connect rejects, so connectUntil logs why and tries again.
    maxReconnectAttempts: -1,
    // By default two refused sign-ins in a row stop the client for good.
    ignoreAuthErrorAbort: true,
  };
}

/**
 * Bridges the PTY to the lane terminal's core NATS subjects (ADR 0040 §1): `input` and `resize`
 * from `pty.<session>.in` go to the PTY, what the PTY writes goes out on `pty.<session>.out`; and
 * sends the lane's events on `agent.<session>.events`, which the api's stream keeps (RAN-63). It
 * signs in as the lane, with its projected token: the auth callout lets it use exactly these
 * three subjects (ADR 0045 amendment). It connects in the background and never gives up, a refused
 * sign-in included (the callout restarting, a token not yet rotated): the lane runs without it,
 * and output while it is away is dropped (the screen redraws on the browser's next resize).
 */
export function startTerminal(o: TerminalOptions): Terminal {
  const out = ptyOut(o.org, o.sessionId);
  const events = agentEvents(o.org, o.sessionId);
  // Events from before the first connect (the sign-in ask is one), sent once it is up. Once
  // connected, the client itself holds what is sent while it reconnects.
  const pending: string[] = [];
  let nc: NatsConnection | undefined;
  let closed = false;
  void connectUntil(connectionOptions(o), () => closed, o.log).then((c) => {
    if (c === undefined) return;
    if (closed) {
      void c.close();
      return;
    }
    nc = c;
    for (const line of pending.splice(0)) c.publish(events, line);
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
    event(line) {
      if (nc) nc.publish(events, line);
      else if (pending.length < PENDING_EVENTS) pending.push(line);
      else o.log("terminal: dropped a lane event while NATS is away");
    },
    async close() {
      closed = true;
      await nc?.close();
    },
  };
}

/**
 * The lane's one NATS connection, up from the start so the first event (the sign-in ask, say)
 * reaches Rangelight, and the emitter that writes each event to stdout (kubectl logs) and to it.
 * The PTY is attached once claude runs.
 */
export function connectLane(config: LaneConfig): {
  terminal: Terminal;
  emit: Emit;
  attach: (pty: Pty) => void;
} {
  const lane: { pty?: Pty } = {};
  const terminal = startTerminal({
    ...config,
    pty: {
      write: (bytes) => lane.pty?.write(bytes),
      resize: (cols, rows) => lane.pty?.resize(cols, rows) ?? Promise.resolve(),
    },
    log: (line) => process.stderr.write(`agent: ${line}\n`),
  });
  const emit = createEmitter({
    sessionId: config.sessionId,
    clock: systemClock,
    ids: uuidv7Ids(systemClock),
    write: (line) => {
      process.stdout.write(`${line}\n`);
      terminal.event(line);
    },
  });
  const attach = (pty: Pty): void => {
    lane.pty = pty;
  };
  return { terminal, emit, attach };
}
