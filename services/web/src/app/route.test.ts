import { expect, test } from "vitest";
import { parse, sessionPath } from "./route";

test("paths map to pages, and session ids round-trip", () => {
  expect(parse("/")).toEqual({ page: "sessions" });
  expect(parse("/wall")).toEqual({ page: "wall" });
  expect(parse("/up-next")).toEqual({ page: "upnext" });
  expect(parse("/architect")).toEqual({ page: "architect" });
  expect(parse("/decisions")).toEqual({ page: "decisions" });
  expect(parse("/reviews")).toEqual({ page: "reviews" });
  expect(parse("/coordinator")).toEqual({ page: "coordinator" });
  expect(parse(sessionPath("019a3f4e-7c21"))).toEqual({ page: "session", id: "019a3f4e-7c21" });
  expect(parse(sessionPath("a/b"))).toEqual({ page: "session", id: "a/b" });
  expect(parse("/sessions/")).toEqual({ page: "missing" });
  expect(parse("/settings")).toEqual({ page: "missing" });
});
