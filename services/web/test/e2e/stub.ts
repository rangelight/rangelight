// A stand-in for the api, built from the real contracts: SessionService from api/proto (via
// shared/ts/gen) over connect-node, and a terminal WebSocket that echoes input as output. Tests
// steer it through POST /__stub/<command>; it is never part of the build.
import { createServer, type IncomingMessage, type Server } from "node:http";
import { create } from "@bufbuild/protobuf";
import { timestampFromMs } from "@bufbuild/protobuf/wkt";
import { Code, ConnectError } from "@connectrpc/connect";
import { connectNodeAdapter } from "@connectrpc/connect-node";
import { DesiredState, Phase } from "@gen/proto/rangelight/runner/v1alpha1/runner_pb.ts";
import {
  ChangeSchema,
  EventSchema,
  type Event,
  type Session,
  SessionSchema,
  SessionService,
  type WatchSessionsResponse,
  WatchSessionsResponseSchema,
} from "@gen/proto/rangelight/session/v1alpha1/session_pb.ts";
import { type WebSocket, WebSocketServer } from "ws";
import { z } from "zod";

export const STUB_PORT = 4180;

const StubSessionSchema = z.object({
  id: z.string(),
  ticketKey: z.string(),
  title: z.string(),
  repo: z.string(),
  state: z.enum(DesiredState),
  minutesAgo: z.number(),
});
/** A session as tests describe it. */
export type StubSession = z.infer<typeof StubSessionSchema>;
const Reset = z.object({
  sessions: z.array(StubSessionSchema).default([]),
  fail: z.boolean().default(false),
});
const Emit = z.object({
  sessionId: z.string(),
  state: z.enum(DesiredState),
  atMs: z.number().optional(),
});
const Abort = z.object({ sessionId: z.string() });
const Report = z.object({
  sessionId: z.string(),
  phase: z.enum(Phase),
  message: z.string().default(""),
  pullRequestUrl: z.string().default(""),
});

interface Watcher {
  push: (e: Event) => void;
  abort: () => void;
}

const world: {
  sessions: Session[];
  fail: boolean;
  watchers: Map<string, Set<Watcher>>;
  lastResize: { cols: number; rows: number } | null;
  /** Every byte the terminal sent since the reset, as UTF-8. */
  input: string;
} = { sessions: [], fail: false, watchers: new Map(), lastResize: null, input: "" };

const toSession = (s: StubSession): Session =>
  create(SessionSchema, {
    ...s,
    tracker: "linear",
    ticketId: `issue-${s.ticketKey}`,
    ticketUrl: `https://linear.app/example/issue/${s.ticketKey}`,
    namespace: `rl-${s.ticketKey.toLowerCase()}`,
    generation: 1n,
    createdAt: timestampFromMs(Date.now() - s.minutesAgo * 60_000),
  });

/** The watchers of every session's events. */
const ALL = "*";

/** A watcher's events (of one session, or ALL), ending like the api's: ABORTED with the
 * events_missed reason. */
async function* watch(key: string, signal: AbortSignal): AsyncGenerator<Event> {
  const queue: Event[] = [];
  let wake = (): void => undefined;
  const ended = { missed: false };
  const watcher: Watcher = {
    push: (e) => {
      queue.push(e);
      wake();
    },
    abort: () => {
      ended.missed = true;
      wake();
    },
  };
  const set = world.watchers.get(key) ?? new Set();
  world.watchers.set(key, set.add(watcher));
  try {
    while (!signal.aborted) {
      const next = queue.shift();
      if (next) yield next;
      else if (ended.missed)
        throw new ConnectError("events_missed: reread the session and watch again", Code.Aborted);
      else
        await new Promise<void>((resolve) => {
          wake = resolve;
          signal.addEventListener("abort", () => {
            resolve();
          });
        });
    }
  } finally {
    set.delete(watcher);
  }
}

function publish(event: Event): void {
  for (const key of [event.sessionId, ALL])
    world.watchers.get(key)?.forEach((w) => {
      w.push(event);
    });
}

/** The session's next event, committed and fanned out. */
function commit(sessionId: string, change: Event["change"], atMs = Date.now()): Session {
  const session = world.sessions.find((s) => s.id === sessionId);
  if (!session) throw new Error(`no session ${sessionId}`);
  session.generation += 1n;
  publish(
    create(EventSchema, {
      id: crypto.randomUUID(),
      sessionId,
      seq: session.generation,
      at: timestampFromMs(atMs),
      ...(change && { change }),
    }),
  );
  return session;
}

function emit(sessionId: string, state: DesiredState, atMs = Date.now()): void {
  const session = world.sessions.find((s) => s.id === sessionId);
  if (session) session.state = state;
  commit(
    sessionId,
    create(ChangeSchema, { kind: { case: "stateChanged", value: { state } } }),
    atMs,
  );
}

/** A runner's report, recorded as the api records it: the session keeps its first PR. */
function report(sessionId: string, phase: Phase, message: string, pullRequestUrl: string): void {
  const session = world.sessions.find((s) => s.id === sessionId);
  const recorded = session?.pullRequestUrl ? "" : pullRequestUrl;
  if (session) {
    session.phase = phase;
    session.pullRequestUrl ||= recorded;
  }
  commit(
    sessionId,
    create(ChangeSchema, {
      kind: { case: "phaseChanged", value: { phase, message, pullRequestUrl: recorded } },
    }),
  );
}

