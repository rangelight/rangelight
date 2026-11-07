import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, onTestFinished, test } from "vitest";
import { childEnv, ConfigError, loadConfig } from "./config.ts";

const env = {
  RANGELIGHT_AGENT_SESSION_ID: "019a3f4e-7c21-7d10-9a3b-2f5e6c7d8a90",
  RANGELIGHT_AGENT_MIRROR_URL: "http://git-mirror.rangelight-system:8080/",
  RANGELIGHT_AGENT_REPO: "opentelemetry-demo",
  RANGELIGHT_AGENT_UNDER_CHANGE: "frontend,cart",
  CLAUDE_CONFIG_DIR: "/home/node/.claude",
};
const none = "/nonexistent/agent.toml";

test("config: defaults from configs/agent.default.toml, then the environment", () => {
  const config = loadConfig(env, none);
  expect(config).toEqual({
    sessionId: env.RANGELIGHT_AGENT_SESSION_ID,
    mirrorUrl: "http://git-mirror.rangelight-system:8080",
    cloneUrl: "http://git-mirror.rangelight-system:8080/opentelemetry-demo.git",
    pushUrl: "http://git-mirror.rangelight-system:8080/session/opentelemetry-demo.git",
    baseRef: "main",
    tokenFile: "/var/run/secrets/rangelight/git-mirror/token",
    workdir: "/workspace/repo",
    profileFile: "/etc/rangelight/profile.json",
    ticketFile: "/etc/rangelight/ticket.json",
    underChange: ["frontend", "cart"],
    claudeConfigDir: "/home/node/.claude",
  });
});

test("config: the mounted file overrides the defaults and the environment overrides both", () => {
  const dir = mkdtempSync(join(tmpdir(), "rl-agent-config-"));
  onTestFinished(() => {
    rmSync(dir, { recursive: true, force: true });
  });
  const file = join(dir, "agent.toml");
  writeFileSync(file, 'base_ref = "develop"\nworkdir = "/w"\n');
  const config = loadConfig({ ...env, RANGELIGHT_AGENT_WORKDIR: "/x" }, file);
  expect([config.baseRef, config.workdir]).toEqual(["develop", "/x"]);
});

test("config: an invalid or unknown value fails with every bad key named", () => {
  const bad = {
    ...env,
    RANGELIGHT_AGENT_SESSION_ID: "s1",
    RANGELIGHT_AGENT_TOKEN_FILE: "/t'oken",
    RANGELIGHT_AGENT_COLOUR: "x",
  };
  expect(() => loadConfig(bad, none)).toThrow(ConfigError);
  expect(() => loadConfig(bad, none)).toThrow(/session_id[\s\S]*token_file|colour/);
  expect(() => loadConfig({ ...env, RANGELIGHT_AGENT_MIRROR_URL: "file:///etc" }, none)).toThrow(
    /mirror_url/,
  );
});

test("config: children get only PATH, HOME and LANG, never an API key", () => {
  const out = childEnv({
    PATH: "/bin",
    HOME: "/h",
    ANTHROPIC_API_KEY: "k",
    RANGELIGHT_AGENT_REPO: "r",
  });
  expect(out).toEqual({ PATH: "/bin", HOME: "/h", LANG: "C.UTF-8" });
});
