import { randomUUID } from "node:crypto";
import { createServer, type ServerResponse } from "node:http";
import { z } from "zod";
import { shellQuote } from "./pty.ts";
import type { RuntimeEvent } from "./runtime.ts";

// The Claude Code hook inputs the agent subscribes to, with only the fields it uses.
const hookSchema = z.discriminatedUnion("hook_event_name", [
  z.object({ hook_event_name: z.literal("UserPromptSubmit") }),
  z.object({ hook_event_name: z.literal("PostToolUse"), tool_name: z.string().min(1) }),
  z.object({
    hook_event_name: z.literal("PostCompact"),
    trigger: z.enum(["auto", "manual"]),
    compact_summary: z.string(),
  }),
  z.object({ hook_event_name: z.literal("Stop") }),
  // The session is up: past the workspace-trust dialog, which holds it back until the member answers.
  z.object({ hook_event_name: z.literal("SessionStart") }),
]);

/** A validated Claude Code hook input. */
export type Hook = z.infer<typeof hookSchema>;

/** The hooks of a session's turns and tools (every one but SessionStart). */
export type TurnHook = Exclude<Hook, { hook_event_name: "SessionStart" }>;

// A PostToolUse input carries the whole tool response; past this it's refused, not buffered.
const MAX_BODY = 16 * 1024 * 1024;

/** A progress stage slug (^[a-z][a-z0-9-]{0,39}$) from a tool name. */
export function stageOf(toolName: string): string {
  const slug = toolName
    .toLowerCase()
    .replace(/[^a-z0-9-]+/g, "-")
    .replace(/^[^a-z]+/, "")
    .slice(0, 40)
    .replace(/-+$/, "");
  return slug === "" ? "tool" : slug;
}

/**
 * The runtime event for a hook. A tool call reports only the tool's name: its input and output
 * may hold anything the model saw, and events leave the pod.
 */
export function toRuntimeEvent(hook: TurnHook): RuntimeEvent {
  switch (hook.hook_event_name) {
    case "UserPromptSubmit":
      return { kind: "turn_start" };
    case "Stop":
      return { kind: "turn_end", outcome: "ended" };
    case "PostToolUse":
      return {
        kind: "lane",
        event: {
          type: "progress",
          payload: { stage: stageOf(hook.tool_name), note: `ran ${hook.tool_name}`.slice(0, 200) },
        },
      };
    case "PostCompact":
      return {
        kind: "lane",
        event: {
          type: "compacted",
          payload: {
            trigger: hook.trigger,
            summary: hook.compact_summary.slice(0, 8000) || "(empty summary)",
          },
        },
      };
  }
}

/** The Claude Code settings that send the subscribed hooks to the agent's local endpoint. */
export function hookSettings(url: string): object {
  const http = [{ type: "http", url }];
  return {
    hooks: {
      UserPromptSubmit: [{ hooks: http }],
      PostToolUse: [{ matcher: "*", hooks: http }],
      PostCompact: [{ hooks: http }],
      Stop: [{ hooks: http }],
      ...sessionStartHook(url),
    },
  };
}

// Posts a SessionStart to the endpoint in argv[1]: a fixed script, no hook input in it.
const POST_SESSION_START = `fetch(process.argv[1], { method: "POST", body: '{"hook_event_name":"SessionStart"}' }).catch(() => {});`;

/**
 * The SessionStart hook to the endpoint at `url`. Claude Code runs no HTTP hook for SessionStart, so
 * it is a command: this agent's own node posting it there. The command line holds only that fixed
 * script and `url`, the agent's own.
 */
export function sessionStartHook(url: string): {
  SessionStart: [{ hooks: [{ type: "command"; command: string }] }];
} {
  const command = [process.execPath, "-e", POST_SESSION_START, url].map(shellQuote).join(" ");
  return { SessionStart: [{ hooks: [{ type: "command", command }] }] };
}

/** A running hook endpoint. */
export interface HookServer {
  url: string;
  close(): Promise<void>;
}

function reply(res: ServerResponse, status: number): void {
  res.writeHead(status, { "content-type": "application/json" }).end("{}");
}

/**
 * Listens on 127.0.0.1 for Claude Code's HTTP hooks and passes each valid one on, in arrival
 * order. Anything else gets a 4xx and is dropped. Claude Code posts each hook and waits for the
 * reply, so hooks of one session arrive in the order they fired.
 *
 * The path is a random UUID, so another process in the pod can't post a forged Stop or
 * PostCompact by guessing the port. Ceiling: the model's tools run as the same uid and can read
 * claude's argv (which carries the URL) from /proc; the events they could forge are still only
 * what the schema allows (ADR 0045 §5).
 */
export function listenForHooks(onHook: (hook: Hook) => void): Promise<HookServer> {
  const path = `/hooks/${randomUUID()}`;
  const server = createServer((req, res) => {
    if (req.method !== "POST" || req.url !== path) {
      reply(res, 404);
      return;
    }
    const chunks: Buffer[] = [];
    let size = 0;
    req.on("data", (chunk: Buffer) => {
      size += chunk.length;
      if (size > MAX_BODY) {
        reply(res, 413);
        req.destroy();
        return;
      }
      chunks.push(chunk);
    });
    req.on("end", () => {
      const parsed = hookSchema.safeParse(parseJson(Buffer.concat(chunks).toString("utf8")));
      if (!parsed.success) {
        reply(res, 400);
        return;
      }
      onHook(parsed.data);
      reply(res, 200);
    });
  });
  return new Promise((resolve, reject) => {
    server.on("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      if (address === null || typeof address === "string") {
        reject(new Error("hooks: no TCP port"));
        return;
      }
      resolve({
        url: `http://127.0.0.1:${String(address.port)}${path}`,
        close: () =>
          new Promise((done) => {
            server.close(() => {
              done();
            });
            server.closeAllConnections();
          }),
      });
    });
  });
}

function parseJson(text: string): unknown {
  try {
    return JSON.parse(text);
  } catch {
    return undefined;
  }
}