/** A session created now: listed, and announced to every watcher like the relay's fan-out. */
function add(s: StubSession): void {
  const session = toSession(s);
  world.sessions.unshift(session);
  publish(
    create(EventSchema, {
      id: crypto.randomUUID(),
      sessionId: session.id,
      seq: 1n,
      ...(session.createdAt && { at: session.createdAt }),
      change: {
        kind: {
          case: "created",
          value: {
            ticketId: session.ticketId,
            ticketKey: session.ticketKey,
            title: session.title,
            ticketUrl: session.ticketUrl,
            namespace: session.namespace,
            state: session.state,
          },
        },
      },
    }),
  );
}

async function* watchAll(signal: AbortSignal): AsyncGenerator<WatchSessionsResponse> {
  if (world.fail) throw new ConnectError("store_unavailable", Code.Unavailable);
  // Subscribed (the generator started) before the snapshot is taken, as the api does.
  const changes = watch(ALL, signal);
  const first = changes.next();
  yield create(WatchSessionsResponseSchema, {
    kind: { case: "snapshot", value: { sessions: world.sessions } },
  });
  for (let next = await first; !next.done; next = await changes.next())
    yield create(WatchSessionsResponseSchema, { kind: { case: "change", value: next.value } });
}

const handler = connectNodeAdapter({
  routes: (router) => {
    router.service(SessionService, {
      getSession: (req) => {
        if (world.fail) throw new ConnectError("store_unavailable", Code.Unavailable);
        const session = world.sessions.find((s) => s.id === req.sessionId);
        if (!session) throw new ConnectError("session_not_found", Code.NotFound);
        return { session };
      },
      watchSession: async function* (req, ctx) {
        // The api's headers go out as the stream opens; connect-node holds them until the first
        // message, so the stub opens with an empty one (the Workbench skips it) to match.
        const events = watch(req.sessionId, ctx.signal);
        const first = events.next();
        yield {};
        for (let next = await first; !next.done; next = await events.next())
          yield { event: next.value };
      },
      watchSessions: (_req, ctx) => watchAll(ctx.signal),
    });
  },
});

async function body(req: IncomingMessage): Promise<unknown> {
  let text = "";
  for await (const chunk of req) text += String(chunk);
  return JSON.parse(text || "{}");
}

// The test's control commands. A switch, not a lookup table: a request's command name never
// selects a property, so it can't reach anything but these.
function command(name: string, b: unknown): unknown {
  switch (name) {
    case "reset": {
      const { sessions, fail } = Reset.parse(b);
      world.fail = fail;
      world.sessions = sessions.map(toSession);
      world.lastResize = null;
      world.input = "";
      return {};
    }
    case "emit": {
      const { sessionId, state, atMs } = Emit.parse(b);
      emit(sessionId, state, atMs);
      return {};
    }
    case "create":
      add(StubSessionSchema.parse(b));
      return {};
    case "report": {
      const { sessionId, phase, message, pullRequestUrl } = Report.parse(b);
      report(sessionId, phase, message, pullRequestUrl);
      return {};
    }
    case "abort":
      world.watchers.get(Abort.parse(b).sessionId)?.forEach((w) => {
        w.abort();
      });
      return {};
    case "state":
      return {
        lastResize: world.lastResize,
        input: world.input,
        watching: [...world.watchers.values()].reduce((n, s) => n + s.size, 0),
      };
    default:
      return undefined;
  }
}

function terminal(ws: WebSocket): void {
  ws.on("message", (raw: Buffer) => {
    const frame: unknown = JSON.parse(raw.toString());
    if (typeof frame !== "object" || frame === null || !("type" in frame)) {
      ws.close(1008);
      return;
    }
    if (frame.type === "input" && "data" in frame) {
      world.input += Buffer.from(String(frame.data), "base64").toString("utf8");
      ws.send(JSON.stringify({ type: "output", data: frame.data }));
    } else if (frame.type === "resize" && "cols" in frame && "rows" in frame)
      world.lastResize = { cols: Number(frame.cols), rows: Number(frame.rows) };
    else ws.close(1008);
  });
}

export function startStub(): Promise<Server> {
  const sockets = new WebSocketServer({
    noServer: true,
    handleProtocols: (p) =>
      p.has("terminal.v1alpha1.rangelight.dev") ? "terminal.v1alpha1.rangelight.dev" : false,
  });
  const server = createServer((req, res) => {
    const name = /^\/__stub\/(reset|emit|create|report|abort|state)$/.exec(req.url ?? "")?.[1];
    if (name === undefined) {
      handler(req, res);
      return;
    }
    void body(req).then((b) => {
      res.setHeader("content-type", "application/json");
      res.end(JSON.stringify(command(name, b)));
    });
  });
  server.on("upgrade", (req, socket, head) => {
    if (!/^\/v1alpha1\/sessions\/[^/]+\/terminal$/.test(req.url ?? "")) {
      socket.destroy();
      return;
    }
    sockets.handleUpgrade(req, socket, head, terminal);
  });
  return new Promise((resolve) =>
    server.listen(STUB_PORT, "127.0.0.1", () => {
      resolve(server);
    }),
  );
}
