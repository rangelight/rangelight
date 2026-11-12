import { Code, ConnectError } from "@connectrpc/connect";
import { expect, test, vi } from "vitest";

// The real client and the real describe(): the guard reads the reason describe() writes.
vi.stubGlobal("location", { origin: "http://127.0.0.1" });
const { describe } = await import("@/api/live");
const { retries } = await import("./store");

const reason = (code: Code) => describe(new ConnectError("x", code));

test("a stopped or failed stream retries on its own, unless the login is missing", () => {
  expect(retries({ status: "error", reason: reason(Code.Unavailable) })).toBe(true);
  // RAN-71's redirect owns that one; a timed retry would defeat its loop guard.
  expect(retries({ status: "error", reason: reason(Code.Unauthenticated) })).toBe(false);
  expect(retries({ status: "ready", sessions: [], paused: "stopped" })).toBe(true);
  expect(retries({ status: "ready", sessions: [], paused: null })).toBe(false);
});
