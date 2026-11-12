// The agent's typed config, and the only module that reads the environment (ADR 0049 §6).
import { existsSync, readFileSync } from "node:fs";
import { parse as parseToml } from "smol-toml";
import { z } from "zod";

const DEFAULTS = new URL("../../../configs/agent.default.toml", import.meta.url);
const MOUNTED = "/etc/rangelight/agent.toml";
const PREFIX = "RANGELIGHT_AGENT_";

const uuidv7 = /^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const absolute = z.string().regex(/^\/[A-Za-z0-9._/-]*$/, "an absolute path of [A-Za-z0-9._/-]");
// The same rule as rl_domain::subject::token: a token can't widen or split a subject.
const subjectToken = z.string().regex(/^[^.*>\s\p{Cc}]+$/u, "a NATS subject token");
// The same rule as rl_nats::check_url: the lane's token travels only over TLS, or in clear text
// only to a Service in this cluster by its full name (the lane resolves only exact names) or to
// loopback. One server, no credentials, no path.
const natsUrl = z.url({ protocol: /^(nats|tls)$/ }).refine((u) => {
  const url = URL.parse(u);
  if (!url || url.username || url.password || !["", "/"].includes(url.pathname)) return false;
  const host = url.hostname;
  if (!/^[A-Za-z0-9.-]+$|^\[::1\]$/.test(host) || /[,?#]/.test(u) || url.port === "0") return false;
  const local =
    /^.+\.svc\.cluster\.local$/.test(host) || ["localhost", "127.0.0.1", "[::1]"].includes(host);
  return url.protocol === "tls:" || local;
}, "tls://<host>[:<port>], or nats:// to a Service in this cluster by its full name (<name>.<namespace>.svc.cluster.local: a lane can't resolve a bare .svc) or on loopback, without credentials");

const schema = z
  .object({
    session_id: z.string().regex(uuidv7, "a lowercase UUIDv7"),
    mirror_url: z.url({ protocol: /^https?$/ }).transform((u) => u.replace(/\/+$/, "")),
    repo: z.string().regex(/^[A-Za-z0-9][A-Za-z0-9._-]*$/, "a mirror repo name"),
    base_ref: z.string().regex(/^[A-Za-z0-9][A-Za-z0-9._/-]*$/, "a branch name"),
    token_file: absolute,
    workdir: absolute,
    profile_file: absolute,
    ticket_file: absolute,
    under_change: z.array(z.string().min(1)),
    nats_url: natsUrl,
    nats_token_file: absolute,
    org: subjectToken,
    CLAUDE_CONFIG_DIR: absolute,
  })
  .strict()
  .transform((c) => ({
    sessionId: c.session_id,
    mirrorUrl: c.mirror_url,
    cloneUrl: `${c.mirror_url}/${c.repo}.git`,
    pushUrl: `${c.mirror_url}/session/${c.repo}.git`,
    baseRef: c.base_ref,
    tokenFile: c.token_file,
    workdir: c.workdir,
    profileFile: c.profile_file,
    ticketFile: c.ticket_file,
    underChange: c.under_change,
    natsUrl: c.nats_url,
    natsTokenFile: c.nats_token_file,
    org: c.org,
    claudeConfigDir: c.CLAUDE_CONFIG_DIR,
  }));

/** The agent's validated config. */
export type Config = z.infer<typeof schema>;

/** The environment of every process the agent starts: only these variables pass through. */
export type ChildEnv = Record<string, string>;

/** A config file or variable failed validation; the message names every bad key. */
export class ConfigError extends Error {
  override name = "ConfigError";
}

type Env = Record<string, string | undefined>;

function readToml(path: string | URL): Record<string, unknown> {
  try {
    return parseToml(readFileSync(path, "utf8"));
  } catch (error) {
    throw new ConfigError(`${path.toString()}: ${String(error)}`);
  }
}

function fromEnv(env: Env): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const [name, value] of Object.entries(env)) {
    if (!name.startsWith(PREFIX) || value === undefined) continue;
    const key = name.slice(PREFIX.length).toLowerCase();
    out[key] = key === "under_change" ? value.split(",").filter((s) => s !== "") : value;
  }
  // Claude Code reads CLAUDE_CONFIG_DIR itself, so the variable keeps its name (ADR 0049 §6).
  if (env.CLAUDE_CONFIG_DIR !== undefined) out.CLAUDE_CONFIG_DIR = env.CLAUDE_CONFIG_DIR;
  return out;
}

/**
 * Loads the config: configs/agent.default.toml, then the mounted file if present, then
 * RANGELIGHT_AGENT_* and CLAUDE_CONFIG_DIR. Throws {@link ConfigError} on any invalid value.
 */
export function loadConfig(env: Env = process.env, mounted = MOUNTED): Config {
  const layers = { ...readToml(DEFAULTS), ...(existsSync(mounted) ? readToml(mounted) : {}) };
  const result = schema.safeParse({ ...layers, ...fromEnv(env) });
  if (!result.success) throw new ConfigError(z.prettifyError(result.error));
  return result.data;
}

/**
 * The base environment for git, the dev commands and Claude Code. Nothing else leaks through: no
 * API key reaches Claude Code (ADR 0046 amendment 2026-11-07: plan sign-in only).
 */
export function childEnv(env: Env = process.env): ChildEnv {
  return {
    PATH: env.PATH ?? "/usr/local/bin:/usr/bin:/bin",
    HOME: env.HOME ?? "/home/node",
    LANG: env.LANG ?? "C.UTF-8",
  };
}

/** The real nats-server of a test run: `scripts/test/services.sh run` starts it and sets this. */
export function testNatsUrl(env: Env = process.env): string {
  const url = env.RANGELIGHT_TESTKIT_NATS_URL;
  if (url === undefined) throw new ConfigError("run the tests under scripts/test/services.sh run");
  return url;
}
