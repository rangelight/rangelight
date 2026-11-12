import { expect, test } from "vitest";
import { keyBytes } from "./KeyBar";

test("arrows follow the cursor-key mode; other keys are sent as they are", () => {
  expect(keyBytes("A", false)).toBe("\x1b[A");
  expect(keyBytes("D", true)).toBe("\x1bOD");
  expect(keyBytes("\x1b[Z", true)).toBe("\x1b[Z");
  expect(keyBytes("/", true)).toBe("/");
});
