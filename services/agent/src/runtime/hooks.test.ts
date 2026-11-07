import { expect, test } from "vitest";
import { listenForHooks, stageOf, toRuntimeEvent, type Hook } from "./hooks.ts";

test("AC6 hooks map to runtime events; tool input and output never leave the pod", () => {
  expect(toRuntimeEvent({ hook_event_name: "UserPromptSubmit" })).toEqual({ kind: "turn_start" });
  expect(toRuntimeEvent({ hook_event_name: "Stop" })).toEqual({
    kind: "turn_end",
    outcome: "ended",
  });
  expect(
    toRuntimeEvent({ hook_event_name: "PostToolUse", tool_name: "mcp__github__get_issue" }),
  ).toEqual({
    kind: "lane",
    event: {
      type: "progress",
      payload: { stage: "mcp-github-get-issue", note: "ran mcp__github__get_issue" },
    },
  });
  const compacted = toRuntimeEvent({
    hook_event_name: "PostCompact",
    trigger: "auto",
    compact_summary: "x".repeat(9000),
  });
  expect(compacted).toEqual({
    kind: "lane",
    event: { type: "compacted", payload: { trigger: "auto", summary: "x".repeat(8000) } },
  });
});

test("a stage is always a valid slug", () => {
  expect(stageOf("Bash")).toBe("bash");
  expect(stageOf("42")).toBe("tool");
  expect(stageOf(`${"a".repeat(39)}__b`)).toBe("a".repeat(39));
});

test("AC6 the hook endpoint passes valid hooks on and refuses anything else", async () => {
  const got: Hook[] = [];
  const server = await listenForHooks((h) => got.push(h));
  const post = (body: string, path?: string) =>
    fetch(path === undefined ? server.url : new URL(path, server.url), {
      method: "POST",
      body,
    }).then((r) => r.status);
  try {
    expect(await post(JSON.stringify({ hook_event_name: "Stop", session_id: "x" }))).toBe(200);
    expect(await post("not json")).toBe(400);
    expect(await post(JSON.stringify({ hook_event_name: "PreToolUse" }))).toBe(400);
    expect(await post(JSON.stringify({ hook_event_name: "PostToolUse" }))).toBe(400);
    expect(await post("{}", "/other")).toBe(404);
    expect(await post(JSON.stringify({ hook_event_name: "Stop" }), "/hooks")).toBe(404);
    expect(got).toEqual([{ hook_event_name: "Stop" }]);
  } finally {
    await server.close();
  }
});
